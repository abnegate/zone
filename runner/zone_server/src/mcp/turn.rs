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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, Toolset};
use zone_core::tools::ToolResult;

use super::progress::Progress;
use super::scope::Scope;
use crate::agent::runner::call_signature;
use crate::agent::trace::content_lines;
use crate::agent::{AgentEvent, ChatTools, Decision, Refusal};
use crate::utils::crypto::{generate_token, hash_token};

/// What a call past the turn's budget is told instead of being run.
const SPENT: &str =
    "This turn has made every tool call it was allowed. Finish with what you already have.";

/// What the console is told when the budget runs out, in the words zone's own
/// loop uses for the same thing.
const EXHAUSTED: &str = "The configured tool-call budget was reached.";

/// What a call that already failed, with nothing changed since, is told
/// instead of being run.
const REPEATED_CALL: &str = "This call already failed and nothing has changed since, so it was not run. Try something \
     else.";

/// What every call is told once the turn has stopped making progress.
const STALLED: &str = "Repeated failed calls made no progress. Finish with what you already have.";

/// What the console is told when the turn stalls, in the words zone's own loop
/// uses for the same thing.
const REPEATED: &str = "Repeated failed calls made no progress.";

/// What the agent is told when zone could not read its permission question.
const UNREADABLE: &str = "Zone could not read that permission request.";

/// What the agent is told when no console is open to answer its question.
const UNATTENDED: &str = "Nobody is listening to this chat to answer for it.";

/// What the console shows on a permission card the reader allowed. The agent
/// runs the tool itself, so zone never sees it finish.
const ALLOWED: &str = "Allowed";

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
    progress: Mutex<Progress>,
    /// Set once a failed call is repeated with nothing changed since. Every
    /// later call is refused, as zone's own loop finalizes the turn.
    stalled: AtomicBool,
}

