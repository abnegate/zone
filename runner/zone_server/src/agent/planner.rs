//! The interview an auto project comes out of, and the two calls that end it.
//!
//! A planner chat is an ordinary agentic chat with two tools more. The model
//! asks its questions through `ask_user`, the way any chat does, and when it
//! has enough it calls `finalize_project` once with everything it learned:
//! the brief, and every task the project needs. `create_repository` sits
//! beside it for a project that has no repository yet, and is confirmed by
//! the person because it reaches outside the workspace.
//!
//! The blueprint is validated here, before anything is written: a plan that
//! would leave the project without continuous integration, without tests or
//! without a way to ship is refused with the reason, and the model plans again.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};
use zone_vcs::git::GitService;
use zone_vcs::pull_request::{PrError, PrService};

use super::WorkspaceScope;
use crate::db::auto_projects::{self, Kind, PlannedRepository, PlannedTask, ProjectPlan};
use crate::db::{sources, workspace_members};

pub const CREATE_REPOSITORY: &str = "create_repository";
pub const FINALIZE_PROJECT: &str = "finalize_project";

/// Tasks one project may be planned with.
pub const MAX_TASKS: usize = 100;
const MAX_NAME_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 8_000;

const CREATE_REPOSITORY_DESCRIPTION: &str = "Create a new, empty GitHub repository for the project, with an initial commit, using the \
     credential of a connected GitHub source in this workspace. Returns the repository URL to \
     pass to finalize_project. Only when the person chose to start a new repository rather than \
     use an existing one; the call is confirmed by them before it runs.";

const FINALIZE_PROJECT_DESCRIPTION: &str = "Create the project and every task needed to complete it, and start executing them. Call \
     it exactly once, after the interview has settled every decision and the person has \
     confirmed the plan. Tasks are ordered and depend on earlier tasks by index; the first \
     scaffolds the repository when it is new, a continuous-integration task follows and every \
     later task depends on it, every feature ships its tests, and a deployment task adds the \
     deploy workflow for the chosen target. The plan is refused with the reason when it lacks \
     any of these.";

/// What the interview produced.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Blueprint {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
    /// Every decision the interview settled: platforms, stack, design,
    /// testing, CI, deployment. Free-form, rendered into every run's prompt.
    #[serde(default)]
    pub spec: Value,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RepositoryRef {
    /// A connected GitHub source whose repository and credential the project uses.
    #[serde(default)]
    pub source_id: Option<Uuid>,
    /// A repository URL: an existing one, or one create_repository returned.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskSpec {
    pub kind: Kind,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<usize>,
    #[serde(default)]
    pub priority: Option<i32>,
}

impl Blueprint {
    /// Read a blueprint from the tool's arguments, refusing one that fails validation.
    pub fn parse(arguments: &Value) -> Result<Self, String> {
        let blueprint: Self = serde_json::from_value(arguments.clone()).map_err(|error| {
            format!(
                "`{FINALIZE_PROJECT}` arguments did not match the schema: {error}. It needs `name`, \
                 `spec`, an optional `repository`, and `tasks`, each with `kind`, `title`, \
                 `description`, `acceptance_criteria`, `depends_on` (indices) and `priority`."
            )
        })?;
        blueprint.validate()?;
        Ok(blueprint)
    }

    /// Whether the spec says the project is not deployed anywhere.
    fn no_deployment(&self) -> bool {
        self.spec
            .get("deployment")
            .and_then(|deployment| {
                deployment
                    .get("target")
                    .and_then(Value::as_str)
                    .or_else(|| deployment.as_str())
            })
            .is_some_and(|target| target.trim().eq_ignore_ascii_case("none"))
    }

