//! A review of one head by one model, over the diff and the files it may read.
//!
//! Modelled on the conflict repair agent: a bounded loop over a hand-built
//! registry of tools, with no shell, no disk and no way to change anything.
//! The reply ends with a verdict, and a reply that does not is asked once more
//! and then counted as a failed round.

pub mod bots;
pub mod model;
pub mod prompt;
pub mod tools;
pub mod verdict;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use zone_core::llm::provider::{UNFUNDED, UNFUNDED_CONTEXT};
use zone_core::llm::{LlmBackend, LlmClient, LlmConfig, Message, ToolDefinition};
use zone_core::tools::{ToolContext, ToolResult};
use zone_vcs::pull_request::{ChangedFile, PrService, PullRequestDetail, PullRequestReference};

use crate::config::Config;
use crate::db::auto_projects::{Finding, ReviewRow};
use crate::db::tasks::TaskRow;
use crate::services::backend;

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
    #[error("the reviewer model {reviewer} cannot run: {reason}")]
    Unfunded { reviewer: String, reason: String },
    #[error("the review did not finish within {} seconds", REVIEW_TIMEOUT.as_secs())]
    TimedOut,
}

impl ReviewError {
    /// `message`, what failed the `reviewer` model the review ran on
    /// `backend`.
    fn model(backend: &LlmBackend, reviewer: &str, message: String) -> Self {
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

    /// Why the task has to wait for a person, when no later tick would get
    /// past this failure.
    pub fn stalled(&self) -> Option<String> {
        matches!(self, Self::Unfunded { .. }).then(|| self.to_string())
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

/// Run one review and return its verdict.
pub async fn run(
    config: &Config,
    backend: LlmBackend,
    pr: PrService,
    request: ReviewRequest<'_>,
) -> Result<Verdict, ReviewError> {
    let client = LlmClient::new(LlmConfig {
        base_url: config.litellm_host.clone(),
        api_key: config.litellm_key.clone(),
        default_model: request.reviewer.clone(),
        temperature: REVIEW_TEMPERATURE,
        max_tokens: REVIEW_TOKENS,
        backend,
    });
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
                .map_err(|error| {
                    ReviewError::model(&client.config().backend, &reviewer, error.to_string())
                })?;
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
        Ok(outcome) => outcome,
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

    use crate::config::ModelBackend;

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
            &config,
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
            &config,
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

    /// A model that fails for any other reason may answer on the next tick.
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
            &config,
            backend::instance(&config),
            PrService::new(),
            request(&task, &pull, "sonnet"),
        )
        .await
        .expect_err("a refused review");

        assert!(matches!(error, ReviewError::Model(_)), "{error:?}");
        assert_eq!(error.stalled(), None, "the next tick asks again");
    }

    /// An endpoint's failure is never read for a coding agent's wording.
    #[test]
    fn an_endpoint_failure_in_the_words_of_an_unfunded_reviewer_is_a_model_failure() {
        let error =
            ReviewError::model(&LlmBackend::Http, "fable", format!("{UNFUNDED}: {REFUSAL}"));

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
            ReviewError::model(&agent, "fable", stderr_only),
            ReviewError::Model(_)
        ));
        assert!(matches!(
            ReviewError::model(
                &agent,
                "sonnet[1m]",
                format!(
                    "claude: {UNFUNDED_CONTEXT}: API Error: Usage credits required for 1M context"
                )
            ),
            ReviewError::Unfunded { .. }
        ));
    }
}
