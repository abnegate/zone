//! Everything a chat turn watches its coding agent for.

use std::fmt;

use abnegate_agent_cli::{Output, Subagent};
use zone_core::llm::Observer;

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
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use abnegate_agent_cli::{
        AgentKind, DEFAULT_TOOL_OUTPUT_LIMIT, StepStatus, SubagentStatus, SubagentStep,
    };
    use abnegate_llm::{LlmConfig, Message, RequestOptions, ToolCall};
    use futures::StreamExt;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tempfile::TempDir;
    use tokio::sync::mpsc::{self, UnboundedReceiver};
    use uuid::Uuid;
    use zone_core::llm::{LlmBackend, LlmClient, cli};

    use super::*;
    use crate::db::chat_pull_requests::{self, PullRequestRow};
    use crate::db::chat_subagents;
    use crate::db::chats::create_chat;
    use crate::db::context::RUNNING as TURN_RUNNING;
    use crate::services::chat::subagent::ChatSubagent;
    use crate::state::AppState;

    const LINK: &str = "https://github.com/acme/widgets/pull/7";
    const PATIENCE: Duration = Duration::from_secs(10);
    const MODEL: &str = "sonnet";

    /// The most the line saying how much of a tool's output was left out takes.
    const OMISSION_NOTE: usize = 256;

    type Announced = UnboundedReceiver<(ChatSubagent, Vec<SubagentStep>)>;

    /// A running turn of a new chat, as the chat's store begins it.
    struct Turn {
        state: AppState,
        chat_id: Uuid,
        message_id: Uuid,
    }

    impl Turn {
        async fn new() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated test database"),
            )
            .await
            .expect("the test database accepts connections");
            let chat_id = create_chat(&pool, None, "Observers", MODEL, false, true)
                .await
                .expect("a chat")
                .id;
            let message = Uuid::new_v4();
            let message_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'user', 'Go')",
            )
            .bind(message)
            .bind(chat_id)
            .execute(&pool)
            .await
            .expect("a user message");
            sqlx::query(
                "INSERT INTO chat_turns (id, chat_id, user_message_id, fence, status) VALUES ($1, $2, $3, 1, $4)",
            )
            .bind(message_id)
            .bind(chat_id)
            .bind(message)
            .bind(TURN_RUNNING)
            .execute(&pool)
            .await
            .expect("a turn");
            Self {
                state: AppState::new(crate::state::test_config(), pool, None),
                chat_id,
                message_id,
            }
        }

        fn recorder(&self) -> Recorder {
            Recorder::new(self.state.clone(), self.chat_id, Uuid::new_v4(), Uuid::new_v4())
        }

        /// The chat's observers, as a turn's agent is handed them, and what
        /// the tracker among them announces.
        fn observers(&self) -> (Observers, Announced) {
            let (announce, announced) = mpsc::unbounded_channel();
            let tracker = Tracker::new(
                self.state.clone(),
                self.chat_id,
                self.message_id,
                Arc::new(announce),
            );
            (Observers::new(self.recorder(), tracker), announced)
        }

        /// The pull requests recorded for the chat, once there are any.
        async fn pull_requests(&self) -> Vec<PullRequestRow> {
            tokio::time::timeout(PATIENCE, async {
                loop {
                    let recorded = chat_pull_requests::list(self.state.db(), self.chat_id)
                        .await
                        .expect("the chat's pull requests");
                    if !recorded.is_empty() {
                        return recorded;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the pull request is recorded in time")
        }
    }

    /// Every announcement the tracker makes until it has finished.
    async fn settled(mut announced: Announced) -> Vec<(ChatSubagent, Vec<SubagentStep>)> {
        let mut settled = Vec::new();
        loop {
            match tokio::time::timeout(PATIENCE, announced.recv()).await {
                Ok(Some(announcement)) => settled.push(announcement),
                Ok(None) => return settled,
                Err(_) => panic!("the tracker never finished: {settled:?}"),
            }
        }
    }

    /// A Claude stand-in that prints `events` as its stream, then answers.
    fn claude(directory: &TempDir, events: &[Value]) -> PathBuf {
        let stream = directory.path().join("stream.jsonl");
        let answer = [
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Done."}]}, "parent_tool_use_id": null}),
            json!({"type": "result", "subtype": "success", "is_error": false, "result": "Done."}),
        ];
        let lines: String = events
            .iter()
            .chain(&answer)
            .map(|event| format!("{event}\n"))
            .collect();
        std::fs::write(&stream, lines).expect("the stand-in's stream");
        let executable = directory.path().join("claude");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\ncat > /dev/null\ncat '{}'\n", stream.display()),
        )
        .expect("the stand-in's script");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("an executable stand-in");
        executable
    }

    /// Runs one turn of the agent at `executable` to its end, `observer`
    /// watching it, and lets go of the observer once the turn is over.
    async fn run(executable: PathBuf, observer: Arc<dyn Observer>) {
        let client = LlmClient::new(LlmConfig::default())
            .with_backend(LlmBackend::cli(
                AgentKind::Claude,
                cli::settings()
                    .with_executable(executable)
                    .with_timeout(PATIENCE),
            ))
            .with_observer(observer);
        let mut stream = client
            .chat_stream_with_options(
                MODEL,
                &[Message::user("Open a pull request")],
                None,
                RequestOptions::new(512),
            )
            .await
            .expect("the turn starts");
        while let Some(chunk) = stream.next().await {
            chunk.expect("the turn runs to its end");
        }
    }

    /// Hands the chat's observers everything, and keeps how long each
    /// output was.
    #[derive(Debug)]
    struct Measured {
        observers: Observers,
        lengths: Mutex<Vec<usize>>,
    }

    impl Observer for Measured {
        fn observe(&self, output: &Output) {
            self.lengths
                .lock()
                .expect("the lengths")
                .push(output.text.len());
            self.observers.observe(output);
        }

        fn track(&self, subagent: &Subagent) {
            self.observers.track(subagent);
        }
    }

    #[tokio::test]
    async fn outputs_go_to_the_pull_requests_and_subagents_to_the_tracker() {
        let turn = Turn::new().await;
        let (tracker, mut tracked) = Tracker::listening();
        let observers = Observers::new(turn.recorder(), tracker);

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
        let recorded = turn.pull_requests().await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].url, LINK);
    }

    #[tokio::test]
    async fn an_agents_turn_records_its_subagents_and_pull_requests_through_the_chats_observers() {
        const SURVEY: &str = "toolu_01Survey";
        const GREP: &str = "toolu_02Grep";
        let turn = Turn::new().await;
        let (observers, announced) = turn.observers();
        let directory = TempDir::new().expect("a directory for the stand-in");
        let executable = claude(
            &directory,
            &[
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": SURVEY, "name": "Agent", "input": {"description": "Survey the crate", "prompt": "List the crate's modules.", "subagent_type": "Explore"}}]}, "parent_tool_use_id": null}),
                json!({"type": "system", "subtype": "task_started", "task_id": "a1b2c3d4e", "tool_use_id": SURVEY, "description": "Survey the crate", "subagent_type": "Explore", "is_backgrounded": false, "task_type": "local_agent"}),
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": GREP, "name": "Grep", "input": {"pattern": "pub mod", "path": "/w/src"}}]}, "parent_tool_use_id": SURVEY}),
                json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": GREP, "content": "/w/src/lib.rs:1:pub mod llm;"}]}, "parent_tool_use_id": SURVEY}),
                json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": SURVEY, "content": [{"type": "text", "text": "One module."}]}]}, "parent_tool_use_id": null, "tool_use_result": {"status": "completed", "agentId": "a1b2c3d4e", "content": [{"type": "text", "text": "One module."}], "resolvedModel": "claude-haiku-4-5", "totalDurationMs": 5400, "totalTokens": 18200, "totalToolUseCount": 1}}),
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "toolu_03Open", "name": "Bash", "input": {"command": "gh pr create --fill"}}]}, "parent_tool_use_id": null}),
                json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_03Open", "content": format!("Creating pull request\n\n{LINK}\n")}]}, "parent_tool_use_id": null}),
            ],
        );

        run(executable, Arc::new(observers)).await;
        let announcements = settled(announced).await;

        assert!(!announcements.is_empty(), "nothing was announced");
        let stored = chat_subagents::list(turn.state.db(), turn.chat_id)
            .await
            .expect("the chat's subagents");
        let [subagent] = stored.as_slice() else {
            panic!("expected the one subagent, got {stored:?}");
        };
        assert_eq!(subagent.external_id, SURVEY);
        assert_eq!(subagent.message_id, Some(turn.message_id));
        assert_eq!(subagent.description, "Survey the crate");
        assert_eq!(subagent.kind.as_deref(), Some("Explore"));
        assert_eq!(subagent.status, SubagentStatus::Completed.as_str());
        assert_eq!(
            (subagent.duration_ms, subagent.tokens, subagent.tool_uses),
            (Some(5400), Some(18200), Some(1))
        );
        let steps = chat_subagents::steps(turn.state.db(), turn.chat_id, subagent.id)
            .await
            .expect("the subagent's steps")
            .expect("the subagent");
        assert_eq!(
            steps
                .iter()
                .map(|step| (step.id.as_str(), step.name.as_str(), step.status))
                .collect::<Vec<_>>(),
            [(GREP, "Grep", StepStatus::Succeeded)]
        );
        let recorded = turn.pull_requests().await;
        assert_eq!(
            recorded
                .iter()
                .map(|pull_request| pull_request.url.as_str())
                .collect::<Vec<_>>(),
            [LINK]
        );
    }

    #[tokio::test]
    async fn a_pull_request_opened_in_a_capped_output_is_still_recorded() {
        const PROGRESS: &str = "remote: Resolving deltas: 100% (812/812), done.\n";
        let turn = Turn::new().await;
        let pushed = PROGRESS.repeat(2 * DEFAULT_TOOL_OUTPUT_LIMIT / PROGRESS.len());
        let printed = format!(
            "{pushed}Creating pull request for feature into main in acme/widgets\n\n{LINK}\n"
        );
        let directory = TempDir::new().expect("a directory for the stand-in");
        let executable = claude(
            &directory,
            &[
                json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "toolu_01Open", "name": "Bash", "input": {"command": "git push -u origin feature && gh pr create --fill"}}]}, "parent_tool_use_id": null}),
                json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_01Open", "content": printed}]}, "parent_tool_use_id": null}),
            ],
        );
        let (observers, _) = turn.observers();
        let measured = Arc::new(Measured {
            observers,
            lengths: Mutex::default(),
        });

        run(executable, measured.clone()).await;

        let lengths = measured.lengths.lock().expect("the lengths").clone();
        let [length] = lengths.as_slice() else {
            panic!("expected one output, got {lengths:?}");
        };
        assert!(
            *length <= DEFAULT_TOOL_OUTPUT_LIMIT + OMISSION_NOTE && *length < printed.len(),
            "the output was not capped: {length} of {} bytes",
            printed.len()
        );
        let recorded = turn.pull_requests().await;
        assert_eq!(
            recorded
                .iter()
                .map(|pull_request| pull_request.url.as_str())
                .collect::<Vec<_>>(),
            [LINK]
        );
    }
}