    /// Refuse a plan that could not complete the project, naming what is missing.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(format!(
                "`name` must be present and at most {MAX_NAME_CHARS} characters."
            ));
        }
        if self.description.chars().count() > MAX_TEXT_CHARS {
            return Err(format!(
                "`description` is over {MAX_TEXT_CHARS} characters."
            ));
        }
        // A source alone means the source's own repository; a URL alone means
        // a repository the runs reach without a credential; both together is
        // what create_repository hands back: the new repository's URL and the
        // source whose token pushes to it.
        if let Some(repository) = &self.repository {
            if repository.source_id.is_none() && repository.url.is_none() {
                return Err("`repository` names neither a source nor a URL.".to_string());
            }
            if let Some(url) = &repository.url {
                GitService::repository_url(url)
                    .map_err(|error| format!("`repository.url` is not a repository: {error}"))?;
            }
        }
        if self.tasks.is_empty() {
            return Err("`tasks` is empty; a project needs at least one task.".to_string());
        }
        if self.tasks.len() > MAX_TASKS {
            return Err(format!(
                "`tasks` holds {} tasks; a project is planned with at most {MAX_TASKS}. Merge \
                 the smallest.",
                self.tasks.len()
            ));
        }
        let mut ci_covered: Vec<bool> = Vec::with_capacity(self.tasks.len());
        let mut scaffolds = 0usize;
        let mut ci = 0usize;
        let mut tests = 0usize;
        let mut deployments = 0usize;
        for (index, task) in self.tasks.iter().enumerate() {
            let position = index + 1;
            if task.title.trim().is_empty() {
                return Err(format!("Task {position} has no title."));
            }
            if task.description.trim().is_empty() {
                return Err(format!("Task {position} has no description."));
            }
            if task.description.chars().count() > MAX_TEXT_CHARS {
                return Err(format!(
                    "Task {position}'s description is over {MAX_TEXT_CHARS} characters."
                ));
            }
            if task
                .priority
                .is_some_and(|priority| !(1..=5).contains(&priority))
            {
                return Err(format!(
                    "Task {position}'s priority must be between 1 and 5."
                ));
            }
            for dependency in &task.depends_on {
                if *dependency >= index {
                    return Err(format!(
                        "Task {position} depends on task {}, which is not before it; a task may \
                         only depend on tasks listed earlier.",
                        dependency + 1
                    ));
                }
            }
            match task.kind {
                Kind::Scaffold => {
                    scaffolds += 1;
                    if index != 0 {
                        return Err(format!(
                            "Task {position} is the scaffold; the scaffold must be the first task."
                        ));
                    }
                }
                Kind::Ci => {
                    ci += 1;
                    if task
                        .depends_on
                        .iter()
                        .any(|dependency| self.tasks[*dependency].kind != Kind::Scaffold)
                    {
                        return Err(format!(
                            "Task {position} adds continuous integration and may depend only on the \
                             scaffold, so it runs before the features do."
                        ));
                    }
                }
                Kind::Tests => tests += 1,
                Kind::Deployment => deployments += 1,
                Kind::Feature | Kind::Docs | Kind::Fix => {}
            }
            let covered = task.kind == Kind::Ci
                || task
                    .depends_on
                    .iter()
                    .any(|dependency| ci_covered[*dependency]);
            ci_covered.push(covered);
        }
        if scaffolds > 1 {
            return Err("More than one task is a scaffold; there can be one.".to_string());
        }
        if ci == 0 {
            return Err(
                "No task adds continuous integration (kind `ci`). Every project needs a workflow \
                 that runs its tests, lint and type checks on every pull request, before any \
                 feature task."
                    .to_string(),
            );
        }
        if tests == 0 {
            return Err(
                "No task is a `tests` task. Plan one that establishes the test suite the features \
                 build on."
                    .to_string(),
            );
        }
        if deployments == 0 && !self.no_deployment() {
            return Err(
                "No task is a `deployment` task and the spec does not say `deployment.target` is \
                 `none`. Plan the deploy workflow for the chosen target, or record that the \
                 project is not deployed."
                    .to_string(),
            );
        }
        for (index, task) in self.tasks.iter().enumerate() {
            if !matches!(task.kind, Kind::Scaffold | Kind::Ci) && !ci_covered[index] {
                return Err(format!(
                    "Task {} does not depend, directly or through earlier tasks, on the \
                     continuous-integration task, so its pull request would have no checks. Add \
                     the dependency.",
                    index + 1
                ));
            }
        }
        Ok(())
    }

    /// The plan in the shape the database creates it from.
    pub fn planned_tasks(&self) -> Vec<PlannedTask> {
        self.tasks
            .iter()
            .map(|task| PlannedTask {
                kind: task.kind,
                title: task.title.trim().to_string(),
                description: task.description.trim().to_string(),
                acceptance_criteria: task
                    .acceptance_criteria
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string),
                depends_on: task.depends_on.clone(),
                priority: task.priority,
            })
            .collect()
    }
}

