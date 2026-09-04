//! Live, read-only GitHub observations using an authorized workspace source.

use async_trait::async_trait;
use chrono::Utc;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::WorkspaceScope;
use crate::db::{sources, workspace_members};

const ORIGIN: &str = "https://api.github.com/";
const PAGE_SIZE: usize = 100;

#[derive(Clone, Copy)]
enum Operation {
    Build,
    Deployments,
    Issues,
    File,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    for operation in [
        Operation::Build,
        Operation::Deployments,
        Operation::Issues,
        Operation::File,
    ] {
        registry.register(Arc::new(Integration {
            scope: scope.clone(),
            operation,
        }));
    }
}

struct Integration {
    scope: WorkspaceScope,
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    source_id: Uuid,
    #[serde(rename = "ref")]
    reference: Option<String>,
    path: Option<String>,
    page: Option<u32>,
    state: Option<String>,
}

#[derive(Deserialize)]
struct Configuration {
    owner: String,
    repo: String,
    branch: Option<String>,
    path: Option<String>,
    token: Option<String>,
}

#[async_trait]
impl Tool for Integration {
    fn name(&self) -> &str {
        match self.operation {
            Operation::Build => "get_build_status",
            Operation::Deployments => "list_deployments",
            Operation::Issues => "list_issues",
            Operation::File => "read_repository_file",
        }
    }

    fn description(&self) -> &str {
        match self.operation {
            Operation::Build => {
                "Read live GitHub workflows, check runs and commit statuses at an immutable commit from a connected workspace source. Missing or incomplete evidence is never green; this is not proof that branch protection requirements are satisfied."
            }
            Operation::Deployments => {
                "Read live GitHub deployments and their latest statuses for a connected source at an immutable commit. Results are paginated; deployment records do not prove the deployed service is healthy."
            }
            Operation::Issues => {
                "Read live GitHub issues (excluding pull requests) from a connected workspace source. Return full issue bodies and a next page when more provider records exist."
            }
            Operation::File => {
                "Read the complete UTF-8 content of a specific repository file from a connected GitHub source at an immutable commit, with a source URL. Does not read host files. GitHub files over 100 MB are unsupported."
            }
        }
    }

