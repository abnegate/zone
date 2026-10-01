//! A review of one head by one model, over the diff and the files it may read.
//!
//! Modelled on the conflict repair agent: a bounded loop over a hand-built
//! registry of tools, with no shell, no disk and no way to change anything.
//! The reply ends with a verdict, and a reply that does not is asked once more
//! and then counted as a failed round.

pub mod bots;
pub mod model;
pub mod outage;
pub mod prompt;
#[cfg(test)]
pub(crate) mod testing;
pub mod tools;
pub mod verdict;

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use serde_json::Value;
use thiserror::Error;
use zone_core::llm::provider::{UNFUNDED, UNFUNDED_CONTEXT};
use zone_core::llm::{LlmBackend, LlmClient, LlmError, Message, ToolDefinition};
use zone_core::tools::{ToolContext, ToolResult};
use zone_vcs::pull_request::{ChangedFile, PrService, PullRequestDetail, PullRequestReference};

use crate::db::auto_projects::{Finding, ReviewRow};
use crate::db::tasks::TaskRow;
use crate::services::backend;
use crate::services::endpoint::Endpoint;

pub use verdict::{Outcome, Verdict};

/// Turns a review may take: reads, then the verdict.
const MAX_TURNS: usize = 12;
/// How long one review may run.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(600);
/// Tokens the verdict turn may spend.
const REVIEW_TOKENS: u32 = 4_096;
/// A review is a judgement, not a draft.
const REVIEW_TEMPERATURE: f32 = 0.0;
/// How zone_core begins a coding agent's refusal of a model or a context the
/// signed-in account cannot spend usage credits on, which every tick meets
/// again.
const UNFUNDED_MARKERS: [&str; 2] = [UNFUNDED, UNFUNDED_CONTEXT];

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("the reviewer's reply carried no readable verdict: {0}")]
    Unparseable(String),
    #[error("the reviewer model failed: {0}")]
    Model(String),
    /// Nothing answered for the model: the endpoint is down, restarting or
    /// overloaded, and the same model may answer on a later tick.
    #[error("the reviewer model could not be reached: {0}")]
    Unreachable(String),
    #[error("the reviewer model {reviewer} cannot run: {reason}")]
    Unfunded { reviewer: String, reason: String },
    #[error("the review did not finish within {} seconds", REVIEW_TIMEOUT.as_secs())]
    TimedOut,
}

impl ReviewError {
    /// `error`, what failed the `reviewer` model the review ran on `backend`.
    fn model(backend: &LlmBackend, reviewer: &str, error: LlmError) -> Self {
        if unreachable(&error) {
            return Self::Unreachable(error.to_string());
        }
        let message = error.to_string();
        let words = backend::own_words(&message);
        let unfunded = matches!(backend, LlmBackend::Cli { .. })
            && UNFUNDED_MARKERS.iter().any(|marker| words.contains(marker));
        if unfunded {
            return Self::Unfunded {
                reviewer: reviewer.to_string(),
                reason: words.to_string(),
            };
        }
        Self::Model(message)
    }

    /// This failure, without the key `endpoint` was sent: it reaches review
    /// rows, pause reasons and logs.
    fn scrubbed(self, endpoint: &Endpoint) -> Self {
        match self {
            Self::Unparseable(message) => Self::Unparseable(endpoint.scrub(&message)),
            Self::Model(message) => Self::Model(endpoint.scrub(&message)),
            Self::Unreachable(message) => Self::Unreachable(endpoint.scrub(&message)),
            Self::Unfunded { reviewer, reason } => Self::Unfunded {
                reviewer,
                reason: endpoint.scrub(&reason),
            },
            Self::TimedOut => Self::TimedOut,
        }
    }

    /// Why the task has to wait for a person, when no later tick would get
    /// past this failure.
    pub fn stalled(&self) -> Option<String> {
        matches!(self, Self::Unfunded { .. }).then(|| self.to_string())
    }
}

