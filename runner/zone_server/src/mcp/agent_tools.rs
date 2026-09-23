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
/// -- returns at once and parks nothing. So it is offered none, and a prompt
/// rendered from what this returns teaches none.
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

    const ENDPOINT: &str = "http://127.0.0.1:8421/mcp";

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
