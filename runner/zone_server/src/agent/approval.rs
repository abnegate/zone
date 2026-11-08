//! Confirm a tool call with the user before it runs in chat.
//!
//! Which calls wait is the tool's own declaration: anything from
//! [`zone_core::tools::CONFIRMED_FROM`] up, which is host writes and commands
//! and everything that leaves the workspace. Tasks auto-approve: they run
//! unattended in a sandboxed cwd, so nobody is there to answer.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;
use zone_core::tools::{APPROVAL_CANCELLED, APPROVAL_TIMED_OUT, DENIED, Tier};

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// How a call put to the reader came out. Only the reader's own answer is a
/// denial: a turn that stopped, or a wait nobody answered, is told so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Denied,
    Cancelled,
    TimedOut,
}

impl Decision {
    fn answered(approved: bool) -> Self {
        if approved {
            Self::Approved
        } else {
            Self::Denied
        }
    }

    pub fn approved(self) -> bool {
        self == Self::Approved
    }

    /// What a call that may not run is told in place of running.
    pub fn refusal(self) -> Option<&'static str> {
        match self {
            Self::Approved => None,
            Self::Denied => Some(DENIED),
            Self::Cancelled => Some(APPROVAL_CANCELLED),
            Self::TimedOut => Some(APPROVAL_TIMED_OUT),
        }
    }
}

static LIVE: Lazy<DashMap<Uuid, ApprovalPolicy>> = Lazy::new(DashMap::new);

/// How a loop should treat mutating file and shell tools.
///
/// Auto is a live flag so turning auto-approve on mid-turn can release waiters
/// and skip later confirmations in the same generation.
#[derive(Clone)]
pub struct ApprovalPolicy {
    auto: Arc<AtomicBool>,
    gate: ApprovalGate,
}

impl ApprovalPolicy {
    pub fn auto() -> Self {
        Self {
            auto: Arc::new(AtomicBool::new(true)),
            gate: ApprovalGate::new(),
        }
    }

    pub fn required(gate: ApprovalGate) -> Self {
        Self {
            auto: Arc::new(AtomicBool::new(false)),
            gate,
        }
    }

    pub fn is_auto(&self) -> bool {
        self.auto.load(Ordering::Acquire)
    }

    /// Whether a call at this tier is put to the user before it runs.
    ///
    /// The tool declares its own tier, so a tool added later is gated by what
    /// it does rather than by whether somebody remembered to name it here.
    pub fn confirms(&self, tier: Tier) -> bool {
        !self.is_auto() && tier.confirmed()
    }

    pub fn set_auto(&self, auto: bool) {
        self.auto.store(auto, Ordering::Release);
        if auto {
            self.gate.approve_all();
        }
    }

    pub fn same_as(&self, other: &Self) -> bool {
        self.gate.same_as(&other.gate)
    }

    /// Register the waiter before the request reaches the client.
    ///
    /// The decision can come back before the caller has even finished emitting
    /// the request, and a decision with nothing to resolve is refused to the
    /// client as a closed approval, so the waiter has to exist first.
    pub fn expect_decision(&self, id: &str) -> Option<oneshot::Receiver<Decision>> {
        if self.is_auto() {
            return None;
        }
        Some(self.gate.begin(id))
    }

    /// Await a decision registered earlier by [`Self::expect_decision`].
    pub async fn awaited_decision(
        &self,
        id: &str,
        pending: Option<oneshot::Receiver<Decision>>,
    ) -> Decision {
        let Some(receiver) = pending else {
            return Decision::Approved;
        };
        if self.is_auto() {
            let _ = self.decide(id, true);
            return Decision::Approved;
        }
        let decision = self.gate.settle(id, APPROVAL_TIMEOUT, receiver).await;
        if self.is_auto() {
            Decision::Approved
        } else {
            decision
        }
    }

    pub async fn await_decision(&self, id: &str) -> Decision {
        let pending = self.expect_decision(id);
        self.awaited_decision(id, pending).await
    }

    pub fn decide(&self, id: &str, approved: bool) -> bool {
        self.gate.decide(id, approved)
    }

    /// Refuse every waiting call because its turn stopped, not because the
    /// reader said no.
    pub fn deny_all(&self) {
        self.gate.deny_all();
    }

