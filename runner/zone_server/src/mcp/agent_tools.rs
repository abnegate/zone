//! Zone's tools on a coding agent's turn: which it is offered, and serving
//! them to it.

use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use zone_core::llm::LlmBackend;

use super::scope::Scope;
use super::turn::{Lease, Turn};
use crate::agent::{AgentEvent, ChatTools};

/// What a CLI-backed turn needs to reach zone's tools, for exactly as long as
/// it is held: the lease keeping the turn's token minted, and the calls the
/// agent makes arriving as the events its driver already handles.
pub struct AgentTools {
    pub lease: Lease,
    pub calls: UnboundedReceiver<AgentEvent>,
}

impl AgentTools {
    /// Serve `tools` to the agent `backend` spawns, at `endpoint`, for the
    /// turn `scope` describes.
    ///
    /// The agent runs its own tool loop and cannot be handed zone's schemas
    /// over the completions API, so the registry moves out of zone's loop,
    /// which under such a backend runs one round and calls nothing, and into
    /// the turn the endpoint answers for. `None` for an endpoint, which carries
    /// its tools in the request itself, and then they stay where they were.
    pub fn serve(
        backend: &LlmBackend,
        tools: &mut ChatTools,
        scope: Scope,
        endpoint: &str,
    ) -> Option<Self> {
        let LlmBackend::Cli { .. } = backend else {
            return None;
        };
        let registry = Arc::new(std::mem::replace(tools, ChatTools::empty()));
        let (events, calls) = mpsc::unbounded_channel();
        let lease = Turn::new(scope, registry, events).open(endpoint);
        Some(Self { lease, calls })
    }
}