impl Turn {
    pub fn new(scope: Scope, tools: Arc<ChatTools>, events: UnboundedSender<AgentEvent>) -> Self {
        Self {
            scope,
            tools,
            events,
            made: AtomicUsize::new(0),
            progress: Mutex::default(),
            stalled: AtomicBool::new(false),
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

    /// Whether a call to `name` is answered here: a tool this turn serves, or
    /// the question an agent asks before running one of its own.
    pub(crate) fn answers(&self, name: &str) -> bool {
        name == Toolset::PERMISSION || self.serves(name)
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

    /// Answer an agent's question about one of *its own* tools.
    ///
    /// Claude asks here before running a tool its own permission rules would
    /// stop. Zone answers under the chat's approval policy: at once when the
    /// chat auto-approves, otherwise as the same card zone's own confirmed
    /// tools raise. Nothing is bypassed; the decision moves to the one place
    /// the reader can see and answer it.
    ///
    /// Spends no budget. The agent asking whether a call may run is not the
    /// call, and charging for the question would halve what a turn can do.
    pub(crate) async fn permission(&self, arguments: &str) -> CallToolResult {
        let Ok(asked) = serde_json::from_str::<Question>(arguments) else {
            return verdict(None, UNREADABLE);
        };
        let approval = &self.scope.approval;
        let id = format!("{CALL_PREFIX}{}", Uuid::new_v4().simple());
        let pending = approval.expect_decision(&id);
        if pending.is_none() {
            return verdict(Some(&asked.input), "");
        }
        let stated = asked.input.to_string();
        self.publish(AgentEvent::ToolCallStarted {
            id: id.clone(),
            name: asked.tool_name.clone(),
            arguments: stated.clone(),
        });
        let started = Instant::now();
        let raised = self.publish(AgentEvent::ToolApprovalRequired {
            id: id.clone(),
            name: asked.tool_name.clone(),
            arguments: stated.clone(),
            reason: crate::agent::reason(&stated),
            // Zone renders an effect for its own tools; it knows nothing about
            // what the agent's own tool would do beyond what the agent said.
            preview: None,
        });
        if !raised {
            approval.withdraw(&id);
            return verdict(None, UNATTENDED);
        }
        let refusal = approval.awaited_decision(&id, pending).await.refusal();
        self.publish(AgentEvent::ToolCallCompleted {
            id,
            name: asked.tool_name,
            success: refusal.is_none(),
            detail: refusal.map_or(ALLOWED, Refusal::message).to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
            citations: Vec::new(),
            receipt: None,
            refusal,
        });
        match refusal {
            None => verdict(Some(&asked.input), ""),
            Some(refusal) => verdict(None, refusal.message()),
        }
    }

    /// Run one tool call for this turn, gated by the chat's approval policy
    /// and held to the turn's budget and to making progress.
    pub(crate) async fn run(&self, name: &str, arguments: &str) -> CallToolResult {
        if name == Toolset::PERMISSION {
            return self.permission(arguments).await;
        }
        let made = self.made.fetch_add(1, Ordering::Relaxed);
        if made >= self.scope.calls {
            return self.refuse(made);
        }
        if self.stalled.load(Ordering::Relaxed) {
            return self.stall();
        }
        let signature = call_signature(name, arguments);
        let generation = {
            let mut progress = self.progress();
            if self.sees_every_change() && progress.repeats(&signature) {
                let warned = progress.warn();
                drop(progress);
                return match warned {
                    true => self.stall(),
                    false => CallToolResult::error(vec![ContentBlock::text(REPEATED_CALL)]),
                };
            }
            progress.generation()
        };
        let id = format!("{CALL_PREFIX}{}", Uuid::new_v4().simple());
        self.publish(AgentEvent::ToolCallStarted {
            id: id.clone(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        });

        let started = Instant::now();
        let refusal = self.allowed(&id, name, arguments).await.refusal();
        let result = match refusal {
            None => self.tools.execute(name, arguments).await,
            Some(refusal) => ToolResult::error(refusal.message()),
        };
        let receipt = self
            .tools
            .write_receipt(&id, name, arguments, &result)
            .await;
        let message = result.to_message();
        self.progress().record(
            signature,
            generation,
            self.tools.mutating(name),
            result.success,
            &message,
        );

        self.publish(AgentEvent::ToolCallCompleted {
            id,
            name: name.to_string(),
            success: result.success,
            detail: detail(name, &message),
            duration_ms: started.elapsed().as_millis() as u64,
            citations: match result.success {
                true => crate::agent::citations::from_tool(name, &message),
                false => Vec::new(),
            },
            receipt,
            refusal,
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
    async fn allowed(&self, id: &str, name: &str, arguments: &str) -> Decision {
        let tier = self.tools.tier(name);
        let approval = &self.scope.approval;
        if !approval.confirms(tier) {
            return Decision::Approved;
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
            approval.withdraw(id);
            return Decision::Cancelled;
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

    /// Refuse a call once the turn has stopped making progress. The console is
    /// told once, when it stalls, and shown no call that never ran.
    fn stall(&self) -> CallToolResult {
        if !self.stalled.swap(true, Ordering::Relaxed) {
            self.publish(AgentEvent::Finalizing(REPEATED.to_string()));
        }
        CallToolResult::error(vec![ContentBlock::text(STALLED)])
    }

    /// Whether every change the agent can make goes through zone. Its own file
    /// and shell tools change things zone never sees, so a call that failed
    /// may succeed when made again with nothing zone could call progress.
    fn sees_every_change(&self) -> bool {
        self.scope.builtin == BuiltinTools::Withheld
    }

    fn progress(&self) -> MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, event: AgentEvent) -> bool {
        self.events.send(event).is_ok()
    }
}

/// What the agent asks about one of its own tools.
#[derive(serde::Deserialize)]
struct Question {
    tool_name: String,
    input: serde_json::Value,
}

/// The answer, in the shape the agent reads it in.
///
/// Always a successful result: the question was answered either way, and
/// returning an error would read to the agent as zone failing to answer
/// rather than as a refusal it should respect.
fn verdict(allowed: Option<&serde_json::Value>, refusal: &str) -> CallToolResult {
    let body = match allowed {
        Some(input) => serde_json::json!({"behavior": "allow", "updatedInput": input}),
        None => serde_json::json!({"behavior": "deny", "message": refusal}),
    };
    CallToolResult::success(vec![ContentBlock::text(body.to_string())])
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
                // The agent writes before it calls, so what it wrote is counted ahead of the call.
                biased;
                event = events.next() => match event {
                    Some(ended @ (AgentEvent::Limited(_) | AgentEvent::Failed(_) | AgentEvent::CutShort)) => {
                        while let Ok(call) = calls.try_recv() {
                            yield call;
                        }
                        yield ended;
                    }
                    Some(event) => yield event,
                    None => break,
                },
                Some(call) = calls.recv() => yield call,
            }
        }
        while let Ok(call) = calls.try_recv() {
            yield call;
        }
    }
}

fn detail(tool: &str, message: &str) -> String {
    let line = content_lines(tool, message).next().unwrap_or_default();
    match line.char_indices().nth(DETAIL_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod permission_tests {
    use super::super::testing::{next_card, open};
    use super::*;
    use crate::agent::{ApprovalGate, ApprovalPolicy};
    use serde_json::json;

    fn asked() -> String {
        json!({"tool_name": "Bash", "input": {"command": "ls"}}).to_string()
    }

    fn body(result: &CallToolResult) -> serde_json::Value {
        let text = result
            .content
            .iter()
            .find_map(|block| block.as_text())
            .expect("the answer is a text block")
            .text
            .clone();
        serde_json::from_str(&text).expect("the answer is a JSON envelope")
    }

    #[tokio::test]
    async fn an_auto_approving_chat_answers_the_agent_at_once() {
        let opened = open(ApprovalPolicy::auto()).await;
        let turn = Turn::find(&opened.token).expect("the turn");

        let answer = turn.run(Toolset::PERMISSION, &asked()).await;

        assert_eq!(
            answer.is_error,
            Some(false),
            "the question was answered, so the result is not an error"
        );
        assert_eq!(body(&answer)["behavior"], "allow", "{:?}", answer.content);
    }

    #[tokio::test]
    async fn a_refused_card_reaches_the_agent_as_a_denial() {
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");

        let asking = tokio::spawn(async move { turn.run(Toolset::PERMISSION, &asked()).await });

        let card = next_card(&mut opened.events).await;
        assert_eq!(
            card.name, "Bash",
            "the card names the agent's own tool, not zone's"
        );
        assert!(opened.approval.decide(&card.id, false), "the refusal lands");

        let answer = asking.await.expect("the question is answered");
        assert_eq!(answer.is_error, Some(false));
        assert_eq!(body(&answer)["behavior"], "deny", "{:?}", answer.content);
    }

    #[tokio::test]
    async fn an_unreadable_question_is_denied_even_when_the_chat_auto_approves() {
        let opened = open(ApprovalPolicy::auto()).await;
        let turn = Turn::find(&opened.token).expect("the turn");

        for unreadable in [
            json!({"tool_name": "Bash"}).to_string(),
            json!({"input": {"command": "ls"}}).to_string(),
            "not json".to_string(),
        ] {
            let answer = turn.run(Toolset::PERMISSION, &unreadable).await;
            assert_eq!(
                body(&answer)["behavior"],
                "deny",
                "{unreadable}: {:?}",
                answer.content
            );
        }
    }

    #[tokio::test]
    async fn a_decided_card_is_settled_in_the_console() {
        for approved in [true, false] {
            let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
            let turn = Turn::find(&opened.token).expect("the turn");

            let asking = tokio::spawn(async move { turn.run(Toolset::PERMISSION, &asked()).await });

            let card = next_card(&mut opened.events).await;
            assert!(opened.approval.decide(&card.id, approved));
            asking.await.expect("the question is answered");

            let settled = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    match opened
                        .events
                        .recv()
                        .await
                        .expect("the console hears the turn")
                    {
                        AgentEvent::ToolCallCompleted { id, success, .. } => break (id, success),
                        _ => continue,
                    }
                }
            })
            .await
            .expect("the decided card is settled");
            assert_eq!(
                settled,
                (card.id, approved),
                "the card stops waiting once the reader has answered it"
            );
        }
    }

    #[tokio::test]
    async fn a_question_nobody_is_listening_to_is_denied_rather_than_held() {
        let opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        drop(opened.events);

        let answer = turn.run(Toolset::PERMISSION, &asked()).await;

        assert_eq!(body(&answer)["behavior"], "deny", "{:?}", answer.content);
    }

    #[tokio::test]
    async fn asking_spends_none_of_the_turns_budget() {
        let opened = open(ApprovalPolicy::auto()).await;
        let turn = Turn::find(&opened.token).expect("the turn");

        for _ in 0..3 {
            turn.run(Toolset::PERMISSION, &asked()).await;
        }

        assert_eq!(
            turn.made.load(Ordering::Relaxed),
            0,
            "asking whether a call may run is not the call"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{drain, next_card, open, scope, tools, wait_for_card, write};
    use super::*;
    use crate::agent::approval::refusal::{APPROVAL_CANCELLED, APPROVAL_TIMED_OUT, DENIED};
    use crate::agent::wait::{self, WAIT_FOR};
    use crate::agent::{ASK_USER, ApprovalGate, ApprovalPolicy};
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::sync::mpsc::unbounded_channel;
    use zone_core::llm::cli::TURN_TIMEOUT;
    use zone_core::tools::Session;
    use zone_core::tools::job::{JobCommand, Jobs};

    #[tokio::test]
    async fn what_the_agent_wrote_is_yielded_before_a_call_that_arrived_with_it() {
        let (sender, mut calls) = unbounded_channel();
        sender
            .send(AgentEvent::ToolCallStarted {
                id: "call_1".to_string(),
                name: "list_chats".to_string(),
                arguments: "{}".to_string(),
            })
            .unwrap();
        let written = futures::stream::iter(vec![AgentEvent::Chunk("Let me look. ".to_string())]);

        let order: Vec<&str> = merged(written, &mut calls)
            .map(|event| match event {
                AgentEvent::Chunk(_) => "text",
                AgentEvent::ToolCallStarted { .. } => "call",
                _ => "other",
            })
            .collect()
            .await;

        assert_eq!(
            order,
            ["text", "call"],
            "a call placed ahead of the text before it is drawn above that text"
        );
    }

    #[tokio::test]
    async fn a_call_that_arrived_before_the_round_failed_is_yielded_before_the_failure() {
        let (sender, mut calls) = unbounded_channel();
        sender
            .send(AgentEvent::ToolCallStarted {
                id: "call_1".to_string(),
                name: "list_chats".to_string(),
                arguments: "{}".to_string(),
            })
            .unwrap();
        let failed = futures::stream::iter(vec![AgentEvent::Failed("signed out".to_string())]);

        let order: Vec<&str> = merged(failed, &mut calls)
            .map(|event| match event {
                AgentEvent::Failed(_) => "failed",
                AgentEvent::ToolCallStarted { .. } => "call",
                _ => "other",
            })
            .collect()
            .await;

        assert_eq!(
            order,
            ["call", "failed"],
            "a turn stops reading at a failure, so a call queued behind it lands in the next login's round"
        );
    }

    #[tokio::test]
    async fn a_call_that_arrived_before_the_round_was_cut_short_is_yielded_before_it() {
        let (sender, mut calls) = unbounded_channel();
        sender
            .send(AgentEvent::ToolCallStarted {
                id: "call_1".to_string(),
                name: "list_chats".to_string(),
                arguments: "{}".to_string(),
            })
            .unwrap();
        let cut_short = futures::stream::iter(vec![AgentEvent::CutShort]);

        let order: Vec<&str> = merged(cut_short, &mut calls)
            .map(|event| match event {
                AgentEvent::CutShort => "cut short",
                AgentEvent::ToolCallStarted { .. } => "call",
                _ => "other",
            })
            .collect()
            .await;

        assert_eq!(
            order,
            ["call", "cut short"],
            "a task stops reading where its round was cut short, so a call queued behind it is never recorded"
        );
    }

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

        tokio::time::advance(TURN_TIMEOUT * 4).await;

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

    /// A task's budget is unlimited, so without this a coding agent repeating
    /// a call that keeps failing would run until the attempt's deadline.
    #[tokio::test]
    async fn a_repeated_failure_is_refused_and_a_second_stops_the_turn() {
        let directory = tempfile::tempdir().expect("tempdir");
        let chat = Uuid::new_v4();
        let (sender, mut events) = unbounded_channel();
        let lease =
            Turn::new(scope(chat), tools(chat).await, sender).open("http://127.0.0.1:8080/mcp");
        let turn = Turn::find(lease.toolset().token.expose()).expect("the turn");
        let missing =
            json!({ "path": directory.path().join("missing.txt").to_string_lossy() }).to_string();

        let failed = turn.run("read_file", &missing).await;
        assert_eq!(failed.is_error, Some(true), "{failed:?}");
        assert_eq!(drain(&mut events), ["started", "completed"]);

        let warned = turn.run("read_file", &missing).await;
        assert!(
            format!("{:?}", warned.content).contains(REPEATED_CALL),
            "{:?}",
            warned.content
        );
        assert!(drain(&mut events).is_empty(), "a refused repeat ran");

        let stopped = turn.run("read_file", &missing).await;
        assert!(
            format!("{:?}", stopped.content).contains(STALLED),
            "{:?}",
            stopped.content
        );
        let after = directory.path().join("after.txt");
        let refused = turn.run("write_file", &write(&after).to_string()).await;
        assert!(
            format!("{:?}", refused.content).contains(STALLED),
            "{:?}",
            refused.content
        );
        assert!(!after.exists(), "a call after the turn stalled ran");
        let told: Vec<AgentEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(
            matches!(told.as_slice(), [AgentEvent::Finalizing(reason)] if reason == REPEATED),
            "the console is told once that the turn stalled, and shown no call that never \
             ran: {told:?}"
        );
    }

    /// An agent that keeps its own shell can create what a failed call was
    /// missing without zone seeing it, so the retry must run.
    #[tokio::test]
    async fn a_turn_whose_agent_keeps_its_own_tools_retries_a_failed_call() {
        let directory = tempfile::tempdir().expect("tempdir");
        let chat = Uuid::new_v4();
        let (sender, _events) = unbounded_channel();
        let lease = Turn::new(
            Scope {
                builtin: BuiltinTools::Granted,
                ..scope(chat)
            },
            tools(chat).await,
            sender,
        )
        .open("http://127.0.0.1:8080/mcp");
        let turn = Turn::find(lease.toolset().token.expose()).expect("the turn");
        let path = directory.path().join("made-elsewhere.txt");
        let read = json!({ "path": path.to_string_lossy() }).to_string();

        let failed = turn.run("read_file", &read).await;
        assert_eq!(failed.is_error, Some(true), "{failed:?}");
        std::fs::write(&path, "made by the agent's own shell").expect("write");

        let retried = turn.run("read_file", &read).await;
        assert_eq!(retried.is_error, Some(false), "{retried:?}");
    }

    #[tokio::test]
    async fn a_failure_followed_by_a_change_may_be_retried() {
        let directory = tempfile::tempdir().expect("tempdir");
        let chat = Uuid::new_v4();
        let (sender, _events) = unbounded_channel();
        let lease =
            Turn::new(scope(chat), tools(chat).await, sender).open("http://127.0.0.1:8080/mcp");
        let turn = Turn::find(lease.toolset().token.expose()).expect("the turn");
        let path = directory.path().join("later.txt");
        let read = json!({ "path": path.to_string_lossy() }).to_string();

        let failed = turn.run("read_file", &read).await;
        assert_eq!(failed.is_error, Some(true), "{failed:?}");
        let written = turn.run("write_file", &write(&path).to_string()).await;
        assert_eq!(written.is_error, Some(false), "{written:?}");

        let retried = turn.run("read_file", &read).await;
        assert_eq!(retried.is_error, Some(false), "{retried:?}");
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

    /// Every call completed so far among `events`, with what refused it.
    fn completed(events: &mut UnboundedReceiver<AgentEvent>) -> Vec<(String, Option<Refusal>)> {
        let mut completed = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let AgentEvent::ToolCallCompleted { name, refusal, .. } = event {
                completed.push((name, refusal));
            }
        }
        completed
    }

    /// The completion zone records for a refused call, the agent's own or zone's, names what
    /// refused it, so the console labels it from that rather than from its words.
    #[tokio::test]
    async fn a_refused_call_is_recorded_with_what_refused_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("refused.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        let arguments = write(&path).to_string();
        let asking = turn.clone();

        let call = tokio::spawn(async move { turn.run("write_file", &arguments).await });
        let card = next_card(&mut opened.events).await;
        assert!(opened.approval.decide(&card.id, false), "the denial lands");
        call.await.expect("the call task");
        let mut refusals = completed(&mut opened.events);
        let asked = json!({"tool_name": "Bash", "input": {"command": "ls"}}).to_string();
        let permission = tokio::spawn(async move { asking.run(Toolset::PERMISSION, &asked).await });
        let card = next_card(&mut opened.events).await;
        assert!(opened.approval.decide(&card.id, false), "the refusal lands");
        permission.await.expect("the question is answered");
        refusals.extend(completed(&mut opened.events));

        assert_eq!(
            refusals,
            [
                ("write_file".to_string(), Some(Refusal::Denied)),
                ("Bash".to_string(), Some(Refusal::Denied)),
            ]
        );
    }

    #[tokio::test]
    async fn a_call_whose_turn_stops_while_it_waits_is_told_it_was_cancelled() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("cancelled.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        let arguments = write(&path).to_string();

        let call = tokio::spawn(async move { turn.run("write_file", &arguments).await });
        wait_for_card(&mut opened.events).await;
        opened.approval.deny_all();

        let result = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .expect("a cancelled call returns at once")
            .expect("the call task");
        let told = format!("{:?}", result.content);
        assert_eq!(result.is_error, Some(true), "{result:?}");
        assert!(told.contains(APPROVAL_CANCELLED), "{told}");
        assert!(!told.contains(DENIED), "nobody denied it: {told}");
        assert!(!path.exists());
        assert_eq!(
            completed(&mut opened.events),
            [("write_file".to_string(), Some(Refusal::Cancelled))]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_nobody_answers_is_told_its_approval_timed_out() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("unanswered.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let turn = Turn::find(&opened.token).expect("the turn");
        let arguments = write(&path).to_string();

        let call = tokio::spawn(async move { turn.run("write_file", &arguments).await });
        wait_for_card(&mut opened.events).await;

        let result = call.await.expect("the call task");
        let told = format!("{:?}", result.content);
        assert_eq!(result.is_error, Some(true), "{result:?}");
        assert!(told.contains(APPROVAL_TIMED_OUT), "{told}");
        assert!(!told.contains(DENIED), "nobody denied it: {told}");
        assert!(!path.exists());
        assert_eq!(
            completed(&mut opened.events),
            [("write_file".to_string(), Some(Refusal::TimedOut))]
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

        let told = format!("{:?}", result.content);
        assert_eq!(result.is_error, Some(true), "{result:?}");
        assert!(told.contains(APPROVAL_CANCELLED), "{told}");
        assert!(!told.contains(DENIED), "nobody denied it: {told}");
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
        assert_eq!(
            detail("read_file", "\n\nfirst line\nsecond line"),
            "first line"
        );
        let long = detail("read_file", &"x".repeat(DETAIL_CHARS * 2));
        assert_eq!(long.chars().count(), DETAIL_CHARS + 1, "{long}");
        assert!(long.ends_with('\u{2026}'));
    }

    #[test]
    fn a_detail_shows_a_command_output_rather_than_its_stream_label() {
        assert_eq!(detail("run_shell", "stdout:\nhello\n"), "hello");
        assert_eq!(
            detail("run_command", "stdout:\n\n\nstderr:\nwarning"),
            "warning"
        );
        assert_eq!(detail("run_shell", "stdout:\n\n"), "");
        assert_eq!(detail("read_file", "stdout:\nreal"), "stdout:");
    }
}
