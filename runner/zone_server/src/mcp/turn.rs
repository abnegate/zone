//! The one turn a bearer token stands for.
//!
//! A token is minted when a CLI-backed turn opens and revoked when its
//! [`Lease`] drops, so it names a workspace, the chat or task run it serves and
//! who acts in it for as long as that turn runs and nothing afterwards. The
//! turn it names is what decides every call: its own tool registry answers
//! `tools/list`, and its own [`ApprovalPolicy`] decides each `tools/call`
//! before the registry runs it.

use dashmap::DashMap;
use futures::{Stream, StreamExt};
use once_cell::sync::Lazy;
use rmcp::model::{CallToolResult, ContentBlock};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use zone_core::llm::Toolset;
use zone_core::llm::provider::DEFAULT_TIMEOUT;
use zone_core::tools::ToolResult;

use crate::agent::{AgentEvent, ApprovalPolicy, ChatTools};
use crate::utils::crypto::{generate_token, hash_token};

/// A token lives exactly as long as the agent it was minted for may run, so a
/// lease that leaks still cannot outlast the process holding it.
const LIFETIME: Duration = DEFAULT_TIMEOUT;

/// What a call is told when the reader did not allow it. The same words a
/// chat's own denied call is told, because it is the same decision.
const DENIED: &str = "The user denied this tool call.";

/// Distinguishes a call the agent made over MCP from the chat's own ids in the
/// console trace and in the approval gate.
const CALL_PREFIX: &str = "mcp_";

/// How much of a finished call's output the console shows beside it.
const DETAIL_CHARS: usize = 200;

static LIVE: Lazy<DashMap<String, Arc<Turn>>> = Lazy::new(DashMap::new);

/// One CLI-backed turn, and everything a call arriving for it is decided by.
pub struct Turn {
    workspace: Uuid,
    chat: Uuid,
    user: Uuid,
    tools: Arc<ChatTools>,
    approval: ApprovalPolicy,
    /// Where an approval card and the call trace reach the console. The turn's
    /// driver owns the receiving half and publishes what arrives into the same
    /// stream the chat's own tool calls are published on.
    events: UnboundedSender<AgentEvent>,
    /// Whether the tools that end zone's own turn are served.
    parks: bool,
    expires: Instant,
}

impl Turn {
    pub fn new(
        workspace: Uuid,
        chat: Uuid,
        user: Uuid,
        tools: Arc<ChatTools>,
        approval: ApprovalPolicy,
        events: UnboundedSender<AgentEvent>,
    ) -> Self {
        Self {
            workspace,
            chat,
            user,
            tools,
            approval,
            events,
            parks: true,
            expires: Instant::now() + LIFETIME,
        }
    }

    /// Serve none of the tools that end zone's own turn: a question, a wait,
    /// a plan. Each answers that the turn ends there and that the answer or the
    /// outcome comes next, which only zone's own loop makes true. Over MCP the
    /// call returns, nothing parks, and a task run ends on that promise.
    pub fn without_parks(mut self) -> Self {
        self.parks = false;
        self
    }

    /// Mint this turn's token and publish it under the given endpoint.
    ///
    /// The returned lease is the turn's registration: drop it and the token
    /// stops being accepted, which is what makes it single-turn.
    pub fn open(self, endpoint: impl Into<String>) -> Lease {
        let token = generate_token();
        let hash = hash_token(&token);
        let served = self.tools.names().iter().filter(|name| self.serves(name));
        let toolset = Toolset::new(endpoint, token, served);
        LIVE.insert(hash.clone(), Arc::new(self));
        Lease { hash, toolset }
    }

    /// Whether a call to `name` reaches this turn's registry.
    pub(crate) fn serves(&self, name: &str) -> bool {
        self.tools.has(name) && (self.parks || !self.tools.ends_turn(name))
    }

