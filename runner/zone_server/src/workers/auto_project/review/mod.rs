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
use zone_core::llm::{LlmClient, LlmConfig, Message, ToolDefinition};
use zone_core::tools::{ToolContext, ToolResult};
use zone_vcs::pull_request::{ChangedFile, PrService, PullRequestDetail, PullRequestReference};

use crate::db::auto_projects::{Finding, ReviewRow};
use crate::db::tasks::TaskRow;

pub use verdict::{Outcome, Verdict};

/// Turns a review may take: reads, then the verdict.
const MAX_TURNS: usize = 12;
/// How long one review may run.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(600);
/// Tokens the verdict turn may spend.
const REVIEW_TOKENS: u32 = 4_096;
/// A review is a judgement, not a draft.
const REVIEW_TEMPERATURE: f32 = 0.0;

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("the reviewer's reply carried no readable verdict: {0}")]
    Unparseable(String),
    #[error("the reviewer model failed: {0}")]
    Model(String),
    #[error("the review did not finish within {} seconds", REVIEW_TIMEOUT.as_secs())]
    TimedOut,
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
    litellm_host: &str,
    litellm_key: &str,
    pr: PrService,
    request: ReviewRequest<'_>,
) -> Result<Verdict, ReviewError> {
    let client = LlmClient::new(LlmConfig {
        base_url: litellm_host.to_string(),
        api_key: litellm_key.to_string(),
        default_model: request.reviewer.clone(),
        temperature: REVIEW_TEMPERATURE,
        max_tokens: REVIEW_TOKENS,
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
    let definitions: Vec<ToolDefinition> = registry.definitions();
    let context = ToolContext::default();
    let mut messages = vec![
        Message::system(prompt::system(request.round, request.same_model)),
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
                .chat_with_model(&reviewer, &messages, Some(&definitions))
                .await
                .map_err(|error| ReviewError::Model(error.to_string()))?;
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