    fn parameters_schema(&self) -> Value {
        let mut properties = json!({"source_id": {"type": "string", "format": "uuid"}});
        if !matches!(self.operation, Operation::Issues) {
            properties["ref"] = json!({"type": "string", "description": "Branch, tag or commit; defaults to the source branch or repository default branch."});
        }
        if matches!(self.operation, Operation::Deployments | Operation::Issues) {
            properties["page"] = json!({"type": "integer", "minimum": 1, "description": "Provider page (100 records), default 1. Follow next_page until null."});
        }
        if matches!(self.operation, Operation::Issues) {
            properties["state"] = json!({"type": "string", "enum": ["open", "closed", "all"]});
        }
        let mut required = vec!["source_id"];
        if matches!(self.operation, Operation::File) {
            properties["path"] = json!({"type": "string", "description": "Exact repository-relative file path, within the configured source path."});
            required.push("path");
        }
        json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(match self.run(params).await {
            Ok(value) => ToolResult::success(value.to_string()),
            Err(error) => ToolResult::error(error),
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(120)
    }
}

impl Integration {
    async fn run(&self, params: Value) -> Result<Value, String> {
        let arguments: Arguments = serde_json::from_value(params)
            .map_err(|_| "Invalid integration arguments.".to_string())?;
        if arguments.page == Some(0) {
            return Err("page must be positive.".to_string());
        }
        if !workspace_members::can_read(
            self.scope.state.db(),
            self.scope.workspace_id,
            self.scope.user_id,
        )
        .await
        .map_err(|_| "Workspace authorization failed.".to_string())?
        {
            return Err("You cannot read this workspace.".to_string());
        }
        let source = sources::get_source(
            self.scope.state.db(),
            arguments.source_id,
            self.scope.workspace_id,
        )
        .await
        .map_err(|_| "Could not read the source.".to_string())?
        .filter(|source| source.is_active.unwrap_or(true))
        .ok_or("Source not found in this workspace or inactive.")?;
        if source.source_type != "github" {
            return Err(
                "Live integrations currently support connected GitHub sources only.".to_string(),
            );
        }
        let mut configuration: Configuration = serde_json::from_value(source.config)
            .map_err(|_| "The GitHub source configuration is invalid.".to_string())?;
        if let Some(encrypted) = source.credentials_encrypted {
            configuration.token = Some(
                crate::crypto::decrypt(self.scope.state.encryption_key(), &encrypted)
                    .map_err(|_| "The source credentials could not be decrypted.".to_string())?,
            );
        }
        let github = Github::new(configuration)?;
        let mut result = github.observe(self.operation, &arguments).await?;
        result["source_id"] = json!(arguments.source_id);
        result["observed_at"] = json!(Utc::now().to_rfc3339());
        Ok(result)
    }
}

struct Github {
    client: Client,
    origin: Url,
    configuration: Configuration,
}

impl Github {
    fn new(configuration: Configuration) -> Result<Self, String> {
        if !segment(&configuration.owner) || !segment(&configuration.repo) {
            return Err("The source owner or repository name is invalid.".to_string());
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent("Zone-workspace-tools")
            .build()
            .map_err(|_| "Could not initialize the GitHub client.".to_string())?;
        Ok(Self {
            client,
            origin: Url::parse(ORIGIN).expect("constant GitHub origin"),
            configuration,
        })
    }

    fn url(&self, parts: &[&str]) -> Url {
        let mut url = self.origin.clone();
        let mut segments = url
            .path_segments_mut()
            .expect("HTTP origin supports path segments");
        segments.pop_if_empty().extend([
            "repos",
            &self.configuration.owner,
            &self.configuration.repo,
        ]);
        segments.extend(parts.iter().copied());
        drop(segments);
        url
    }

    async fn response(
        &self,
        parts: &[&str],
        query: &[(&str, String)],
        raw: bool,
    ) -> Result<reqwest::Response, String> {
        let mut url = self.url(parts);
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        let mut request = self
            .client
            .get(url)
            .header(
                "Accept",
                if raw {
                    "application/vnd.github.raw+json"
                } else {
                    "application/vnd.github+json"
                },
            )
            .header("X-GitHub-Api-Version", "2026-03-10");
        if let Some(token) = &self.configuration.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "GitHub request failed or timed out.".to_string())?;
        if !response.status().is_success() {
            // Provider response bodies and request errors can contain secrets.
            return Err(format!(
                "GitHub returned HTTP {}. Check source access, permissions, rate limits and the requested resource.",
                response.status().as_u16()
            ));
        }
        Ok(response)
    }

    async fn get(&self, parts: &[&str], query: &[(&str, String)]) -> Result<Value, String> {
        self.response(parts, query, false)
            .await?
            .json()
            .await
            .map_err(|_| "GitHub returned an invalid response.".to_string())
    }

    async fn resolve(&self, reference: Option<&str>) -> Result<(String, String), String> {
        let reference = match reference.or(self.configuration.branch.as_deref()) {
            Some(value) if !value.trim().is_empty() => value.to_string(),
            Some(_) => return Err("ref must not be empty.".to_string()),
            None => self.get(&[], &[]).await?["default_branch"]
                .as_str()
                .ok_or("GitHub did not return a default branch.")?
                .to_string(),
        };
        let commit = self.get(&["commits", &reference], &[]).await?;
        let sha = commit["sha"]
            .as_str()
            .filter(|value| valid_sha(value))
            .ok_or("GitHub did not return an immutable commit SHA.")?
            .to_string();
        Ok((reference, sha))
    }

    async fn observe(&self, operation: Operation, arguments: &Arguments) -> Result<Value, String> {
        let repository = format!(
            "https://github.com/{}/{}",
            self.configuration.owner, self.configuration.repo
        );
        if matches!(operation, Operation::Issues) {
            let state = arguments.state.as_deref().unwrap_or("open");
            if !matches!(state, "open" | "closed" | "all") {
                return Err("Issue state must be open, closed or all.".to_string());
            }
            let page = arguments.page.unwrap_or(1);
            let records = self
                .get(
                    &["issues"],
                    &[
                        ("state", state.into()),
                        ("per_page", PAGE_SIZE.to_string()),
                        ("page", page.to_string()),
                    ],
                )
                .await?;
            let records = array(&records)?;
            let next = next_page(records.len(), page)?;
            let issues: Vec<Value> = records
                .iter()
                .filter(|row| row.get("pull_request").is_none())
                .map(|row| {
                    project(
                        row,
                        &[
                            "number",
                            "title",
                            "body",
                            "state",
                            "html_url",
                            "created_at",
                            "updated_at",
                            "closed_at",
                            "labels",
                            "assignees",
                        ],
                    )
                })
                .collect();
            return Ok(json!({"repository": repository, "issues": issues, "next_page": next}));
        }
        let (reference, sha) = self.resolve(arguments.reference.as_deref()).await?;
        let mut result = match operation {
            Operation::Build => self.build(&sha).await?,
            Operation::Deployments => self.deployments(&sha, arguments.page.unwrap_or(1)).await?,
            Operation::File => {
                self.file(&sha, arguments.path.as_deref().ok_or("path is required.")?)
                    .await?
            }
            Operation::Issues => unreachable!(),
        };
        result["repository"] = json!(repository);
        result["ref"] = json!(reference);
        result["sha"] = json!(sha);
        Ok(result)
    }

    async fn pages(
        &self,
        parts: &[&str],
        key: Option<&str>,
        query: &[(&str, String)],
    ) -> Result<Vec<Value>, String> {
        let mut page = 1;
        let mut records = Vec::new();
        loop {
            let mut query = query.to_vec();
            query.extend([
                ("per_page", PAGE_SIZE.to_string()),
                ("page", page.to_string()),
            ]);
            let response = self.get(parts, &query).await?;
            let rows = array(key.map_or(&response, |key| &response[key]))?;
            let next = next_page(rows.len(), page)?;
            records.extend(rows.iter().cloned());
            // Workflow searches are capped by GitHub at 1,000 records.
            if key == Some("workflow_runs") && response["total_count"].as_u64().unwrap_or(0) > 1000
            {
                return Err("GitHub's 1,000-result workflow search limit prevents a complete build assessment.".into());
            }
            match next {
                Some(next) => page = next,
                None => return Ok(records),
            }
        }
    }

    async fn build(&self, sha: &str) -> Result<Value, String> {
        let workflows = self
            .pages(
                &["actions", "runs"],
                Some("workflow_runs"),
                &[("head_sha", sha.into())],
            )
            .await?;
        let checks = self
            .pages(
                &["commits", sha, "check-runs"],
                Some("check_runs"),
                &[("filter", "latest".into())],
            )
            .await?;
        let statuses = self.pages(&["commits", sha, "statuses"], None, &[]).await?;
        if workflows
            .iter()
            .any(|row| row["workflow_id"].as_u64().is_none())
            || statuses
                .iter()
                .any(|row| row["context"].as_str().is_none_or(str::is_empty))
        {
            return Err("GitHub returned CI records without their identities.".into());
        }
        let mut identities = std::collections::HashSet::new();
        let workflows: Vec<Value> = workflows
            .into_iter()
            .filter(|row| {
                identities.insert((
                    row["workflow_id"].to_string(),
                    row["event"].to_string(),
                    row["head_branch"].to_string(),
                ))
            })
            .collect();
        let statuses = latest(statuses, "context");
        if workflows
            .iter()
            .chain(checks.iter())
            .any(|row| row["head_sha"].as_str() != Some(sha))
        {
            return Err("GitHub returned checks for a different or missing commit SHA.".into());
        }
        let conclusions: Vec<&str> = workflows
            .iter()
            .chain(checks.iter())
            .map(|row| {
                if row["status"] == "completed" {
                    row["conclusion"].as_str().unwrap_or("unknown")
                } else {
                    "pending"
                }
            })
            .chain(
                statuses
                    .iter()
                    .map(|row| row["state"].as_str().unwrap_or("unknown")),
            )
            .collect();
        let state = assessment(&conclusions);
        Ok(json!({"state": state, "complete": true,
            "assessment": "Observed CI only; required branch checks and service health are not evaluated.",
            "workflows": workflows.iter().map(|row| project(row, &["id", "name", "head_sha", "status", "conclusion", "html_url", "updated_at"])).collect::<Vec<_>>(),
            "checks": checks.iter().map(|row| project(row, &["id", "name", "head_sha", "status", "conclusion", "html_url", "details_url", "completed_at"])).collect::<Vec<_>>(),
            "statuses": statuses.iter().map(|row| project(row, &["context", "state", "description", "target_url", "created_at"])).collect::<Vec<_>>() }))
    }

    async fn deployments(&self, sha: &str, page: u32) -> Result<Value, String> {
        let records = self
            .get(
                &["deployments"],
                &[
                    ("sha", sha.into()),
                    ("per_page", PAGE_SIZE.to_string()),
                    ("page", page.to_string()),
                ],
            )
            .await?;
        let records = array(&records)?;
        let mut deployments = Vec::new();
        for row in records {
            if row["sha"].as_str() != Some(sha) {
                return Err(
                    "GitHub returned a deployment for a different or missing commit SHA.".into(),
                );
            }
            let id = row["id"]
                .as_u64()
                .ok_or("GitHub returned an invalid deployment ID.")?
                .to_string();
            let response = self
                .get(
                    &["deployments", &id, "statuses"],
                    &[("per_page", "1".into())],
                )
                .await?;
            let status = array(&response)?.first().cloned().unwrap_or(Value::Null);
            let mut deployment = project(
                row,
                &[
                    "id",
                    "sha",
                    "ref",
                    "environment",
                    "created_at",
                    "updated_at",
                ],
            );
            deployment["status"] = project(
                &status,
                &[
                    "state",
                    "description",
                    "environment_url",
                    "log_url",
                    "created_at",
                ],
            );
            deployment["url"] = json!(self.url(&["deployments", &id]).to_string());
            deployments.push(deployment);
        }
        Ok(json!({"deployments": deployments, "next_page": next_page(records.len(), page)?}))
    }

    async fn file(&self, sha: &str, path: &str) -> Result<Value, String> {
        validate_path(path, self.configuration.path.as_deref())?;
        let mut tree = sha.to_string();
        let mut metadata = Value::Null;
        let components: Vec<&str> = path.split('/').collect();
        for (index, component) in components.iter().enumerate() {
            let response = self.get(&["git", "trees", &tree], &[]).await?;
            if response["truncated"] != false {
                return Err("GitHub returned an incomplete repository tree.".into());
            }
            metadata = array(&response["tree"])?
                .iter()
                .find(|entry| entry["path"].as_str() == Some(component))
                .cloned()
                .ok_or("The file path was not found in this commit.")?;
            let last = index + 1 == components.len();
            if (last
                && (metadata["type"] != "blob"
                    || !matches!(metadata["mode"].as_str(), Some("100644" | "100755"))))
                || (!last && metadata["type"] != "tree")
            {
                return Err("The path must identify a regular repository file without symlinks or submodules.".into());
            }
            tree = metadata["sha"]
                .as_str()
                .filter(|value| valid_sha(value))
                .ok_or("GitHub did not return a valid tree or file SHA.")?
                .to_string();
        }
        let blob = tree;
        let bytes = self
            .response(&["git", "blobs", &blob], &[], true)
            .await?
            .bytes()
            .await
            .map_err(|_| "Could not read the complete repository file.".to_string())?;
        if metadata["size"].as_u64() != Some(bytes.len() as u64) {
            return Err("The file response size did not match its metadata.".into());
        }
        let content =
            std::str::from_utf8(&bytes).map_err(|_| "The file is not UTF-8 text.".to_string())?;
        Ok(
            json!({"path": path, "content": content, "bytes": bytes.len(), "blob_sha": blob,
            "url": format!("https://github.com/{}/{}/blob/{}/{}", self.configuration.owner, self.configuration.repo, sha, path.split('/').map(|part| urlencoding::encode(part).into_owned()).collect::<Vec<_>>().join("/"))}),
        )
    }
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_path(path: &str, root: Option<&str>) -> Result<(), String> {
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.contains(['\\', '\0'])
    {
        return Err("Use an exact repository-relative file path without traversal.".into());
    }
    if let Some(root) = root
        .map(|root| root.trim_matches('/'))
        .filter(|root| !root.is_empty())
        && path != root
        && !path.starts_with(&format!("{root}/"))
    {
        return Err("The file is outside the connected source path.".into());
    }
    Ok(())
}

fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| "GitHub returned an invalid record list.".to_string())
}

fn next_page(length: usize, page: u32) -> Result<Option<u32>, String> {
    if length < PAGE_SIZE {
        Ok(None)
    } else {
        page.checked_add(1)
            .map(Some)
            .ok_or_else(|| "GitHub pagination overflowed.".into())
    }
}

fn project(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys {
        if let Some(value) = value.get(key) {
            result.insert((*key).into(), value.clone());
        }
    }
    Value::Object(result)
}

fn latest(rows: Vec<Value>, key: &str) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|row| seen.insert(row[key].to_string()))
        .collect()
}