/// Add the planner's two tools to a chat's registry, bound to its workspace scope.
pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    registry.register(Arc::new(CreateRepositoryTool(scope.clone())));
    registry.register(Arc::new(FinalizeProjectTool(scope.clone())));
}

/// The GitHub source's token and its configured owner and repository.
struct SourceCredential {
    token: String,
    owner: Option<String>,
    repo: Option<String>,
}

/// The token and repository a connected GitHub source holds, decrypted for one call.
async fn source_credential(
    scope: &WorkspaceScope,
    source_id: Uuid,
) -> Result<SourceCredential, String> {
    let source = sources::get_source(scope.state.db(), source_id, scope.workspace_id)
        .await
        .map_err(|_| "Could not read the source.".to_string())?
        .filter(|source| source.is_active.unwrap_or(true))
        .ok_or("Source not found in this workspace or inactive.")?;
    if source.source_type != "github" {
        return Err("The source is not a GitHub source.".to_string());
    }
    let encrypted = source
        .credentials_encrypted
        .ok_or("The GitHub source has no credential stored; add a token to it first.")?;
    let token = crate::crypto::decrypt(scope.state.encryption_key(), &encrypted)
        .map_err(|_| "The source credentials could not be decrypted.".to_string())?;
    Ok(SourceCredential {
        token,
        owner: source.config["owner"].as_str().map(str::to_string),
        repo: source.config["repo"].as_str().map(str::to_string),
    })
}

/// Refuse unless the chat's user may still write to the workspace.
async fn authorized_writer(scope: &WorkspaceScope) -> Result<(), String> {
    match workspace_members::can_write(scope.state.db(), scope.workspace_id, scope.user_id).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("You cannot write to this workspace.".to_string()),
        Err(_) => Err("Workspace authorization failed.".to_string()),
    }
}

struct CreateRepositoryTool(WorkspaceScope);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRepositoryArguments {
    source_id: Uuid,
    name: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_private")]
    private: bool,
}

/// A new repository is private unless the model says otherwise.
fn default_private() -> bool {
    true
}

#[async_trait]
impl Tool for CreateRepositoryTool {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &str {
        CREATE_REPOSITORY
    }

    /// What the model is told the tool does.
    fn description(&self) -> &str {
        CREATE_REPOSITORY_DESCRIPTION
    }