/// The tools a turn on `backend` offers its model.
///
/// A coding agent runs the loop itself and reaches zone's tools over MCP,
/// where a call that would end zone's own turn -- a question, a wait, a plan
/// -- returns at once and parks nothing. So it is offered none, a prompt
/// rendered from what this returns teaches none, and the tools it keeps say
/// that waiting on one needs a tool it does not have. An endpoint's turn is
/// offered its tools as they are.
pub fn offered(backend: &LlmBackend, tools: ChatTools) -> ChatTools {
    match backend {
        LlmBackend::Cli { .. } => {
            let parks: Vec<String> = tools
                .names()
                .iter()
                .filter(|name| tools.ends_turn(name))
                .cloned()
                .collect();
            tools.without(&parks)
        }
        LlmBackend::Http => tools,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;
    use crate::agent::WorkspaceScope;
    use crate::agent::question::ASK_USER;
    use crate::agent::wait::WAIT_FOR;
    use uuid::Uuid;
    use zone_core::llm::{AgentKind, CliSettings};
    use zone_core::tools::WaitFor;

    const ENDPOINT: &str = "http://127.0.0.1:8421/mcp";

    /// Where a text sits in a tool's definition: its description, or a
    /// description in its schema.
    const DESCRIPTION: &str = "";
    const BACKGROUND: &str = "/properties/background/description";
    const COMMAND: &str = "/properties/command/description";

    const CREATE_REMINDER: &str = "Schedule a durable reminder delivered to this chat. due_at is \
        the first firing: a future RFC3339 time with an explicit timezone offset, and the exact \
        time the person named. Clarify an ambiguous date or timezone rather than guessing, and do \
        not claim this will run a task. Add rrule to repeat it, as an RFC 5545 rule in this \
        subset: FREQ (HOURLY, DAILY, WEEKLY, MONTHLY), INTERVAL, BYDAY, BYHOUR, BYMINUTE, \
        BYMONTHDAY, UNTIL, COUNT. A clause outside that list is refused rather than dropped. \
        FREQ=HOURLY is the finest cadence there is; MINUTELY and SECONDLY are refused, not rounded \
        up. Once an hour is the ceiling, measured at the shortest gap the rule produces rather \
        than its average — BYHOUR=0,1 with BYMINUTE=0,30 fires four times a day and three of those \
        gaps are half an hour. A condition that changes faster than the ceiling wants wait_for on \
        the event itself, not a schedule. A repeating reminder stops after seven days unless it is \
        asked for again. Without a prompt, each firing delivers content as it is written. With \
        one, each firing runs the prompt as a turn of your own in this chat and what you say is \
        the delivery, and content is not sent at all — it stays as the schedule's name, which is \
        what list_reminders shows and what the person reads when deciding whether to cancel it, so \
        make it a short description of the standing job rather than a message. Use a prompt when \
        the useful answer has to be worked out at the time, and content alone when it is the same \
        words every time. A prompt is an instruction to your future self, which will have this \
        chat and these tools and no memory of writing it, so say what to check and what to report. \
        End it with the rule that if nothing changed, it should say nothing: a schedule that \
        reports every firing whether or not anything happened teaches the person to ignore it. Set \
        timing_mode to condition_watch when the change is the point rather than the time. Each \
        firing is handed what the last one answered and asked what differs, so the prompt only has \
        to say what to look at — the comparison is supplied, and so is the rule for an unchanged \
        firing, which for a watch is one short line rather than silence. A watch needs both an \
        rrule and a prompt and is refused without them: one firing has nothing to compare against, \
        and fixed content has nothing to compare. Two limits to state when you offer one. It sees \
        only the state at each firing, so a condition that appears and disappears between two \
        firings is never noticed — for something that raises an event of its own, use wait_for on \
        the event rather than a watch. And a watch cannot stay silent: running the turn is how it \
        reports at all, so an unchanged firing still answers here, in one short line. That is the \
        one place a watch departs from the say-nothing rule above, so a watch's prompt should not \
        repeat that rule. Offer a repeat when somebody plainly wants the same thing again; never \
        turn a request made once into a standing one they did not ask for.";

    const START_TASK: &str = "Create an agentic coding task and start the background runner \
        immediately. Returns task_id and run_id. Does not wait for completion — wait for it with \
        wait_for, then read get_task_run and tail_task_log. Use only when the user asked to run \
        work in the background.";

    const TAIL_TASK_LOG: &str = "Fetch new runner log lines since a previous log ID. Read a run's \
        progress with it once rather than calling it again: from a chat, find out when the run \
        finishes by waiting for it with wait_for kind=task_run; from inside a run, finish and let \
        whoever started it coordinate.";

    const BACKGROUND_JOB: &str = "Detach and return immediately with a job id and log path. Use \
        for anything long-running; wait for it with wait_for instead of blocking. A background job \
        ends with the turn that started it, or with the run. Default false.";

    const SHELL_COMMAND: &str = "Shell command to run, e.g. 'cargo test 2>&1 | tail -40'. It may \
        not block on sleep for more than 60 seconds: to wait longer, start it with background: \
        true and wait for it with wait_for.";

    const RUNNER_STARTED: &str = "Runner started. Wait for it with wait_for, then read \
        get_task_run and tail_task_log; do not claim the work finished.";

    const SLEEP_REFUSED: &str = "Execution failed: This command sleeps for 600 seconds, and a call \
        may block on sleep for at most 60. Start it with background: true and wait for it with \
        wait_for.";

    const BACKGROUNDED_SLEEP_REFUSED: &str = "Execution failed: This command sleeps for 600 \
        seconds, and a call may block on sleep for at most 60. Backgrounding does not raise the \
        cap. Start something that finishes on its own and wait for it with wait_for rather than \
        sleeping.";

    /// Every text an endpoint's turn is served that names wait_for, as it read
    /// at 4d27d1f4, before coding agents were served zone's tools: the tool,
    /// where in its definition the text sits, and the text.
    const ENDPOINT_WAITS: [(&str, &str, &str); 6] = [
        ("create_reminder", DESCRIPTION, CREATE_REMINDER),
        ("run_command", BACKGROUND, BACKGROUND_JOB),
        ("run_shell", BACKGROUND, BACKGROUND_JOB),
        ("run_shell", COMMAND, SHELL_COMMAND),
        ("start_task", DESCRIPTION, START_TASK),
        ("tail_task_log", DESCRIPTION, TAIL_TASK_LOG),
    ];

    async fn chat() -> ChatTools {
        ChatTools::build(WorkspaceScope {
            state: testing::state(),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
        })
        .await
    }

    async fn task() -> ChatTools {
        ChatTools::for_task(
            &testing::state(),
            std::env::temp_dir(),
            Uuid::new_v4(),
            None,
        )
        .await
    }

    #[tokio::test]
    async fn an_agents_turn_is_offered_no_tool_that_ends_zones_turn() {
        for agent in AgentKind::ALL {
            let backend = LlmBackend::cli(agent, CliSettings::default());
            for tools in [chat().await, task().await] {
                let every = tools.names().to_vec();

                let offered = offered(&backend, tools);

                for name in [ASK_USER, WAIT_FOR] {
                    assert!(every.iter().any(|known| known == name), "{name}");
                    assert!(!offered.has(name), "{agent} was offered {name}");
                }
                assert!(offered.has("read_file"), "{:?}", offered.names());
                assert_eq!(offered.names().len(), every.len() - 2);
            }
        }
    }

    /// What an agent is offered still names `wait_for` where a turn that has
    /// the tool is told to use it, so each of those says it needs the tool:
    /// every definition, the refusals a call reads, and the runner's start.
    #[tokio::test]
    async fn nothing_an_agent_is_offered_sends_it_to_wait_for_regardless() {
        let backend = LlmBackend::cli(AgentKind::Codex, CliSettings::default());
        let mut texts = vec![crate::agent::actions::RUNNER_STARTED_UNWAITED.to_string()];
        for tools in [chat().await, task().await] {
            let offered = offered(&backend, tools);
            texts.extend(
                offered
                    .all_definitions()
                    .iter()
                    .map(|definition| serde_json::to_string(definition).expect("a definition")),
            );
            if offered.has("run_shell") {
                for background in [false, true] {
                    let refused = offered.execute("run_shell", &sleeping(background)).await;
                    texts.extend(refused.error);
                }
            }
        }

        let waits: Vec<&String> = texts
            .iter()
            .filter(|text| text.contains(WAIT_FOR))
            .collect();
        assert!(waits.len() >= ENDPOINT_WAITS.len(), "{waits:?}");
        for text in waits {
            assert_eq!(
                text.matches(WAIT_FOR).count(),
                text.matches(WaitFor::Withheld.condition()).count(),
                "{text}"
            );
        }
    }

    /// A shell call that sleeps past the cap, refused before anything runs.
    fn sleeping(background: bool) -> String {
        serde_json::json!({
            "command": "sleep 600",
            "background": background,
            "reason": "Wait for the deploy.",
        })
        .to_string()
    }

    /// The self-hosted path is held to what it sent models before coding
    /// agents were served zone's tools: every text an endpoint's turn reads
    /// that names wait_for is the text it read then, byte for byte, and no
    /// other text names it.
    #[tokio::test]
    async fn an_endpoints_turn_reads_every_instruction_to_wait_as_it_always_did() {
        let mut served = std::collections::HashSet::new();
        for tools in [chat().await, task().await] {
            let offered = offered(&LlmBackend::Http, tools);
            for definition in offered.all_definitions() {
                let name = definition.function.name.as_str();
                let mut expected = 0;
                for (tool, at, text) in ENDPOINT_WAITS.iter().filter(|(tool, ..)| *tool == name) {
                    let actual = match *at {
                        DESCRIPTION => Some(definition.function.description.as_str()),
                        pointer => definition
                            .function
                            .parameters
                            .pointer(pointer)
                            .and_then(serde_json::Value::as_str),
                    };
                    assert_eq!(actual, Some(*text), "{tool}{at}");
                    expected += text.matches(WAIT_FOR).count();
                    served.insert((*tool, *at));
                }
                let written = format!(
                    "{}{}",
                    definition.function.description, definition.function.parameters
                );
                assert_eq!(
                    written.matches(WAIT_FOR).count(),
                    expected,
                    "{name} names wait_for where an endpoint's turn never read it: {written}"
                );
            }
            if offered.has("run_shell") {
                for (background, refusal) in
                    [(false, SLEEP_REFUSED), (true, BACKGROUNDED_SLEEP_REFUSED)]
                {
                    let refused = offered.execute("run_shell", &sleeping(background)).await;
                    assert_eq!(refused.error.as_deref(), Some(refusal));
                }
            }
        }

        assert_eq!(served.len(), ENDPOINT_WAITS.len(), "{served:?}");
        assert_eq!(crate::db::actions::RUNNER_STARTED, RUNNER_STARTED);
    }

    #[tokio::test]
    async fn an_endpoints_turn_is_offered_every_tool() {
        for tools in [chat().await, task().await] {
            let every = tools.names().to_vec();

            assert_eq!(offered(&LlmBackend::Http, tools).names(), every);
        }
    }

    #[tokio::test]
    async fn an_agent_is_served_what_its_turn_offers_and_zones_loop_keeps_nothing() {
        for agent in AgentKind::ALL {
            let backend = LlmBackend::cli(agent, CliSettings::default());
            for tools in [chat().await, task().await] {
                let mut tools = offered(&backend, tools);
                let names = tools.names().to_vec();

                let served = AgentTools::serve(
                    &backend,
                    &mut tools,
                    testing::scope(Uuid::new_v4()),
                    ENDPOINT,
                )
                .unwrap_or_else(|| panic!("{agent} lets zone decide its tools"));

                assert!(
                    tools.is_empty(),
                    "the registry the endpoint answers from must not also sit in zone's own loop"
                );
                let toolset = served.lease.toolset();
                assert_eq!(toolset.endpoint, ENDPOINT);
                assert_eq!(toolset.tools, names);
            }
        }
    }

    #[tokio::test]
    async fn an_endpoints_turn_serves_nothing_and_keeps_every_tool() {
        let mut tools = task().await;
        let names = tools.names().to_vec();

        let served = AgentTools::serve(
            &LlmBackend::Http,
            &mut tools,
            testing::scope(Uuid::new_v4()),
            ENDPOINT,
        );

        assert!(served.is_none(), "an endpoint spawns nothing to serve");
        assert_eq!(tools.names(), names);
    }

    #[tokio::test]
    async fn a_served_turns_token_works_until_it_is_dropped() {
        let backend = LlmBackend::cli(AgentKind::Claude, CliSettings::default());
        let mut tools = task().await;
        let chat = Uuid::new_v4();
        let served = AgentTools::serve(&backend, &mut tools, testing::scope(chat), ENDPOINT)
            .expect("claude lets zone decide its tools");
        let token = served.lease.toolset().token.expose().to_string();

        let turn = Turn::find(&token).expect("the token reaches its turn while it runs");
        assert_eq!(turn.chat(), chat);
        drop(served);

        assert!(
            Turn::find(&token).is_none(),
            "a token that outlives its turn is a standing grant on this workspace"
        );
    }
}
