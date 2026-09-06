//! Confirm mutating file and shell tool calls before they run in chat.
//!
//! Chat tools execute inside the server container with process permissions.
//! Tasks auto-approve: they already run in a sandboxed cwd.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

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

    pub fn set_auto(&self, auto: bool) {
        self.auto.store(auto, Ordering::Release);
        if auto {
            self.gate.approve_all();
        }
    }

    pub fn same_as(&self, other: &Self) -> bool {
        self.gate.same_as(&other.gate)
    }

    pub async fn await_decision(&self, id: &str) -> bool {
        if self.is_auto() {
            return true;
        }
        let receiver = self.gate.begin(id);
        if self.is_auto() {
            let _ = self.decide(id, true);
            return true;
        }
        match tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await {
            Ok(Ok(approved)) => approved || self.is_auto(),
            _ => self.is_auto(),
        }
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

/// File and shell tools that change the host or run a command.
pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "apply_patch" | "run_command" | "run_shell"
    )
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
    fn file_and_shell_tools_require_approval() {
        assert!(requires_approval("write_file"));
        assert!(requires_approval("apply_patch"));
        assert!(requires_approval("run_command"));
        assert!(requires_approval("run_shell"));
        assert!(!requires_approval("read_file"));
        assert!(!requires_approval("generate_image"));
        assert!(!requires_approval("create_pull_request"));
        assert!(!requires_approval("comment_on_issue"));
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
