//! The one turn a bearer token stands for.
//!
//! A token is minted when a CLI-backed turn opens and revoked when its
//! [`Lease`] drops, so it names a workspace, the chat or task run it serves and
//! who acts in it for as long as that turn runs and nothing afterwards. The
//! turn it names is what decides every call: its own tool registry answers
//! `tools/list`, and its own [`crate::agent::ApprovalPolicy`] decides each
//! `tools/call` before the registry runs it.

use dashmap::DashMap;
use futures::{Stream, StreamExt};
use once_cell::sync::Lazy;
use rmcp::model::{CallToolResult, ContentBlock};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use zone_core::llm::Toolset;
use zone_core::tools::ToolResult;

use super::scope::Scope;
use crate::agent::{AgentEvent, ChatTools};
use crate::utils::crypto::{generate_token, hash_token};

/// What a call past the turn's budget is told instead of being run.
const SPENT: &str =
    "This turn has made every tool call it was allowed. Finish with what you already have.";

/// What the console is told when the budget runs out, in the words zone's own
/// loop uses for the same thing.
const EXHAUSTED: &str = "The configured tool-call budget was reached.";

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
    scope: Scope,
    tools: Arc<ChatTools>,
    /// Where an approval card and the call trace reach the console. The turn's
    /// driver owns the receiving half and publishes what arrives into the same
    /// stream the chat's own tool calls are published on.
    events: UnboundedSender<AgentEvent>,
    /// Calls the agent has made, refused ones included.
    made: AtomicUsize,
}

impl Turn {
    pub fn new(scope: Scope, tools: Arc<ChatTools>, events: UnboundedSender<AgentEvent>) -> Self {
        Self {
            scope,
            tools,
            events,
            made: AtomicUsize::new(0),
        }
    }

    /// Mint this turn's token and publish it under the given endpoint.
    ///
    /// The returned lease is the turn's registration: drop it and the token
    /// stops being accepted, which is what makes it single-turn. Nothing else
    /// ends it, so the agent keeps zone's tools for as long as its turn runs.
    pub fn open(self, endpoint: impl Into<String>) -> Lease {
        let token = generate_token();
        let hash = hash_token(&token);
        let served = self.tools.names().iter().filter(|name| self.serves(name));
        let toolset = Toolset::new(endpoint, token, served);
        LIVE.insert(hash.clone(), Arc::new(self));
        Lease { hash, toolset }
    }

    /// Whether a call to `name` reaches this turn's registry.
    ///
    /// A tool that ends zone's own turn -- a question, a wait, a plan -- never
    /// does. It answers that the answer or the outcome comes next, which only
    /// zone's own loop makes true: over MCP the call returns at once, nothing
    /// parks, and the turn reads that as an answer nobody gave.
    pub(crate) fn serves(&self, name: &str) -> bool {
        self.tools.has(name) && !self.tools.ends_turn(name)
    }

    pub(crate) fn find(token: &str) -> Option<Arc<Self>> {
        LIVE.get(&hash_token(token)).map(|entry| Arc::clone(&entry))
    }

    pub(crate) fn workspace(&self) -> Uuid {
        self.scope.workspace
    }

    pub(crate) fn chat(&self) -> Uuid {
        self.scope.chat
    }

    pub(crate) fn user(&self) -> Option<Uuid> {
        self.scope.user
    }

    pub(crate) fn tools(&self) -> &ChatTools {
        &self.tools
    }

    /// Run one tool call for this turn, gated by the chat's approval policy
    /// and held to the turn's budget.
    pub(crate) async fn run(&self, name: &str, arguments: &str) -> CallToolResult {
        let made = self.made.fetch_add(1, Ordering::Relaxed);
        if made >= self.scope.calls {
            return self.refuse(made);
        }
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
        let approval = &self.scope.approval;
        if !approval.confirms(tier) {
            return true;
        }
        let pending = approval.expect_decision(id);
        let raised = self.publish(AgentEvent::ToolApprovalRequired {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            reason: crate::agent::reason(arguments),
            preview: self.tools.effect(name, arguments),
        });
        if !raised {
            approval.decide(id, false);
            return false;
        }
        approval.awaited_decision(id, pending).await
    }

