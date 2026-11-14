//! Noting the pull requests a chat's turns open.

use std::fmt;

use abnegate_agent_cli::Output;
use uuid::Uuid;
use zone_core::llm::Observer;

use super::credential;
use super::detection;
use super::opened::Opened;
use crate::db::chat_pull_requests::{self, NewPullRequest, PullRequestRow};
use crate::state::AppState;

/// Watches a coding agent's own tools during one chat turn, and records each
/// pull request one of them opened as the chat's.
pub struct Recorder {
    state: AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    /// The member whose turn this is, whom the pull requests it opens are
    /// recorded as watched by.
    user_id: Uuid,
}

impl Recorder {
    pub fn new(state: AppState, chat_id: Uuid, workspace_id: Uuid, user_id: Uuid) -> Self {
        Self {
            state,
            chat_id,
            workspace_id,
            user_id,
        }
    }
}

impl fmt::Debug for Recorder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Recorder")
            .field("chat_id", &self.chat_id)
            .finish_non_exhaustive()
    }
}

impl Observer for Recorder {
    fn observe(&self, output: &Output) {
        let Some(url) = detection::opened(output) else {
            return;
        };
        let state = self.state.clone();
        let opened = Opened {
            chat_id: self.chat_id,
            workspace_id: self.workspace_id,
            url,
            title: None,
            source_id: None,
            opened_by: Some(self.user_id),
        };
        tokio::spawn(async move {
            if let Err(error) = record(&state, &opened).await {
                tracing::warn!(chat = %opened.chat_id, %error, "could not record a pull request the agent opened");
            }
        });
    }
}

/// Records the pull request as its chat's. A link on a host the configured
/// GitHub origin does not answer for is not one Zone can follow, and is left
/// out.
pub async fn record(
    state: &AppState,
    opened: &Opened,
) -> Result<Option<PullRequestRow>, sqlx::Error> {
    let Some(reference) = state
        .config()
        .pull_requests()
        .ok()
        .and_then(|service| service.pull_request(&opened.url).ok())
    else {
        tracing::debug!(chat = %opened.chat_id, "a pull request link outside the configured GitHub was not recorded");
        return Ok(None);
    };
    let Ok(number) = i64::try_from(reference.number().get()) else {
        return Ok(None);
    };
    let repository = reference.repository();
    let source_id = match opened.source_id {
        Some(id) => Some(id),
        None => credential::repository_source(
            state,
            opened.workspace_id,
            repository.owner(),
            repository.name(),
        )
        .await?
        .map(|source| source.id),
    };
    chat_pull_requests::record(
        state.db(),
        &NewPullRequest {
            chat_id: opened.chat_id,
            owner: repository.owner(),
            repo: repository.name(),
            number,
            url: &opened.url,
            title: opened.title.as_deref(),
            source_id,
            opened_by: opened.opened_by,
        },
    )
    .await
    .map(Some)
}