    /// The JSON schema of the tool's arguments.
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": {"type": "string", "format": "uuid", "description": "A connected GitHub source in this workspace whose credential creates the repository."},
                "name": {"type": "string", "minLength": 1, "description": "Repository name, e.g. acme-shop."},
                "owner": {"type": "string", "description": "Organisation to create it under; omit for the credential's own user, or when the source's owner should be used."},
                "description": {"type": "string"},
                "private": {"type": "boolean", "default": true}
            },
            "required": ["source_id", "name"],
            "additionalProperties": false
        })
    }

    /// An outward call: the person confirms it on an approval card first.
    fn tier(&self) -> Tier {
        Tier::Outward
    }

    /// The one line the approval card shows: owner, name and visibility.
    fn preview(&self, params: &Value) -> Option<String> {
        let name = params["name"].as_str()?;
        let owner = params["owner"].as_str().unwrap_or("the source's owner");
        let visibility = if params["private"].as_bool().unwrap_or(true) {
            "private"
        } else {
            "public"
        };
        Some(format!(
            "Create the {visibility} GitHub repository {owner}/{name} with an initial commit"
        ))
    }

    /// Create the repository through the source's credential and report its URL.
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let arguments: CreateRepositoryArguments = match serde_json::from_value(params) {
            Ok(arguments) => arguments,
            Err(error) => return Ok(ToolResult::error(format!("Invalid arguments: {error}"))),
        };
        if let Err(refusal) = authorized_writer(&self.0).await {
            return Ok(ToolResult::error(refusal));
        }
        let credential = match source_credential(&self.0, arguments.source_id).await {
            Ok(credential) => credential,
            Err(refusal) => return Ok(ToolResult::error(refusal)),
        };
        let owner = arguments
            .owner
            .clone()
            .map(|owner| owner.trim().to_string())
            .filter(|owner| !owner.is_empty())
            .or(credential.owner.clone());
        let service = PrService::configured(self.0.state.config().github_api_url.clone());
        match service
            .create_repository(
                &credential.token,
                owner.as_deref(),
                arguments.name.trim(),
                arguments.description.as_deref().unwrap_or_default(),
                arguments.private,
            )
            .await
        {
            Ok(created) => Ok(ToolResult::success(
                json!({
                    "url": created.html_url,
                    "clone_url": created.clone_url,
                    "default_branch": created.default_branch,
                    "source_id": arguments.source_id,
                    "message": "Repository created with an initial commit. Pass its url and this source_id to finalize_project as the repository."
                })
                .to_string(),
            )),
            Err(PrError::RepositoryExists(name)) => Ok(ToolResult::error(format!(
                "A repository named {name} already exists under that owner; pick another name or use the existing repository's URL."
            ))),
            Err(PrError::AuthFailed) => Ok(ToolResult::error(
                "GitHub refused the source's credential for creating a repository; it needs repository-creation permission.",
            )),
            Err(error) => Ok(ToolResult::error(format!("GitHub refused: {error}"))),
        }
    }
}

struct FinalizeProjectTool(WorkspaceScope);

#[async_trait]
impl Tool for FinalizeProjectTool {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &str {
        FINALIZE_PROJECT
    }

    /// What the model is told the tool does.
    fn description(&self) -> &str {
        FINALIZE_PROJECT_DESCRIPTION
    }