fn assessment(conclusions: &[&str]) -> &'static str {
    if conclusions.is_empty() {
        return "unknown";
    }
    if conclusions.iter().any(|value| {
        matches!(
            *value,
            "failure"
                | "error"
                | "cancelled"
                | "timed_out"
                | "action_required"
                | "startup_failure"
                | "stale"
        )
    }) {
        return "failure";
    }
    if conclusions.iter().any(|value| {
        matches!(
            *value,
            "pending" | "queued" | "in_progress" | "waiting" | "requested"
        )
    }) {
        return "pending";
    }
    if conclusions.iter().all(|value| *value == "success") {
        "success"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BLOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn github(server: &MockServer) -> Github {
        let mut github = Github::new(Configuration {
            owner: "owner".into(),
            repo: "repository".into(),
            branch: Some("main".into()),
            path: None,
            token: Some("test-secret".into()),
        })
        .unwrap();
        github.origin = Url::parse(&format!("{}/", server.uri())).unwrap();
        github
    }

    async fn mock(server: &MockServer, endpoint: &str, response: Value) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/owner/repository/{endpoint}")))
            .and(header("Authorization", "Bearer test-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(server)
            .await;
    }

    #[test]
    fn absent_neutral_unknown_and_pending_results_are_not_green() {
        for conclusions in [
            vec![],
            vec!["neutral"],
            vec!["skipped"],
            vec!["unknown"],
            vec!["success", "unexpected"],
        ] {
            assert_eq!(assessment(&conclusions), "unknown");
        }
        assert_eq!(assessment(&["success"]), "success");
        assert_eq!(assessment(&["success", "pending"]), "pending");
        assert_eq!(assessment(&["pending", "failure"]), "failure");
        assert_eq!(assessment(&["cancelled"]), "failure");
    }

    #[test]
    fn source_repository_and_paths_cannot_escape_scope() {
        for value in ["", "..", ".", "owner/repository", "//evil.example", "%2f"] {
            assert!(!segment(value));
        }
        for value in [
            "../secret",
            "/secret",
            "docs/../secret",
            "docs//file",
            "docs\\file",
        ] {
            assert!(validate_path(value, None).is_err());
        }
        assert!(validate_path("docs-other/file", Some("docs")).is_err());
        assert!(validate_path("docs/file", Some("docs")).is_ok());
        assert!(validate_path("docs/file", Some("/docs/")).is_ok());
    }

    #[tokio::test]
    async fn resolves_ref_once_and_observes_exact_commit() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        Mock::given(path("/repos/owner/repository/actions/runs")).and(query_param("head_sha", COMMIT))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"workflow_runs": [{"workflow_id": 1,"head_sha": COMMIT,"status": "completed","conclusion":"success"}], "total_count":1}))).expect(1).mount(&server).await;
        mock(&server, &format!("commits/{COMMIT}/check-runs"), json!({"check_runs": [{"head_sha": COMMIT,"status":"completed","conclusion":"success"}]})).await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        let result = github(&server)
            .observe(
                Operation::Build,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: None,
                    state: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["sha"], COMMIT);
        assert_eq!(result["ref"], "main");
        assert_eq!(result["state"], "success");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path().ends_with("/commits/main"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn check_pagination_cannot_hide_a_failure() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [], "total_count":0}),
        )
        .await;
        let checks: Vec<Value> = (0..100)
            .map(
                |id| json!({"id":id,"head_sha":COMMIT,"status":"completed","conclusion":"success"}),
            )
            .collect();
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"check_runs":checks})))
        .mount(&server)
        .await;
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"check_runs":[{"head_sha":COMMIT,"status":"completed","conclusion":"failure"}]}),
        ))
        .mount(&server)
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        let result = github(&server).build(COMMIT).await.unwrap();
        assert_eq!(result["state"], "failure");
        assert_eq!(result["checks"].as_array().unwrap().len(), 101);
    }

    #[tokio::test]
    async fn mismatched_sha_and_provider_limit_fail_closed() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [],"total_count":1001}),
        )
        .await;
        assert!(
            github(&server)
                .build(COMMIT)
                .await
                .unwrap_err()
                .contains("1,000")
        );
        server.reset().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [],"total_count":0}),
        )
        .await;
        mock(
            &server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs":[{"head_sha":BLOB,"status":"completed","conclusion":"success"}]}),
        )
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        assert!(
            github(&server)
                .build(COMMIT)
                .await
                .unwrap_err()
                .contains("different")
        );
    }

    #[tokio::test]
    async fn empty_build_is_unknown_and_latest_status_replaces_old_failure() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs":[],"total_count":0}),
        )
        .await;
        mock(
            &server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs":[]}),
        )
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        assert_eq!(
            github(&server).build(COMMIT).await.unwrap()["state"],
            "unknown"
        );
        assert_eq!(
            latest(
                vec![
                    json!({"context":"test","state":"success"}),
                    json!({"context":"test","state":"failure"})
                ],
                "context"
            )
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn provider_errors_never_echo_secret_or_follow_redirects() {
        let server = MockServer::start().await;
        let external = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", external.uri())
                    .set_body_string("test-secret"),
            )
            .mount(&server)
            .await;
        let error = github(&server).get(&[], &[]).await.unwrap_err();
        assert!(error.contains("302"));
        assert!(!error.contains("test-secret"));
        assert!(external.received_requests().await.unwrap().is_empty());
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("test-secret"))
            .mount(&server)
            .await;
        assert!(
            !github(&server)
                .get(&[], &[])
                .await
                .unwrap_err()
                .contains("test-secret")
        );
    }

    #[tokio::test]
    async fn file_is_complete_and_symlinks_are_rejected() {
        let server = MockServer::start().await;
        let content = "Long document 🦀\n".repeat(2000);
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":content.len()}]})).await;
        Mock::given(path(format!("/repos/owner/repository/git/blobs/{BLOB}")))
            .and(header("Accept", "application/vnd.github.raw+json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(content.clone()))
            .mount(&server)
            .await;
        let result = github(&server).file(COMMIT, "README.md").await.unwrap();
        assert_eq!(result["content"], content);
        assert_eq!(result["blob_sha"], BLOB);
        server.reset().await;
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"120000","sha":BLOB,"size":9}]})).await;
        assert!(
            github(&server)
                .file(COMMIT, "README.md")
                .await
                .unwrap_err()
                .contains("symlinks")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deployments_include_latest_provider_status() {
        let server = MockServer::start().await;
        mock(
            &server,
            "deployments",
            json!([{"id":7,"sha":COMMIT,"environment":"production"}]),
        )
        .await;
        Mock::given(path("/repos/owner/repository/deployments/7/statuses"))
            .and(query_param("per_page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!([{"state":"pending","created_at":"2026-09-05T00:00:00Z"}]),
                ),
            )
            .mount(&server)
            .await;
        let result = github(&server).deployments(COMMIT, 1).await.unwrap();
        assert_eq!(result["deployments"][0]["status"]["state"], "pending");
        assert_eq!(result["next_page"], Value::Null);
    }

    #[tokio::test]
    async fn issues_keep_full_body_exclude_pulls_and_report_provider_page() {
        let server = MockServer::start().await;
        let mut issues = vec![json!({"number":1,"pull_request":{}}); 99];
        let body = "full issue ".repeat(2000);
        issues.push(json!({"number":100,"title":"Bug","body":body}));
        mock(&server, "issues", json!(issues)).await;
        let result = github(&server)
            .observe(
                Operation::Issues,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: Some(1),
                    state: Some("all".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(result["issues"].as_array().unwrap().len(), 1);
        assert_eq!(result["issues"][0]["body"], body);
        assert_eq!(result["next_page"], 2);
    }
    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL via TEST_DATABASE_URL"]
    async fn source_access_requires_active_membership_and_matching_workspace() {
        use crate::db::{organizations, users, workspaces};
        let pool = sqlx::PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL required"),
        )
        .await
        .unwrap();
        let identifier = Uuid::new_v4();
        let organization = organizations::create_organization(
            &pool,
            "Integration test",
            &format!("integration-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Allowed",
            &format!("allowed-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let foreign = workspaces::create_workspace(
            &pool,
            organization.id,
            "Foreign",
            &format!("foreign-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let user = users::create_user(
            &pool,
            &format!("integration-{identifier}@example.test"),
            "hash",
            None,
            false,
        )
        .await
        .unwrap();
        let source = sources::create_source(
            &pool,
            workspace.id,
            "Invalid configured source",
            "github",
            json!({}),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let other = sources::create_source(
            &pool,
            foreign.id,
            "Foreign source",
            "github",
            json!({}),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let tool = Integration {
            operation: Operation::Build,
            scope: WorkspaceScope {
                state: crate::state::AppState::new(crate::state::test_config(), pool.clone(), None),
                workspace_id: workspace.id,
                chat_id: Uuid::new_v4(),
                user_id: user.id,
            },
        };
        let denied = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(denied.contains("cannot read"));
        workspace_members::add_member(
            &pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Viewer,
            None,
        )
        .await
        .unwrap();
        let allowed = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(allowed.contains("configuration is invalid"));
        let foreign_result = tool.run(json!({"source_id":other.id})).await.unwrap_err();
        assert!(foreign_result.contains("not found"));
        workspace_members::remove_member(&pool, workspace.id, user.id)
            .await
            .unwrap();
        let revoked = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(revoked.contains("cannot read"));
        organizations::delete_organization(&pool, organization.id)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
