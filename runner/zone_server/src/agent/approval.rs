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
use zone_core::tools::Tier;

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

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
    pub fn expect_decision(&self, id: &str) -> Option<oneshot::Receiver<bool>> {
        if self.is_auto() {
            return None;
        }
        Some(self.gate.begin(id))
    }

    /// Await a decision registered earlier by [`Self::expect_decision`].
    pub async fn awaited_decision(
        &self,
        id: &str,
        pending: Option<oneshot::Receiver<bool>>,
    ) -> bool {
        let Some(receiver) = pending else {
            return true;
        };
        if self.is_auto() {
            let _ = self.decide(id, true);
            return true;
        }
        match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(Ok(approved)) => approved || self.is_auto(),
            _ => {
                self.gate.forget(id);
                self.is_auto()
            }
        }
    }

    pub async fn await_decision(&self, id: &str) -> bool {
        let pending = self.expect_decision(id);
        self.awaited_decision(id, pending).await
    }

    pub fn decide(&self, id: &str, approved: bool) -> bool {
        self.gate.decide(id, approved)
    }

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
    pending: Arc<DashMap<String, oneshot::Sender<bool>>>,
}

impl ApprovalGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pending, &other.pending)
    }

    /// Block until the user decides, or time out as a denial.
    pub async fn await_decision(&self, id: &str) -> bool {
        self.await_decision_with_timeout(id, APPROVAL_TIMEOUT).await
    }

    fn forget(&self, id: &str) {
        self.pending.remove(id);
    }

    fn begin(&self, id: &str) -> oneshot::Receiver<bool> {
        let (sender, receiver) = oneshot::channel();
        self.pending.insert(id.to_string(), sender);
        receiver
    }

    async fn await_decision_with_timeout(&self, id: &str, timeout: Duration) -> bool {
        let receiver = self.begin(id);
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(approved)) => approved,
            _ => {
                self.pending.remove(id);
                false
            }
        }
    }

    /// Resolve a waiting call. Returns whether a waiter existed.
    pub fn decide(&self, id: &str, approved: bool) -> bool {
        self.pending
            .remove(id)
            .map(|(_, sender)| sender.send(approved).is_ok())
            .unwrap_or(false)
    }

    pub fn deny_all(&self) {
        self.resolve_all(false);
    }

    pub fn approve_all(&self) {
        self.resolve_all(true);
    }

    fn resolve_all(&self, approved: bool) {
        for id in self
            .pending
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>()
        {
            let _ = self.decide(&id, approved);
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
        assert!(wait.await.unwrap());
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
        assert!(wait.await.unwrap());
        assert!(policy.is_auto());
    }

    #[tokio::test]
    async fn live_auto_skips_later_waits() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        policy.set_auto(true);
        assert!(policy.await_decision("already_on").await);
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
        assert!(!wait.await.unwrap());
    }

    #[tokio::test]
    async fn decide_twice_is_only_a_decision_once() {
        let gate = ApprovalGate::new();
        let clone = gate.clone();
        let wait = tokio::spawn(async move { clone.await_decision("call_1").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_1", true));
        assert!(!gate.decide("call_1", false));
        assert!(wait.await.unwrap());
    }

    #[tokio::test]
    async fn deny_all_rejects_every_waiter() {
        let gate = ApprovalGate::new();
        let first = gate.clone();
        let second = gate.clone();
        let wait_a = tokio::spawn(async move { first.await_decision("a").await });
        let wait_b = tokio::spawn(async move { second.await_decision("b").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        gate.deny_all();
        assert!(!wait_a.await.unwrap());
        assert!(!wait_b.await.unwrap());
        assert!(!gate.decide("a", true));
    }

    #[tokio::test]
    async fn timed_out_waiters_are_denied() {
        let gate = ApprovalGate::new();
        let denied = gate
            .await_decision_with_timeout("late", Duration::from_millis(15))
            .await;
        assert!(!denied);
        assert!(!gate.decide("late", true));
    }
}