    /// Refuse a call past the budget, the `made`th this turn. The console is
    /// told once, when the budget runs out, and shown no call that never ran.
    fn refuse(&self, made: usize) -> CallToolResult {
        if made == self.scope.calls {
            self.publish(AgentEvent::Finalizing(EXHAUSTED.to_string()));
        }
        CallToolResult::error(vec![ContentBlock::text(SPENT)])
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
    use super::super::testing::{drain, next_card, open, scope, tools, wait_for_card, write};
    use super::*;
    use crate::agent::wait::{self, WAIT_FOR};
    use crate::agent::{ASK_USER, ApprovalGate, ApprovalPolicy};
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::sync::mpsc::unbounded_channel;
    use zone_core::llm::provider::DEFAULT_TIMEOUT;
    use zone_core::tools::Session;
    use zone_core::tools::job::{JobCommand, Jobs};

    #[tokio::test]
    async fn a_token_reaches_the_turn_it_was_minted_for() {
        let opened = open(ApprovalPolicy::auto()).await;

        let found = Turn::find(&opened.token).expect("the minted token reaches its turn");

        assert_eq!(found.chat(), opened.chat);
        assert_eq!(found.workspace(), opened.workspace);
        assert_eq!(found.user(), Some(opened.user));
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

    /// A task attempt runs for an hour and a chat for as long as the operator
    /// sets, so a token that expired on its own clock would cut the agent off
    /// from zone's tools partway through. The lease is the only thing that
    /// ends it.
    #[tokio::test(start_paused = true)]
    async fn a_token_answers_for_as_long_as_its_lease_is_held() {
        let opened = open(ApprovalPolicy::auto()).await;

        tokio::time::advance(DEFAULT_TIMEOUT * 4).await;

        assert!(
            Turn::find(&opened.token).is_some(),
            "a turn that outlasts the agent's default timeout lost its tools"
        );
        drop(opened.lease);
        assert!(Turn::find(&opened.token).is_none());
    }

    #[tokio::test]
    async fn a_turn_refuses_every_call_past_its_budget_without_running_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let chat = Uuid::new_v4();
        let (sender, mut events) = unbounded_channel();
        let lease = Turn::new(
            Scope {
                calls: 2,
                ..scope(chat)
            },
            tools(chat).await,
            sender,
        )
        .open("http://127.0.0.1:8080/mcp");
        let turn = Turn::find(lease.toolset().token.expose()).expect("the turn");

        for number in 0..2 {
            let path = directory.path().join(format!("{number}.txt"));
            let result = turn.run("write_file", &write(&path).to_string()).await;
            assert_eq!(result.is_error, Some(false), "{result:?}");
        }
        assert_eq!(
            drain(&mut events),
            ["started", "completed", "started", "completed"]
        );

        let over = directory.path().join("over.txt");
        for _ in 0..2 {
            let refused = turn.run("write_file", &write(&over).to_string()).await;
            assert_eq!(refused.is_error, Some(true), "{refused:?}");
            assert!(
                format!("{:?}", refused.content).contains(SPENT),
                "{:?}",
                refused.content
            );
        }
        assert!(!over.exists(), "a call past the budget ran");
        let told: Vec<AgentEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(
            matches!(told.as_slice(), [AgentEvent::Finalizing(_)]),
            "the console is told once that the budget ran out, and shown no call that never \
             ran: {told:?}"
        );
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

    /// A chat's turn and a task's alike: over MCP a call that would end zone's
    /// turn returns at once, having parked nothing, so no turn serves one.
    #[tokio::test]
    async fn a_turn_offers_no_tool_that_ends_zones_turn() {
        let chat = Uuid::new_v4();
        let registry = tools(chat).await;
        let (sender, _events) = unbounded_channel();
        let lease =
            Turn::new(scope(chat), Arc::clone(&registry), sender).open("http://127.0.0.1:8080/mcp");
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
    }

    #[test]
    fn a_detail_is_one_line_the_console_can_show() {
        assert_eq!(detail("\n\nfirst line\nsecond line"), "first line");
        let long = detail(&"x".repeat(DETAIL_CHARS * 2));
        assert_eq!(long.chars().count(), DETAIL_CHARS + 1, "{long}");
        assert!(long.ends_with('\u{2026}'));
    }
}