    /// The JSON schema of the tool's arguments.
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "description": {"type": "string", "description": "One paragraph: what the project is."},
                "repository": {
                    "type": "object",
                    "properties": {
                        "source_id": {"type": "string", "format": "uuid", "description": "A connected GitHub source; its repository and credential are used."},
                        "url": {"type": "string", "description": "The repository URL, existing or just created. With source_id, the source supplies the credential."}
                    },
                    "additionalProperties": false
                },
                "spec": {
                    "type": "object",
                    "description": "Every decision the interview settled, e.g. platforms, frameworks, languages, theme {archetype, primary, secondary, typography, animation_style, accessibility}, features, integrations, data, auth, testing {levels, frameworks}, ci {jobs}, deployment {target, environments, trigger, secrets}, definition_of_done. deployment.target may be \"none\"."
                },
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_TASKS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": {"type": "string", "enum": ["scaffold", "ci", "tests", "feature", "deployment", "docs"]},
                            "title": {"type": "string", "minLength": 1},
                            "description": {"type": "string", "minLength": 1, "description": "What to build, precisely enough that a run needs no question answered."},
                            "acceptance_criteria": {"type": "string", "description": "How the run knows it is done."},
                            "depends_on": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "Zero-based indices of earlier tasks this one needs merged first."},
                            "priority": {"type": "integer", "minimum": 1, "maximum": 5}
                        },
                        "required": ["kind", "title", "description"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["name", "spec", "tasks"],
            "additionalProperties": false
        })
    }

    /// A workspace write: recorded as a receipt, no approval card.
    fn tier(&self) -> Tier {
        Tier::Write
    }

    /// The one line the receipt shows: the project and how many tasks it creates.
    fn preview(&self, params: &Value) -> Option<String> {
        let name = params["name"].as_str()?;
        let tasks = params["tasks"].as_array().map_or(0, Vec::len);
        Some(format!(
            "Create project «{name}» with {tasks} tasks and start it"
        ))
    }

    /// Validate the blueprint, create the project and its tasks in one transaction, and wake the driver.
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let blueprint = match Blueprint::parse(&params) {
            Ok(blueprint) => blueprint,
            Err(refusal) => return Ok(ToolResult::error(refusal)),
        };
        if let Err(refusal) = authorized_writer(&self.0).await {
            return Ok(ToolResult::error(refusal));
        }
        let (url, token) = match &blueprint.repository {
            Some(RepositoryRef {
                source_id: Some(source_id),
                url,
            }) => {
                let credential = match source_credential(&self.0, *source_id).await {
                    Ok(credential) => credential,
                    Err(refusal) => return Ok(ToolResult::error(refusal)),
                };
                let url = match url.clone() {
                    Some(url) => url,
                    None => match (credential.owner.as_deref(), credential.repo.as_deref()) {
                        (Some(owner), Some(repo)) => format!("https://github.com/{owner}/{repo}"),
                        _ => {
                            return Ok(ToolResult::error(
                                "The source names no repository; give `repository.url` as well.",
                            ));
                        }
                    },
                };
                // Stored the way the source keeps it: encrypted at rest, opened
                // by the checkout and pull request code that uses it.
                let sealed = match crate::crypto::encrypt(
                    self.0.state.encryption_key(),
                    &credential.token,
                ) {
                    Ok(sealed) => sealed,
                    Err(_) => {
                        return Ok(ToolResult::error(
                            "The repository token could not be encrypted for storage.",
                        ));
                    }
                };
                (Some(url), Some(sealed))
            }
            Some(RepositoryRef {
                source_id: None,
                url: Some(url),
            }) => (Some(url.clone()), None),
            _ => (None, None),
        };
        let planned = blueprint.planned_tasks();
        let plan = ProjectPlan {
            name: blueprint.name.trim(),
            description: Some(blueprint.description.trim()).filter(|text| !text.is_empty()),
            repository: url.as_deref().map(|url| PlannedRepository {
                url,
                token: token.as_deref(),
            }),
            brief: &blueprint.spec,
            tasks: &planned,
        };
        match auto_projects::finalize(
            self.0.state.db(),
            self.0.workspace_id,
            self.0.user_id,
            self.0.chat_id,
            &plan,
        )
        .await
        {
            Ok(finalized) => {
                crate::workers::auto_project::poke(finalized.project_id);
                Ok(ToolResult::success(
                    json!({
                        "id": finalized.project_id,
                        "project_id": finalized.project_id,
                        "title": blueprint.name.trim(),
                        "task_ids": finalized.task_ids,
                        "updates_chat_id": finalized.updates_chat_id,
                        "repository_url": url,
                        "message": "Project created and automation started. Progress and every merge are reported in the project's updates chat and on the Projects page."
                    })
                    .to_string(),
                ))
            }
            Err(sqlx::Error::Protocol(message)) => Ok(ToolResult::error(message)),
            Err(error) => {
                tracing::warn!(%error, "finalize_project failed");
                Ok(ToolResult::error(
                    "The project could not be created; nothing was written.",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(kind: &str, title: &str, depends_on: &[usize]) -> Value {
        json!({"kind": kind, "title": title, "description": format!("Do {title}"), "depends_on": depends_on})
    }

    fn plan(tasks: Vec<Value>, spec: Value) -> Value {
        json!({"name": "Shop", "description": "A shop", "spec": spec, "tasks": tasks})
    }

    fn sound() -> Value {
        plan(
            vec![
                task("scaffold", "Scaffold", &[]),
                task("ci", "CI", &[0]),
                task("tests", "Test suite", &[1]),
                task("feature", "Cart", &[2]),
                task("deployment", "Deploy", &[3]),
                task("docs", "README", &[4]),
            ],
            json!({"deployment": {"target": "vercel"}}),
        )
    }

    #[test]
    fn a_sound_plan_is_accepted_and_its_tasks_keep_their_order() {
        let blueprint = Blueprint::parse(&sound()).unwrap();
        assert_eq!(blueprint.tasks.len(), 6);
        let planned = blueprint.planned_tasks();
        assert_eq!(planned[1].kind, Kind::Ci);
        assert_eq!(planned[3].depends_on, vec![2]);
    }

    #[test]
    fn a_plan_missing_ci_tests_or_deployment_is_refused_with_the_reason() {
        let no_ci = plan(
            vec![
                task("scaffold", "S", &[]),
                task("tests", "T", &[0]),
                task("deployment", "D", &[1]),
            ],
            json!({}),
        );
        assert!(
            Blueprint::parse(&no_ci)
                .unwrap_err()
                .contains("continuous integration")
        );
        let no_tests = plan(
            vec![
                task("scaffold", "S", &[]),
                task("ci", "C", &[0]),
                task("deployment", "D", &[1]),
            ],
            json!({}),
        );
        assert!(Blueprint::parse(&no_tests).unwrap_err().contains("`tests`"));
        let no_deploy = plan(
            vec![
                task("scaffold", "S", &[]),
                task("ci", "C", &[0]),
                task("tests", "T", &[1]),
            ],
            json!({}),
        );
        assert!(
            Blueprint::parse(&no_deploy)
                .unwrap_err()
                .contains("`deployment`")
        );
        let not_deployed = plan(
            vec![
                task("scaffold", "S", &[]),
                task("ci", "C", &[0]),
                task("tests", "T", &[1]),
            ],
            json!({"deployment": {"target": "none"}}),
        );
        assert!(
            Blueprint::parse(&not_deployed).is_ok(),
            "an explicit none is a decision"
        );
    }

    #[test]
    fn ordering_rules_are_enforced() {
        let forward = plan(
            vec![
                task("scaffold", "S", &[1]),
                task("ci", "C", &[0]),
                task("tests", "T", &[1]),
            ],
            json!({"deployment": {"target": "none"}}),
        );
        assert!(
            Blueprint::parse(&forward)
                .unwrap_err()
                .contains("not before it")
        );
        let late_scaffold = plan(
            vec![
                task("ci", "C", &[]),
                task("scaffold", "S", &[]),
                task("tests", "T", &[0]),
            ],
            json!({"deployment": {"target": "none"}}),
        );
        assert!(
            Blueprint::parse(&late_scaffold)
                .unwrap_err()
                .contains("first task")
        );
        let uncovered = plan(
            vec![
                task("scaffold", "S", &[]),
                task("ci", "C", &[0]),
                task("tests", "T", &[0]),
            ],
            json!({"deployment": {"target": "none"}}),
        );
        assert!(
            Blueprint::parse(&uncovered)
                .unwrap_err()
                .contains("no checks")
        );
        let ci_after_feature = plan(
            vec![
                task("scaffold", "S", &[]),
                task("feature", "F", &[0]),
                task("ci", "C", &[1]),
                task("tests", "T", &[2]),
            ],
            json!({"deployment": {"target": "none"}}),
        );
        let error = Blueprint::parse(&ci_after_feature).unwrap_err();
        assert!(
            error.contains("only on the scaffold") || error.contains("no checks"),
            "{error}"
        );
    }

    #[test]
    fn a_repository_names_a_source_a_url_or_both_and_a_bad_url_is_refused() {
        // A created repository is named by its URL and the source whose token
        // pushes to it: exactly what create_repository tells the model to pass.
        let mut both = sound();
        both["repository"] =
            json!({"source_id": Uuid::new_v4(), "url": "https://github.com/acme/shop"});
        assert!(Blueprint::parse(&both).is_ok());
        let mut neither = sound();
        neither["repository"] = json!({});
        assert!(
            Blueprint::parse(&neither)
                .unwrap_err()
                .contains("neither a source nor a URL")
        );
        let mut bad = sound();
        bad["repository"] = json!({"url": "ftp://nowhere"});
        assert!(
            Blueprint::parse(&bad)
                .unwrap_err()
                .contains("not a repository")
        );
        let mut good = sound();
        good["repository"] = json!({"url": "https://github.com/acme/shop"});
        assert!(Blueprint::parse(&good).is_ok());
    }
}