    pub fn register(chat_id: Uuid, policy: Self) {
        LIVE.insert(chat_id, policy);
    }

    pub fn unregister(chat_id: Uuid, policy: &Self) {
        if LIVE
            .get(&chat_id)
            .is_some_and(|entry| entry.same_as(policy))
        {
            LIVE.remove(&chat_id);
        }
    }

    pub fn decide_chat(chat_id: Uuid, id: &str, approved: bool) -> bool {
        LIVE.get(&chat_id)
            .is_some_and(|policy| policy.decide(id, approved))
    }

    pub fn deny_chat(chat_id: Uuid) {
        if let Some(policy) = LIVE.get(&chat_id) {
            policy.deny_all();
        }
    }

    pub fn set_chat_auto(chat_id: Uuid, auto: bool) {
        if let Some(policy) = LIVE.get(&chat_id) {
            policy.set_auto(auto);
        }
    }
}

/// In-flight approval waiters for one generation.
#[derive(Clone, Default)]
pub struct ApprovalGate {
    pending: Arc<DashMap<String, oneshot::Sender<Decision>>>,
}

impl ApprovalGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pending, &other.pending)
    }

    /// Block until the user decides, the turn stops, or the wait runs out.
    pub async fn await_decision(&self, id: &str) -> Decision {
        self.await_decision_with_timeout(id, APPROVAL_TIMEOUT).await
    }

    fn begin(&self, id: &str) -> oneshot::Receiver<Decision> {
        let (sender, receiver) = oneshot::channel();
        self.pending.insert(id.to_string(), sender);
        receiver
    }

    async fn await_decision_with_timeout(&self, id: &str, timeout: Duration) -> Decision {
        let receiver = self.begin(id);
        self.settle(id, timeout, receiver).await
    }

    /// A waiter whose sender went with its generation was cancelled with it.
    async fn settle(
        &self,
        id: &str,
        timeout: Duration,
        receiver: oneshot::Receiver<Decision>,
    ) -> Decision {
        let settled = tokio::time::timeout(timeout, receiver).await;
        if !matches!(settled, Ok(Ok(_))) {
            self.pending.remove(id);
        }
        match settled {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => Decision::Cancelled,
            Err(_) => Decision::TimedOut,
        }
    }

    /// Resolve a waiting call with the reader's answer. Returns whether a
    /// waiter existed.
    pub fn decide(&self, id: &str, approved: bool) -> bool {
        self.resolve(id, Decision::answered(approved))
    }

    fn resolve(&self, id: &str, decision: Decision) -> bool {
        self.pending
            .remove(id)
            .map(|(_, sender)| sender.send(decision).is_ok())
            .unwrap_or(false)
    }

    /// Refuse every waiting call because its turn stopped.
    pub fn deny_all(&self) {
        self.resolve_all(Decision::Cancelled);
    }

    pub fn approve_all(&self) {
        self.resolve_all(Decision::Approved);
    }

    fn resolve_all(&self, decision: Decision) {
        for id in self
            .pending
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>()
        {
            let _ = self.resolve(&id, decision);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn decide_unblocks_the_waiter() {
        let gate = ApprovalGate::new();
        let clone = gate.clone();
        let wait = tokio::spawn(async move { clone.await_decision("call_1").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_1", true));
        assert_eq!(wait.await.unwrap(), Decision::Approved);
    }

    #[test]
    fn unknown_ids_are_not_decisions() {
        assert!(!ApprovalGate::new().decide("missing", true));
    }

    #[test]
    fn host_and_outward_calls_wait_and_nothing_below_them_does() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        assert!(policy.confirms(Tier::Host));
        assert!(policy.confirms(Tier::Outward));
        assert!(!policy.confirms(Tier::Write));
        assert!(!policy.confirms(Tier::Read));
    }

    /// Auto-approve is the surface saying nobody is waiting to answer, so it
    /// has to release every tier, including the outward ones.
    #[test]
    fn auto_approve_waits_for_nothing() {
        let policy = ApprovalPolicy::auto();
        for tier in [Tier::Read, Tier::Write, Tier::Host, Tier::Outward] {
            assert!(!policy.confirms(tier), "{tier:?}");
        }
    }

    /// The tools whose tier decides this, named once so a retiering that
    /// silently drops a confirmation fails here rather than in production.
    #[test]
    fn the_catalog_tiers_the_calls_a_reader_has_to_see_first() {
        use zone_core::tools::{ApplyPatchTool, ReadFileTool, RunShellTool, Tool, WriteFileTool};

        assert_eq!(WriteFileTool.tier(), Tier::Host);
        assert_eq!(ApplyPatchTool.tier(), Tier::Host);
        assert_eq!(RunShellTool.tier(), Tier::Host);
        assert_eq!(ReadFileTool.tier(), Tier::Read);
    }

    #[test]
    fn auto_policy_skips_the_gate() {
        assert!(ApprovalPolicy::auto().is_auto());
        assert!(!ApprovalPolicy::required(ApprovalGate::new()).is_auto());
    }

    #[tokio::test]
    async fn enabling_auto_releases_waiters_as_approved() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        let waiting = policy.clone();
        let wait = tokio::spawn(async move { waiting.await_decision("call_live").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        policy.set_auto(true);
        assert_eq!(wait.await.unwrap(), Decision::Approved);
        assert!(policy.is_auto());
    }

    #[tokio::test]
    async fn live_auto_skips_later_waits() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        policy.set_auto(true);
        assert_eq!(
            policy.await_decision("already_on").await,
            Decision::Approved
        );
    }

    #[test]
    fn set_chat_auto_reaches_the_registered_generation() {
        let chat = Uuid::new_v4();
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        ApprovalPolicy::register(chat, policy.clone());
        ApprovalPolicy::set_chat_auto(chat, true);
        assert!(policy.is_auto());
        ApprovalPolicy::unregister(chat, &policy);
    }

    #[test]
    fn same_as_is_pointer_identity() {
        let gate = ApprovalGate::new();
        assert!(gate.same_as(&gate));
        assert!(gate.same_as(&gate.clone()));
        assert!(!gate.same_as(&ApprovalGate::new()));
    }

    #[tokio::test]
    async fn deny_unblocks_the_waiter() {
        let gate = ApprovalGate::new();
        let clone = gate.clone();
        let wait = tokio::spawn(async move { clone.await_decision("call_deny").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_deny", false));
        assert_eq!(wait.await.unwrap(), Decision::Denied);
    }

    #[tokio::test]
    async fn decide_twice_is_only_a_decision_once() {
        let gate = ApprovalGate::new();
        let clone = gate.clone();
        let wait = tokio::spawn(async move { clone.await_decision("call_1").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_1", true));
        assert!(!gate.decide("call_1", false));
        assert_eq!(wait.await.unwrap(), Decision::Approved);
    }

    #[tokio::test]
    async fn deny_all_cancels_every_waiter_rather_than_denying_it() {
        let gate = ApprovalGate::new();
        let first = gate.clone();
        let second = gate.clone();
        let wait_a = tokio::spawn(async move { first.await_decision("a").await });
        let wait_b = tokio::spawn(async move { second.await_decision("b").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        gate.deny_all();
        assert_eq!(wait_a.await.unwrap(), Decision::Cancelled);
        assert_eq!(wait_b.await.unwrap(), Decision::Cancelled);
        assert!(!gate.decide("a", true));
    }

    #[tokio::test]
    async fn a_waiter_nobody_answers_times_out_rather_than_being_denied() {
        let gate = ApprovalGate::new();
        let decision = gate
            .await_decision_with_timeout("late", Duration::from_millis(15))
            .await;
        assert_eq!(decision, Decision::TimedOut);
        assert!(!gate.decide("late", true));
    }

    #[tokio::test]
    async fn a_waiter_whose_generation_went_away_is_cancelled() {
        let gate = ApprovalGate::new();
        let receiver = gate.begin("orphan");
        gate.pending.clear();
        let decision = gate
            .settle("orphan", Duration::from_secs(5), receiver)
            .await;
        assert_eq!(decision, Decision::Cancelled);
    }

    #[test]
    fn each_refusal_tells_the_call_why_it_did_not_run() {
        assert_eq!(Decision::Approved.refusal(), None);
        assert_eq!(Decision::Denied.refusal(), Some(DENIED));
        assert_eq!(Decision::Cancelled.refusal(), Some(APPROVAL_CANCELLED));
        assert_eq!(Decision::TimedOut.refusal(), Some(APPROVAL_TIMED_OUT));
    }
}