    pub(crate) fn find(token: &str) -> Option<Arc<Self>> {
        let hash = hash_token(token);
        let turn = LIVE.get(&hash).map(|entry| Arc::clone(&entry))?;
        if turn.expires > Instant::now() {
            return Some(turn);
        }
        LIVE.remove(&hash);
        None
    }

    pub(crate) fn workspace(&self) -> Uuid {
        self.workspace
    }

    pub(crate) fn chat(&self) -> Uuid {
        self.chat
    }

    pub(crate) fn user(&self) -> Uuid {
        self.user
    }

    pub(crate) fn tools(&self) -> &ChatTools {
        &self.tools
    }

    /// Run one tool call for this turn, gated by the chat's approval policy.
    pub(crate) async fn run(&self, name: &str, arguments: &str) -> CallToolResult {
        let id = format!("{CALL_PREFIX}{}", Uuid::new_v4().simple());
        self.publish(AgentEvent::ToolCallStarted {
            id: id.clone(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        });

        let started = Instant::now();
        let result = if self.allowed(&id, name, arguments).await {
            self.tools.execute(name, arguments).await
        } else {
            ToolResult::error(DENIED)
        };
        let receipt = self
            .tools
            .write_receipt(&id, name, arguments, &result)
            .await;
        let message = result.to_message();

        self.publish(AgentEvent::ToolCallCompleted {
            id,
            name: name.to_string(),
            success: result.success,
            detail: detail(&message),
            duration_ms: started.elapsed().as_millis() as u64,
            citations: match result.success {
                true => crate::agent::citations::from_tool(name, &message),
                false => Vec::new(),
            },
            receipt,
        });

        let content = vec![ContentBlock::text(message)];
        match result.success {
            true => CallToolResult::success(content),
            false => CallToolResult::error(content),
        }
    }

    /// Whether this call may run: immediately when the chat auto-approves,
    /// otherwise once the reader has answered the card it raises.
    ///
    /// A console that is no longer listening refuses the call rather than
    /// holding it for the approval timeout nobody is there to answer.
    async fn allowed(&self, id: &str, name: &str, arguments: &str) -> bool {
        let tier = self.tools.tier(name);
        if !self.approval.confirms(tier) {
            return true;
        }
        let pending = self.approval.expect_decision(id);
        let raised = self.publish(AgentEvent::ToolApprovalRequired {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            reason: crate::agent::reason(arguments),
            preview: self.tools.effect(name, arguments),
        });
        if !raised {
            self.approval.decide(id, false);
            return false;
        }
        self.approval.awaited_decision(id, pending).await
    }

    fn publish(&self, event: AgentEvent) -> bool {
        self.events.send(event).is_ok()
    }
}

/// A live turn's registration. Dropping it revokes the token.
pub struct Lease {
    hash: String,
    toolset: Toolset,
}

impl Lease {
    /// What the spawned agent is configured with: where to reach zone, the
    /// token that reaches it, and the tools this turn may call.
    pub fn toolset(&self) -> Toolset {
        self.toolset.clone()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        LIVE.remove(&self.hash);
    }
}

/// Everything a round produced, in one stream: zone's own loop, and the calls
/// a spawned agent made over MCP while that loop was waiting on its answer.
///
/// Ends with the loop rather than with the channel. The lease holding the
/// channel open outlives the round on purpose, so a merge that waited for it
/// to close would never end the turn.
pub fn merged<'a>(
    events: impl Stream<Item = AgentEvent> + Send + 'a,
    calls: &'a mut UnboundedReceiver<AgentEvent>,
) -> impl Stream<Item = AgentEvent> + Send + 'a {
    async_stream::stream! {
        futures::pin_mut!(events);
        loop {
            tokio::select! {
                biased;
                Some(call) = calls.recv() => yield call,
                event = events.next() => match event {
                    Some(event) => yield event,
                    None => break,
                },
            }
        }
        while let Ok(call) = calls.try_recv() {
            yield call;
        }
    }
}

