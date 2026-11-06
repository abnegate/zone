//! Everything a chat turn watches its coding agent for.

use std::fmt;

use zone_core::llm::{Observer, Output, Subagent};

use super::pull_request::Recorder;
use super::subagent::Tracker;

/// A turn's agent takes one observer, so the pull requests it opens and the
/// subagents it starts are watched through this one.
pub struct Observers {
    pull_requests: Recorder,
    subagents: Tracker,
}

impl Observers {
    pub fn new(pull_requests: Recorder, subagents: Tracker) -> Self {
        Self {
            pull_requests,
            subagents,
        }
    }
}

impl fmt::Debug for Observers {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Observers")
            .field("pull_requests", &self.pull_requests)
            .field("subagents", &self.subagents)
            .finish()
    }
}

impl Observer for Observers {
    fn observe(&self, output: &Output) {
        self.pull_requests.observe(output);
    }

    fn track(&self, subagent: &Subagent) {
        self.subagents.track(subagent);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use abnegate_llm::ToolCall;
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::*;
    use crate::db::chat_pull_requests;
    use crate::db::chats::create_chat;
    use crate::state::AppState;

    const LINK: &str = "https://github.com/acme/widgets/pull/7";
    const PATIENCE: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn outputs_go_to_the_pull_requests_and_subagents_to_the_tracker() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let chat_id = create_chat(&pool, None, "Observers", "sonnet", false, true)
            .await
            .expect("a chat")
            .id;
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let (tracker, mut tracked) = Tracker::listening(state.clone());
        let observers = Observers::new(Recorder::new(state, chat_id, Uuid::new_v4()), tracker);

        observers.track(&Subagent::new("toolu_01Agent"));
        observers.observe(&Output::new(
            ToolCall::function(
                "call_1",
                "Bash",
                json!({ "command": "gh pr create --fill" }).to_string(),
            ),
            format!("Creating pull request\n\n{LINK}\n"),
            true,
        ));

        assert_eq!(
            tracked
                .try_recv()
                .expect("the tracker heard of the subagent"),
            Subagent::new("toolu_01Agent")
        );
        let recorded = tokio::time::timeout(PATIENCE, async {
            loop {
                let recorded = chat_pull_requests::list(&pool, chat_id)
                    .await
                    .expect("the chat's pull requests");
                if !recorded.is_empty() {
                    return recorded;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the pull request is recorded in time");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].url, LINK);
    }
}
