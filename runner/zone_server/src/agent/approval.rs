//! Confirm mutating file and shell tool calls before they run in chat.
//!
//! Chat tools execute inside the server container with process permissions.
//! Tasks auto-approve: they already run in a sandboxed cwd.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// How a loop should treat mutating file and shell tools.
#[derive(Clone)]
pub enum ApprovalPolicy {
    /// Run immediately (task worker, tests, CLI).
    Auto,
    /// Wait for the user to confirm each write or shell call.
    Required(ApprovalGate),
}

impl ApprovalPolicy {
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
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

    async fn await_decision_with_timeout(&self, id: &str, timeout: Duration) -> bool {
        let (sender, receiver) = oneshot::channel();
        self.pending.insert(id.to_string(), sender);
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
        for id in self
            .pending
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>()
        {
            let _ = self.decide(&id, false);
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
        assert!(ApprovalPolicy::Auto.is_auto());
        assert!(!ApprovalPolicy::Required(ApprovalGate::new()).is_auto());
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