fn detail(message: &str) -> String {
    let line = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    match line.char_indices().nth(DETAIL_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{drain, next_card, open, tools, wait_for_card, write};
    use super::*;
    use crate::agent::wait::{self, WAIT_FOR};
    use crate::agent::{ASK_USER, ApprovalGate};
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::sync::mpsc::unbounded_channel;
    use zone_core::tools::Session;
    use zone_core::tools::job::{JobCommand, Jobs};

    #[tokio::test]
    async fn a_token_reaches_the_turn_it_was_minted_for() {
        let opened = open(ApprovalPolicy::auto()).await;

        let found = Turn::find(&opened.token).expect("the minted token reaches its turn");

        assert_eq!(found.chat(), opened.chat);
        assert_eq!(found.workspace(), opened.workspace);
        assert_eq!(found.user(), opened.user);
        assert!(Turn::find("not-a-token").is_none());
    }

    #[tokio::test]
    async fn a_dropped_lease_revokes_the_token() {
        let opened = open(ApprovalPolicy::auto()).await;
        assert!(Turn::find(&opened.token).is_some());

        drop(opened.lease);

        assert!(
            Turn::find(&opened.token).is_none(),
            "a token must not outlive the turn it was minted for"
        );
    }

    #[tokio::test]
    async fn an_expired_turn_stops_answering() {
        let chat = Uuid::new_v4();
        let (sender, _events) = unbounded_channel();
        let mut turn = Turn::new(
            Uuid::new_v4(),
            chat,
            Uuid::new_v4(),
            tools(chat).await,
            ApprovalPolicy::auto(),
            sender,
        );
        turn.expires = Instant::now() - Duration::from_secs(1);
        let lease = turn.open("http://127.0.0.1:8080/mcp");

        assert!(Turn::find(lease.toolset().token.expose()).is_none());
    }

    #[tokio::test]
    async fn an_auto_approve_turn_runs_without_waiting() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("auto.txt");
        let mut opened = open(ApprovalPolicy::auto()).await;
        let turn = Turn::find(&opened.token).expect("the turn");

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            turn.run("write_file", &write(&path).to_string()),
        )
        .await
        .expect("an auto-approve turn must not wait for anybody");

        assert_eq!(result.is_error, Some(false), "{result:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "written");
        assert_eq!(
            drain(&mut opened.events),
            ["started", "completed"],
            "an auto-approve turn must raise no card"
        );
    }

    #[tokio::test]
    async fn a_confirmed_call_waits_and_a_denial_refuses_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("denied.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        let arguments = write(&path).to_string();

        let call = tokio::spawn(async move { turn.run("write_file", &arguments).await });

        let card = next_card(&mut opened.events).await;
        assert_eq!(card.name, "write_file");
        assert_eq!(
            card.reason.as_deref(),
            Some("The turn asked for this file."),
            "the card carries the model's stated reason, as a chat card does"
        );
        let preview = card
            .preview
            .expect("the card describes what the call will do");
        assert!(preview.contains("denied.txt"), "{preview}");
        assert!(
            !call.is_finished(),
            "a confirmed call must wait for the answer"
        );

        assert!(opened.approval.decide(&card.id, false), "the denial lands");

        let result = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .expect("a denied call returns as soon as it is denied")
            .expect("the call task");
        assert_eq!(result.is_error, Some(true), "{result:?}");
        assert!(
            format!("{:?}", result.content).contains(DENIED),
            "{:?}",
            result.content
        );
        assert!(
            !path.exists(),
            "a denied write must not have touched the filesystem"
        );
    }

    #[tokio::test]
    async fn a_confirmed_call_the_console_cannot_hear_is_refused_rather_than_held() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("unheard.txt");
        let opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        drop(opened.events);

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            turn.run("write_file", &write(&path).to_string()),
        )
        .await
        .expect("a call nobody can answer must not hold for the approval timeout");

        assert_eq!(result.is_error, Some(true), "{result:?}");
        assert!(!path.exists());
    }

    /// The chat's live auto-approve flag reaches a call already waiting here,
    /// the same way it reaches one the chat loop is holding.
    #[tokio::test]
    async fn turning_auto_approve_on_releases_a_waiting_call() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("released.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        ApprovalPolicy::register(opened.chat, opened.approval.clone());
        let turn = Turn::find(&opened.token).expect("the turn");
        let arguments = write(&path).to_string();

        let call = tokio::spawn(async move { turn.run("write_file", &arguments).await });
        wait_for_card(&mut opened.events).await;

        ApprovalPolicy::set_chat_auto(opened.chat, true);

        let result = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .expect("auto-approve releases the waiter")
            .expect("the call task");
        assert_eq!(result.is_error, Some(false), "{result:?}");
        ApprovalPolicy::unregister(opened.chat, &opened.approval);
    }

    /// Parking is zone's own loop acting on the call it made. A call that
    /// arrives over MCP returns at once and succeeds, and nothing parks behind
    /// it: no question is registered, and the wait `wait_for` stages is left
    /// for a loop that never binds it.
    #[tokio::test]
    async fn a_call_that_would_end_zones_turn_parks_nothing_over_mcp() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut opened = open(ApprovalPolicy::auto()).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        let session = Session::Chat(opened.chat);
        let environment = HashMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )]);
        let job = Jobs::spawn(
            session,
            &JobCommand::shell("sleep 30"),
            directory.path(),
            &environment,
        )
        .await
        .expect("the job starts");
        let question = json!({"questions": [{
            "header": "Scope",
            "question": "Which scope?",
            "options": [
                {"label": "Backfill", "description": "Do the backfill"},
                {"label": "Forward only", "description": "Skip the backfill"}
            ],
        }]});

        for (name, arguments) in [
            (ASK_USER, question),
            (WAIT_FOR, json!({"kind": "job", "id": job.id})),
        ] {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                turn.run(name, &arguments.to_string()),
            )
            .await
            .expect("a call that would park zone's loop returns at once over MCP");

            assert_eq!(result.is_error, Some(false), "{name}: {result:?}");
            assert_eq!(
                drain(&mut opened.events),
                ["started", "completed"],
                "{name}"
            );
        }
        assert!(
            wait::bind(session, "unbound").is_some(),
            "the wait was staged, and nothing over MCP binds it to a park"
        );

        wait::reset_session(session);
        Jobs::kill_session(session).await;
    }

    #[tokio::test]
    async fn a_turn_without_parks_offers_no_tool_that_ends_zones_turn() {
        let chat = Uuid::new_v4();
        let registry = tools(chat).await;
        let (sender, _events) = unbounded_channel();
        let lease = Turn::new(
            Uuid::new_v4(),
            chat,
            Uuid::new_v4(),
            Arc::clone(&registry),
            ApprovalPolicy::auto(),
            sender,
        )
        .without_parks()
        .open("http://127.0.0.1:8080/mcp");
        let offered = lease.toolset().tools;
        let turn = Turn::find(lease.toolset().token.expose()).expect("the turn");

        for name in [ASK_USER, WAIT_FOR] {
            assert!(registry.has(name), "the registry holds {name}");
            assert!(
                !offered.iter().any(|tool| tool == name),
                "{name} was offered"
            );
            assert!(!turn.serves(name), "{name} is served");
        }
        assert!(turn.serves("write_file"));
        assert_eq!(offered.len(), registry.names().len() - 2);

        let parking = open(ApprovalPolicy::auto()).await;
        assert!(
            parking
                .lease
                .toolset()
                .tools
                .iter()
                .any(|tool| tool == ASK_USER),
            "a turn that has not withheld them still offers them"
        );
    }

    #[test]
    fn a_detail_is_one_line_the_console_can_show() {
        assert_eq!(detail("\n\nfirst line\nsecond line"), "first line");
        let long = detail(&"x".repeat(DETAIL_CHARS * 2));
        assert_eq!(long.chars().count(), DETAIL_CHARS + 1, "{long}");
        assert!(long.ends_with('\u{2026}'));
    }
}