/// Whether `error` came from the way to the model rather than the model: no
/// connection, no answer in time, or a status that says the server in front
/// of it is overloaded or restarting. A proxy's 5xx that carries the model's
/// refusal of tools is the model's.
fn unreachable(error: &LlmError) -> bool {
    match error {
        LlmError::Http(error) => !error.is_decode(),
        LlmError::Api { status, .. } => {
            let status = StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status.is_server_error()
                || status == StatusCode::TOO_MANY_REQUESTS
                || status == StatusCode::REQUEST_TIMEOUT)
                && !error.unsupported_tools()
        }
        LlmError::Json(_)
        | LlmError::Stream(_)
        | LlmError::Agent(_)
        | LlmError::InvalidConfig(_) => false,
    }
}

pub struct ReviewRequest<'a> {
    pub task: &'a TaskRow,
    pub brief: Option<&'a Value>,
    pub pull: &'a PullRequestDetail,
    pub diff: String,
    pub files: Vec<ChangedFile>,
    pub open: &'a [Finding],
    pub prior: &'a [ReviewRow],
    pub round: i32,
    pub reviewer: String,
    pub same_model: bool,
    pub token: String,
    pub reference: PullRequestReference,
}

/// Run one review on `endpoint` and return its verdict.
pub async fn run(
    endpoint: &Endpoint,
    backend: LlmBackend,
    pr: PrService,
    request: ReviewRequest<'_>,
) -> Result<Verdict, ReviewError> {
    let client = LlmClient::new(endpoint.llm(
        request.reviewer.clone(),
        REVIEW_TEMPERATURE,
        REVIEW_TOKENS,
        backend,
    ));
    let shared = Arc::new(tools::Shared {
        pr,
        reference: request.reference.clone(),
        head: request.pull.head_sha.clone(),
        token: request.token.clone(),
        diff: request.diff.clone(),
        files: request.files.clone(),
    });
    let registry = tools::registry(shared);
    let definitions: Option<Vec<ToolDefinition>> =
        matches!(client.config().backend, LlmBackend::Http).then(|| registry.definitions());
    let context = ToolContext::default();
    let mut messages = vec![
        Message::system(prompt::system(
            request.round,
            request.same_model,
            definitions.is_some(),
        )),
        Message::user(prompt::user(
            request.task,
            request.brief,
            request.pull,
            &request.diff,
            request.open,
            request.prior,
        )),
    ];
    let round = request.round;
    let reviewer = request.reviewer.clone();
    let session = async {
        let mut corrected = false;
        for _ in 0..MAX_TURNS {
            let response = client
                .chat_with_model(&reviewer, &messages, definitions.as_deref())
                .await
                .map_err(|error| ReviewError::model(&client.config().backend, &reviewer, error))?;
            let Some(choice) = response.choices.into_iter().next() else {
                return Err(ReviewError::Model("the model returned no choices".into()));
            };
            let calls = choice.message.tool_calls.clone().unwrap_or_default();
            let content = choice.message.content.clone().unwrap_or_default();
            messages.push(choice.message);
            if calls.is_empty() {
                match verdict::parse(&content, round) {
                    Ok(verdict) => return Ok(verdict),
                    Err(error) if !corrected => {
                        corrected = true;
                        messages.push(Message::user(format!(
                            "Your reply did not carry a valid verdict: {error}. Reply again with \
                             only the verdict between {} and {}.",
                            verdict::OPEN_TAG,
                            verdict::CLOSE_TAG
                        )));
                        continue;
                    }
                    Err(error) => return Err(ReviewError::Unparseable(error.to_string())),
                }
            }
            for call in calls {
                let arguments =
                    serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
                let outcome = registry
                    .execute(&call.function.name, arguments, &context)
                    .await
                    .unwrap_or_else(|error| ToolResult::error(error.to_string()));
                let rendered = outcome
                    .output
                    .or(outcome.error)
                    .unwrap_or_else(|| "no output".to_string());
                messages.push(Message::tool_result(call.id, rendered));
            }
        }
        Err(ReviewError::Unparseable(format!(
            "the review did not reach a verdict within {MAX_TURNS} turns"
        )))
    };
    match tokio::time::timeout(REVIEW_TIMEOUT, session).await {
        Ok(outcome) => outcome.map_err(|error| error.scrubbed(endpoint)),
        Err(_) => Err(ReviewError::TimedOut),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use uuid::Uuid;
    use zone_core::llm::AgentKind;
    use zone_vcs::pull_request::Mergeability;

    use crate::config::{Config, ModelBackend};

    const DIFF: &str = "diff --git a/src/cart.ts b/src/cart.ts\n+export const total = 0;";

    const REPLY: &str = "I read the diff.\n<zone-review>\n{\"verdict\":\"approve\",\"summary\":\"The cart is sound.\",\"findings\":[],\"addressed\":[]}\n</zone-review>";

    const REFUSAL: &str = "Fable 5.1 requires usage credits. Switch to another model to continue.";

    /// A stand-in claude that keeps its prompt in `prompt` and answers with
    /// `stream`.
    fn agent(directory: &TempDir, prompt: &Path, stream: &[Value]) -> PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let path = directory.path().join("claude");
        let mut file = std::fs::File::create(&path).expect("the fake agent");
        let lines: Vec<String> = stream.iter().map(Value::to_string).collect();
        writeln!(
            file,
            "#!/bin/sh\ncat > '{prompt}'\ncat <<'EOF'\n{stream}\nEOF",
            prompt = prompt.display(),
            stream = lines.join("\n"),
        )
        .expect("the fake agent body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the fake agent to be executable");
        path
    }

    fn reviewer(directory: &TempDir, prompt: &Path) -> PathBuf {
        agent(
            directory,
            prompt,
            &[
                json!({
                    "type": "assistant",
                    "message": {"content": [{"type": "text", "text": REPLY}]},
                }),
                json!({"type": "result", "subtype": "success", "is_error": false}),
            ],
        )
    }

    /// A stand-in claude that refuses its turn in `words`, typed `kind`.
    fn refusing(directory: &TempDir, prompt: &Path, words: &str, kind: Option<&str>) -> PathBuf {
        let mut refusal = json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": words}]},
            "error": "rate_limit",
            "is_api_error_message": true,
        });
        if let Some(kind) = kind {
            refusal["api_error"] = kind.into();
        }
        agent(
            directory,
            prompt,
            &[
                refusal,
                json!({"type": "result", "subtype": "success", "is_error": true, "result": words}),
            ],
        )
    }

    fn claude(executable: PathBuf) -> Config {
        Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Claude,
                executable: Some(executable),
            },
            ..crate::state::test_config()
        }
    }

    fn request<'a>(
        task: &'a TaskRow,
        pull: &'a PullRequestDetail,
        reviewer: &str,
    ) -> ReviewRequest<'a> {
        ReviewRequest {
            task,
            brief: None,
            pull,
            diff: DIFF.to_string(),
            files: Vec::new(),
            open: &[],
            prior: &[],
            round: 1,
            reviewer: reviewer.into(),
            same_model: false,
            token: "token".into(),
            reference: PullRequestReference {
                owner: "acme".into(),
                repository: "shop".into(),
                number: 7,
            },
        }
    }

    fn task() -> TaskRow {
        TaskRow {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            created_by: None,
            project_ids: Vec::new(),
            title: "Add a cart".into(),
            description: "Shoppers need a cart.".into(),
            acceptance_criteria: None,
            status: "completed".into(),
            priority: None,
            model_name: None,
            dependencies: None,
            is_agentic: true,
            require_plan_approval: false,
            github_repo_url: None,
            source_id: None,
            source_ids: None,
            worker_id: None,
            queued_at: None,
            started_at: None,
            completed_at: None,
            created_at: None,
            updated_at: None,
            pr_url: None,
            branch_name: None,
            pr_status: None,
            pr_created_at: None,
        }
    }

    fn pull() -> PullRequestDetail {
        PullRequestDetail {
            node_id: String::new(),
            number: 7,
            title: "(feat): add a cart".into(),
            body: Some("Adds a cart.".into()),
            state: "open".into(),
            draft: false,
            merged: false,
            merge_commit_sha: None,
            head_sha: "abc123".into(),
            head_ref: "zone/cart".into(),
            base_ref: "main".into(),
            mergeable: Mergeability::Unknown,
            mergeable_state: None,
            changed_files: 1,
            additions: 1,
            deletions: 0,
            commits: 1,
            html_url: String::new(),
        }
    }

    #[tokio::test]
    async fn a_coding_agent_reviews_the_change_from_its_inline_diff() {
        let directory = TempDir::new().expect("a temporary directory");
        let prompt = directory.path().join("prompt");
        let config = claude(reviewer(&directory, &prompt));
        let task = task();
        let pull = pull();

        let verdict = run(
            &Endpoint::instance(&config),
            backend::instance(&config),
            PrService::new(),
            request(&task, &pull, "sonnet"),
        )
        .await
        .expect("an agent that answers with a verdict has reviewed the change");

        assert_eq!(verdict.outcome, Outcome::Approve);
        assert_eq!(verdict.summary, "The cart is sound.");
        let prompt = std::fs::read_to_string(&prompt).expect("the agent was given a prompt");
        assert!(
            prompt.contains(DIFF),
            "the agent reviews from the diff inline: {prompt}"
        );
        assert!(
            !prompt.contains("read_pr_file"),
            "the agent was told of a tool it cannot call: {prompt}"
        );
    }

    /// claude refuses a reviewer the signed-in account cannot spend usage
    /// credits on the same way on every tick, so the review says so apart
    /// from a model that merely failed.
    #[tokio::test]
    async fn a_reviewer_the_signed_in_account_cannot_fund_is_unfunded() {
        let directory = TempDir::new().expect("a temporary directory");
        let prompt = directory.path().join("prompt");
        let config = claude(refusing(
            &directory,
            &prompt,
            REFUSAL,
            Some("model_requires_usage_credits"),
        ));
        let task = task();
        let pull = pull();

        let error = run(
            &Endpoint::instance(&config),
            backend::instance(&config),
            PrService::new(),
            request(&task, &pull, "fable"),
        )
        .await
        .expect_err("a refused review");

        let ReviewError::Unfunded { reviewer, reason } = &error else {
            panic!("expected an unfunded reviewer, got {error:?}");
        };
        assert_eq!(reviewer, "fable");
        assert_eq!(reason, &format!("claude: {UNFUNDED}: {REFUSAL}"));
        assert_eq!(
            error.stalled(),
            Some(format!(
                "the reviewer model fable cannot run: claude: {UNFUNDED}: {REFUSAL}"
            )),
            "the task pauses with claude's words rather than asking every tick"
        );
    }

    /// A model that fails for any other reason is a failed round, and the next
    /// tick asks another reviewer.
    #[tokio::test]
    async fn a_reviewer_that_hit_its_plans_window_is_a_model_failure() {
        let directory = TempDir::new().expect("a temporary directory");
        let prompt = directory.path().join("prompt");
        let config = claude(refusing(
            &directory,
            &prompt,
            "You've hit your session limit · resets 5pm",
            None,
        ));
        let task = task();
        let pull = pull();

        let error = run(
            &Endpoint::instance(&config),
            backend::instance(&config),
            PrService::new(),
            request(&task, &pull, "sonnet"),
        )
        .await
        .expect_err("a refused review");

        assert!(matches!(error, ReviewError::Model(_)), "{error:?}");
        assert_eq!(error.stalled(), None, "the next tick asks another reviewer");
    }

    /// An endpoint's failure is never read for a coding agent's wording.
    #[test]
    fn an_endpoint_failure_in_the_words_of_an_unfunded_reviewer_is_a_model_failure() {
        let error = ReviewError::model(
            &LlmBackend::Http,
            "fable",
            LlmError::Api {
                status: StatusCode::BAD_REQUEST.as_u16(),
                message: format!("{UNFUNDED}: {REFUSAL}"),
            },
        );

        assert!(matches!(error, ReviewError::Model(_)), "{error:?}");
        assert_eq!(error.stalled(), None);
    }

    /// Zone's words for a coding agent's failure count only among the
    /// agent's own words, never in the stderr that follows them.
    #[test]
    fn an_unfunded_reviewer_is_read_from_the_agents_own_words() {
        let agent = LlmBackend::cli(AgentKind::Claude, zone_core::llm::CliSettings::default());
        let stderr_only = format!(
            "claude: the agent exited without completing its event stream{}{UNFUNDED}: {REFUSAL}",
            zone_core::llm::provider::STDERR_HEADING
        );

        assert!(matches!(
            ReviewError::model(&agent, "fable", LlmError::Agent(stderr_only)),
            ReviewError::Model(_)
        ));
        assert!(matches!(
            ReviewError::model(
                &agent,
                "sonnet[1m]",
                LlmError::Agent(format!(
                    "claude: {UNFUNDED_CONTEXT}: API Error: Usage credits required for 1M context"
                ))
            ),
            ReviewError::Unfunded { .. }
        ));
    }

    fn endpoint(url: String) -> Endpoint {
        Endpoint::instance(&Config {
            litellm_host: url,
            ..crate::state::test_config()
        })
    }

    async fn review_on(endpoint: &Endpoint) -> ReviewError {
        let task = task();
        let pull = pull();
        run(
            endpoint,
            LlmBackend::Http,
            PrService::new(),
            request(&task, &pull, "qwen3:32b"),
        )
        .await
        .expect_err("the endpoint gave no verdict")
    }

    async fn answering(status: u16, message: &str) -> wiremock::MockServer {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"error": {"message": message}})),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_reviewer_endpoint_nothing_listens_on_is_unreachable_and_records_no_round() {
        use crate::workers::auto_project::pipeline::{Attempt, recover};

        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("a free port")
            .local_addr()
            .expect("its address")
            .port();

        let error = review_on(&endpoint(format!("http://127.0.0.1:{port}"))).await;

        assert!(matches!(error, ReviewError::Unreachable(_)), "{error:?}");
        let lineup = ["qwen3:32b".to_string()];
        let attempt = Attempt {
            outages: &outage::Outages::default(),
            project: Uuid::nil(),
            task: Uuid::nil(),
            lineup: &lineup,
            now: chrono::Utc::now(),
            origin: crate::services::endpoint::Origin::Instance,
        };
        assert_eq!(recover(&error, "qwen3:32b", 0, &attempt).missed, None);
    }

    #[tokio::test]
    async fn an_endpoint_that_is_restarting_or_overloaded_is_unreachable() {
        for status in [408, 429, 500, 502, 503, 504] {
            let server = answering(status, "Ollama is starting").await;

            let error = review_on(&endpoint(server.uri())).await;

            assert!(
                matches!(error, ReviewError::Unreachable(_)),
                "{status}: {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_model_that_refuses_the_review_is_a_failed_round_whatever_status_the_proxy_gives() {
        const REFUSAL: &str = "llava:7b does not support tools";
        for (status, message) in [(400, REFUSAL), (500, REFUSAL), (404, "model not found")] {
            let server = answering(status, message).await;

            let error = review_on(&endpoint(server.uri())).await;

            assert!(
                matches!(error, ReviewError::Model(_)),
                "{status}: {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_review_round_goes_to_the_organization_endpoint_with_its_key() {
        use super::model::{Author, Venue};
        use super::testing::{
            INSTANCE_KEY, ORGANIZATION_KEY, Organization, SAVED_MODEL, Saved, authorization,
            completing, received,
        };

        let instance = completing(REPLY, "stop").await;
        let saved = completing(REPLY, "stop").await;
        let host = saved.uri();
        let organization = Organization::saving(Saved {
            host: Some(&host),
            key: Some(ORGANIZATION_KEY),
            fast: Some(SAVED_MODEL),
        })
        .await;
        let state = organization.state(&instance.uri());
        let venue = Venue::for_workspace(&state, organization.workspace).await;
        organization.remove().await;
        let venue = venue.expect("an endpoint needs no sign-in");
        let reviewers = venue
            .lineup(&Author::Unrecorded, &["instance-reviewer".to_string()], 1)
            .expect("the saved model reviews");
        let task = task();
        let pull = pull();

        let verdict = run(
            &venue.endpoint,
            venue.backend,
            PrService::new(),
            request(&task, &pull, &reviewers[0].model),
        )
        .await
        .expect("the saved endpoint gave a verdict");

        assert_eq!(verdict.outcome, Outcome::Approve);
        assert!(
            received(&instance).await.is_empty(),
            "the instance's LITELLM_HOST was sent the review with {INSTANCE_KEY}"
        );
        let requests = received(&saved).await;
        let [request] = requests.as_slice() else {
            panic!(
                "the saved endpoint was asked for the review {} times",
                requests.len()
            );
        };
        assert_eq!(
            authorization(request),
            Some(format!("Bearer {ORGANIZATION_KEY}"))
        );
        let body: Value = request.body_json().expect("a JSON completion request");
        assert_eq!(
            body["model"], SAVED_MODEL,
            "the instance's ZONE_AUTO_REVIEW_MODELS name no model on the saved endpoint"
        );
    }

    #[tokio::test]
    async fn a_review_on_an_endpoint_the_instance_no_longer_allows_pauses_asking_no_one() {
        use super::model::Venue;
        use super::testing::{
            INSTANCE_KEY, ORGANIZATION_KEY, Organization, SAVED_MODEL, Saved, completing, received,
        };
        use crate::config::Config;
        use crate::services::hosts::Hosts;
        use crate::state::AppState;

        let instance = completing(REPLY, "stop").await;
        let saved = completing(REPLY, "stop").await;
        let host = saved.uri();
        let organization = Organization::saving(Saved {
            host: Some(&host),
            key: Some(ORGANIZATION_KEY),
            fast: Some(SAVED_MODEL),
        })
        .await;
        let state = AppState::new(
            Config {
                litellm_host: instance.uri(),
                litellm_key: INSTANCE_KEY.to_string(),
                endpoint_hosts: Hosts::parse("llm.corp.example"),
                ..crate::state::test_config()
            },
            organization.pool.clone(),
            None,
        );

        let venue = Venue::for_workspace(&state, organization.workspace).await;
        organization.remove().await;

        let Err(reason) = venue else {
            panic!("a review venue was resolved on an endpoint the instance does not allow");
        };
        let reason = reason.to_string();
        assert!(
            reason.starts_with("This workspace's AI endpoint can't be used")
                && reason.ends_with("Check AI Settings."),
            "{reason}"
        );
        assert!(!reason.contains(ORGANIZATION_KEY), "{reason}");
        assert!(!reason.contains(INSTANCE_KEY), "{reason}");
        assert!(received(&instance).await.is_empty());
        assert!(received(&saved).await.is_empty());
    }

    #[tokio::test]
    async fn a_provider_that_echoes_the_key_never_puts_it_in_the_review() {
        use crate::services::endpoint::testing::settings;
        use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
        use zone_core::secret::SecretValue;

        const KEY: &str = "sk-organization-0a7d44e19b";
        let server = answering(
            401,
            &format!("Incorrect API key provided: {KEY}. Also seen as sk-org****e19b."),
        )
        .await;
        let saved = crate::db::ai_settings::EffectiveAiSettings {
            litellm_host: Some(server.uri()),
            litellm_key: Some(SecretValue::new(KEY.to_string())),
            ..settings(PROVIDER_SELF_HOSTED)
        };

        let error = review_on(&Endpoint::resolve(&crate::state::test_config(), &saved)).await;

        let recorded = error.to_string();
        assert!(matches!(error, ReviewError::Model(_)), "{error:?}");
        assert!(!recorded.contains(KEY), "{recorded}");
        assert!(!recorded.contains("sk-org****e19b"), "{recorded}");
    }
}
