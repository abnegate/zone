//! Confirm a tool call with the user before it runs in chat.
//!
//! Which calls wait is the tool's own declaration: anything from
//! [`abnegate_agent::tool::CONFIRMED_FROM`] up, which is host writes and commands
//! and everything that leaves the workspace. Tasks auto-approve: they run
//! unattended in a sandboxed cwd, so nobody is there to answer.
//!
//! An organization's ceiling caps what auto-approve may release: a call above
//! it is put to the reader even when the chat auto-approves, and withheld
//! outright from a run nobody attends.

use abnegate_agent::tool::Tier;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

pub mod refusal;

pub use refusal::Refusal;

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// How a call put to the reader came out. Only the reader's own answer is a
/// denial: a turn that stopped, or a wait nobody answered, is told so.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Denied,
    Cancelled,
    TimedOut,
    Withheld,
}

impl Decision {
    fn answered(approved: bool) -> Self {
        if approved {
            Self::Approved
        } else {
            Self::Denied
        }
    }

    /// Why a call that may not run did not.
    pub fn refusal(self) -> Option<Refusal> {
        match self {
            Self::Approved => None,
            Self::Denied => Some(Refusal::Denied),
            Self::Cancelled => Some(Refusal::Cancelled),
            Self::TimedOut => Some(Refusal::TimedOut),
            Self::Withheld => Some(Refusal::Policy),
        }
    }
}

static LIVE: Lazy<DashMap<Uuid, ApprovalPolicy>> = Lazy::new(DashMap::new);

/// Told the name of each call a policy withholds.
pub type Withholding = dyn Fn(&str) + Send + Sync;

/// How a loop should treat mutating file and shell tools.
///
/// Auto is a live flag so turning auto-approve on mid-turn can release waiters
/// and skip later confirmations in the same generation. The ceiling and the
/// withholding hook are shared the same way, so every clone answers to the one
/// set last.
#[derive(Clone)]
pub struct ApprovalPolicy {
    auto: Arc<AtomicBool>,
    ceiling: Arc<RwLock<Option<Tier>>>,
    withholding: Arc<RwLock<Option<Arc<Withholding>>>>,
    unattended: bool,
    gate: ApprovalGate,
}

impl ApprovalPolicy {
    fn new(auto: bool, unattended: bool, gate: ApprovalGate) -> Self {
        Self {
            auto: Arc::new(AtomicBool::new(auto)),
            ceiling: Arc::new(RwLock::new(None)),
            withholding: Arc::new(RwLock::new(None)),
            unattended,
            gate,
        }
    }

    pub fn auto() -> Self {
        Self::new(true, false, ApprovalGate::new())
    }

    /// A run nobody attends: everything is approved at once, except a call
    /// above the ceiling, which is withheld because nobody can approve it.
    pub fn unattended() -> Self {
        Self::new(true, true, ApprovalGate::new())
    }

    pub fn required(gate: ApprovalGate) -> Self {
        Self::new(false, false, gate)
    }

    pub fn is_auto(&self) -> bool {
        self.auto.load(Ordering::Acquire)
    }

    pub fn ceiling(&self) -> Option<Tier> {
        *self.ceiling.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_ceiling(&self, ceiling: Option<Tier>) {
        *self.ceiling.write().unwrap_or_else(PoisonError::into_inner) = ceiling;
        if self.is_auto() {
            self.gate.approve_within(ceiling);
        }
    }

    pub fn with_ceiling(self, ceiling: Option<Tier>) -> Self {
        self.set_ceiling(ceiling);
        self
    }

    pub fn set_recorder(&self, withholding: Arc<Withholding>) {
        *self
            .withholding
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(withholding);
    }

    fn releases(&self, tier: Tier) -> bool {
        self.is_auto() && within(tier, self.ceiling())
    }

    /// Whether a call at this tier is put to the user before it runs.
    ///
    /// The tool declares its own tier, so a tool added later is gated by what
    /// it does rather than by whether somebody remembered to name it here.
    pub fn confirms(&self, tier: Tier) -> bool {
        tier.confirmed() && !self.releases(tier)
    }

    /// Whether the call is refused without being asked, because it needs an
    /// approval nobody attending this run can give.
    pub fn withheld(&self, name: &str, tier: Tier) -> bool {
        let withheld = self.withholds(tier);
        if withheld {
            let withholding = self
                .withholding
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(withholding) = withholding {
                withholding(name);
            }
        }
        withheld
    }

    fn withholds(&self, tier: Tier) -> bool {
        self.unattended && self.confirms(tier)
    }

    /// Turning auto on releases only the waiters at or below the ceiling; a
    /// call above it still needs the reader.
    pub fn set_auto(&self, auto: bool) {
        self.auto.store(auto, Ordering::Release);
        if auto {
            self.gate.approve_within(self.ceiling());
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
    ///
    /// A run nobody attends registers nothing, since nobody would answer.
    pub fn expect_decision(&self, id: &str, tier: Tier) -> Option<oneshot::Receiver<Decision>> {
        if self.unattended || !self.confirms(tier) {
            return None;
        }
        Some(self.gate.begin(id, tier))
    }

    /// Await a decision registered earlier by [`Self::expect_decision`].
    ///
    /// With nothing registered, a call this policy withholds is refused, so a
    /// caller that skipped [`Self::withheld`] still never runs it.
    pub async fn awaited_decision(
        &self,
        id: &str,
        tier: Tier,
        pending: Option<oneshot::Receiver<Decision>>,
    ) -> Decision {
        let Some(receiver) = pending else {
            return match self.withholds(tier) {
                true => Decision::Withheld,
                false => Decision::Approved,
            };
        };
        if self.releases(tier) && self.decide(id, true) {
            return Decision::Approved;
        }
        match self.gate.settle(id, APPROVAL_TIMEOUT, receiver).await {
            Decision::TimedOut if self.releases(tier) => Decision::Approved,
            decision => decision,
        }
    }

    pub async fn await_decision(&self, id: &str, tier: Tier) -> Decision {
        let pending = self.expect_decision(id, tier);
        self.awaited_decision(id, tier, pending).await
    }

    pub fn decide(&self, id: &str, approved: bool) -> bool {
        self.gate.decide(id, approved)
    }

    /// Let go of a waiter whose request never reached the reader.
    pub fn withdraw(&self, id: &str) {
        self.gate.pending.remove(id);
    }

    /// Refuse every waiting call because its turn stopped, not because the
    /// reader said no.
    pub fn deny_all(&self) {
        self.gate.cancel_all();
    }

    /// Whether a call is parked on a decision nobody has made yet.
    pub fn waiting(&self) -> bool {
        self.gate.waiting()
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

    /// Whether this chat's live turn is waiting for the reader to approve a
    /// call. A chat with no registered policy has no turn in flight, so it is
    /// waiting for nothing.
    pub fn chat_waiting(chat_id: Uuid) -> bool {
        LIVE.get(&chat_id).is_some_and(|policy| policy.waiting())
    }

    pub fn set_chat_auto(chat_id: Uuid, auto: bool) {
        if let Some(policy) = LIVE.get(&chat_id) {
            policy.set_auto(auto);
        }
    }
}

fn within(tier: Tier, ceiling: Option<Tier>) -> bool {
    ceiling.is_none_or(|ceiling| tier <= ceiling)
}

struct Waiter {
    sender: oneshot::Sender<Decision>,
    tier: Tier,
}

/// In-flight approval waiters for one generation.
#[derive(Clone, Default)]
pub struct ApprovalGate {
    pending: Arc<DashMap<String, Waiter>>,
}

impl ApprovalGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pending, &other.pending)
    }

    /// Block until the user decides, the turn stops, or the wait runs out.
    pub async fn await_decision(&self, id: &str, tier: Tier) -> Decision {
        self.await_decision_with_timeout(id, tier, APPROVAL_TIMEOUT)
            .await
    }

    /// Whether anything is still parked here.
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    fn begin(&self, id: &str, tier: Tier) -> oneshot::Receiver<Decision> {
        let (sender, receiver) = oneshot::channel();
        self.pending.insert(id.to_string(), Waiter { sender, tier });
        receiver
    }

    async fn await_decision_with_timeout(
        &self,
        id: &str,
        tier: Tier,
        timeout: Duration,
    ) -> Decision {
        let receiver = self.begin(id, tier);
        self.settle(id, timeout, receiver).await
    }

    /// A waiter whose sender was replaced by a newer one under the same id is
    /// cancelled, and only a sender nobody is waiting on is let go, so the
    /// newer waiter stays decidable.
    async fn settle(
        &self,
        id: &str,
        timeout: Duration,
        receiver: oneshot::Receiver<Decision>,
    ) -> Decision {
        let settled = tokio::time::timeout(timeout, receiver).await;
        match settled {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => Decision::Cancelled,
            Err(_) => {
                self.pending
                    .remove_if(id, |_, waiter| waiter.sender.is_closed());
                Decision::TimedOut
            }
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
            .map(|(_, waiter)| waiter.sender.send(decision).is_ok())
            .unwrap_or(false)
    }

    pub fn cancel_all(&self) {
        self.resolve_where(Decision::Cancelled, |_| true);
    }

    pub fn approve_within(&self, ceiling: Option<Tier>) {
        self.resolve_where(Decision::Approved, |tier| within(tier, ceiling));
    }

    fn resolve_where(&self, decision: Decision, admits: impl Fn(Tier) -> bool) {
        for id in self
            .pending
            .iter()
            .filter(|entry| admits(entry.value().tier))
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
        let wait = tokio::spawn(async move { clone.await_decision("call_1", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_1", true));
        assert_eq!(wait.await.unwrap(), Decision::Approved);
    }

    #[test]
    fn unknown_ids_are_not_decisions() {
        assert!(!ApprovalGate::new().decide("missing", true));
    }

    #[test]
    fn a_parked_call_reads_as_waiting_until_it_is_decided() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        assert!(!policy.waiting(), "nothing was asked yet");

        let _pending = policy.expect_decision("call_1", Tier::Host);
        assert!(policy.waiting());

        policy.decide("call_1", true);
        assert!(!policy.waiting());
    }

    /// Auto-approve parks nothing, so it never reads as waiting for a reader.
    #[test]
    fn auto_approve_never_waits() {
        let policy = ApprovalPolicy::auto();
        let _pending = policy.expect_decision("call_1", Tier::Host);
        assert!(!policy.waiting());
    }

    #[test]
    fn only_a_registered_chat_with_a_parked_call_is_waiting() {
        let chat = Uuid::new_v4();
        assert!(
            !ApprovalPolicy::chat_waiting(chat),
            "a chat with no turn in flight was reported as waiting"
        );

        let policy = ApprovalPolicy::required(ApprovalGate::new());
        ApprovalPolicy::register(chat, policy.clone());
        assert!(!ApprovalPolicy::chat_waiting(chat));

        let _pending = policy.expect_decision("call_1", Tier::Host);
        assert!(ApprovalPolicy::chat_waiting(chat));

        ApprovalPolicy::unregister(chat, &policy);
        assert!(!ApprovalPolicy::chat_waiting(chat));
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
        use abnegate_agent::Tool;
        use abnegate_agent::tool::ApplyPatchTool;
        use abnegate_agent::tool::ReadFileTool;
        use abnegate_agent::tool::RunShellTool;
        use abnegate_agent::tool::WriteFileTool;

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
        let wait =
            tokio::spawn(async move { waiting.await_decision("call_live", Tier::Host).await });
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
            policy.await_decision("already_on", Tier::Host).await,
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
        let wait = tokio::spawn(async move { clone.await_decision("call_deny", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_deny", false));
        assert_eq!(wait.await.unwrap(), Decision::Denied);
    }

    #[tokio::test]
    async fn turning_auto_on_does_not_overturn_a_denial_already_given() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        let waiting = policy.clone();
        let wait =
            tokio::spawn(async move { waiting.await_decision("call_denied", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(policy.decide("call_denied", false));
        policy.set_auto(true);
        assert_eq!(wait.await.unwrap(), Decision::Denied);
    }

    #[tokio::test]
    async fn turning_auto_on_does_not_run_a_call_whose_turn_stopped() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        let waiting = policy.clone();
        let wait =
            tokio::spawn(async move { waiting.await_decision("call_stopped", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        policy.deny_all();
        policy.set_auto(true);
        assert_eq!(wait.await.unwrap(), Decision::Cancelled);
    }

    #[tokio::test]
    async fn a_decision_delivered_before_the_wait_starts_survives_auto_turning_on() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());

        let cancelled = policy.expect_decision("call_cancelled", Tier::Host);
        policy.deny_all();
        let denied = policy.expect_decision("call_denied", Tier::Host);
        assert!(policy.decide("call_denied", false));
        policy.set_auto(true);

        assert_eq!(
            policy
                .awaited_decision("call_cancelled", Tier::Host, cancelled)
                .await,
            Decision::Cancelled
        );
        assert_eq!(
            policy
                .awaited_decision("call_denied", Tier::Host, denied)
                .await,
            Decision::Denied
        );
    }

    #[tokio::test]
    async fn decide_twice_is_only_a_decision_once() {
        let gate = ApprovalGate::new();
        let clone = gate.clone();
        let wait = tokio::spawn(async move { clone.await_decision("call_1", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(gate.decide("call_1", true));
        assert!(!gate.decide("call_1", false));
        assert_eq!(wait.await.unwrap(), Decision::Approved);
    }

    #[tokio::test]
    async fn cancel_all_cancels_every_waiter_rather_than_denying_it() {
        let gate = ApprovalGate::new();
        let first = gate.clone();
        let second = gate.clone();
        let wait_a = tokio::spawn(async move { first.await_decision("a", Tier::Host).await });
        let wait_b = tokio::spawn(async move { second.await_decision("b", Tier::Host).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        gate.cancel_all();
        assert_eq!(wait_a.await.unwrap(), Decision::Cancelled);
        assert_eq!(wait_b.await.unwrap(), Decision::Cancelled);
        assert!(!gate.decide("a", true));
    }

    #[tokio::test]
    async fn a_waiter_nobody_answers_times_out_rather_than_being_denied() {
        let gate = ApprovalGate::new();
        let decision = gate
            .await_decision_with_timeout("late", Tier::Host, Duration::from_millis(15))
            .await;
        assert_eq!(decision, Decision::TimedOut);
        assert!(!gate.decide("late", true));
    }

    #[tokio::test]
    async fn a_waiter_replaced_under_the_same_id_is_cancelled_and_leaves_the_new_one_waiting() {
        let gate = ApprovalGate::new();
        let replaced = gate.begin("call_again", Tier::Host);
        let current = gate.begin("call_again", Tier::Host);

        let decision = gate
            .settle("call_again", Duration::from_secs(5), replaced)
            .await;

        assert_eq!(decision, Decision::Cancelled);
        assert!(
            gate.decide("call_again", true),
            "the newer waiter is still decidable"
        );
        assert_eq!(current.await.unwrap(), Decision::Approved);
    }

    const TIERS: [Tier; 4] = [Tier::Read, Tier::Write, Tier::Host, Tier::Outward];

    fn policy(auto: bool, ceiling: Option<Tier>) -> ApprovalPolicy {
        let policy = ApprovalPolicy::required(ApprovalGate::new()).with_ceiling(ceiling);
        policy.set_auto(auto);
        policy
    }

    #[test]
    fn a_ceiling_caps_what_auto_approve_releases_and_nothing_else() {
        for (auto, ceiling, confirmed) in [
            (false, None, [false, false, true, true]),
            (false, Some(Tier::Write), [false, false, true, true]),
            (false, Some(Tier::Host), [false, false, true, true]),
            (true, None, [false, false, false, false]),
            (true, Some(Tier::Write), [false, false, true, true]),
            (true, Some(Tier::Host), [false, false, false, true]),
        ] {
            let policy = policy(auto, ceiling);
            for (tier, confirmed) in TIERS.into_iter().zip(confirmed) {
                assert_eq!(
                    policy.confirms(tier),
                    confirmed,
                    "auto {auto}, ceiling {ceiling:?}, tier {tier:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn turning_auto_on_releases_only_the_waiters_within_the_ceiling() {
        let policy = ApprovalPolicy::required(ApprovalGate::new()).with_ceiling(Some(Tier::Host));
        let host = policy.expect_decision("host", Tier::Host);
        let outward = policy.expect_decision("outward", Tier::Outward);

        policy.set_auto(true);

        assert_eq!(
            policy.awaited_decision("host", Tier::Host, host).await,
            Decision::Approved
        );
        assert!(policy.waiting(), "the outward call still needs the reader");
        assert!(policy.decide("outward", false));
        assert_eq!(
            policy
                .awaited_decision("outward", Tier::Outward, outward)
                .await,
            Decision::Denied
        );
    }

    #[tokio::test]
    async fn turning_auto_on_releases_every_waiter_without_a_ceiling() {
        let policy = ApprovalPolicy::required(ApprovalGate::new());
        let outward = policy.expect_decision("outward", Tier::Outward);

        policy.set_auto(true);

        assert_eq!(
            policy
                .awaited_decision("outward", Tier::Outward, outward)
                .await,
            Decision::Approved
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_above_the_ceiling_times_out_under_auto_rather_than_running() {
        let policy = policy(true, Some(Tier::Write));
        assert_eq!(
            policy.await_decision("host", Tier::Host).await,
            Decision::TimedOut
        );

        let late = ApprovalPolicy::required(ApprovalGate::new()).with_ceiling(Some(Tier::Host));
        let pending = late.expect_decision("outward", Tier::Outward);
        late.set_auto(true);
        assert_eq!(
            late.awaited_decision("outward", Tier::Outward, pending)
                .await,
            Decision::TimedOut
        );
    }

    #[tokio::test]
    async fn auto_approve_within_the_ceiling_still_waits_for_nothing() {
        let policy = policy(true, Some(Tier::Host));
        assert_eq!(
            policy.await_decision("host", Tier::Host).await,
            Decision::Approved
        );
        assert!(!policy.waiting());
    }

    #[test]
    fn a_ceiling_set_on_a_clone_is_the_originals() {
        let policy = ApprovalPolicy::auto();
        policy.clone().set_ceiling(Some(Tier::Write));
        assert_eq!(policy.ceiling(), Some(Tier::Write));
        assert!(policy.confirms(Tier::Host));

        policy.clone().set_ceiling(None);
        assert_eq!(policy.ceiling(), None);
        assert!(!policy.confirms(Tier::Host));
    }

    fn recorded(policy: &ApprovalPolicy) -> Arc<std::sync::Mutex<Vec<String>>> {
        let names = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::clone(&names);
        policy.set_recorder(Arc::new(move |name: &str| {
            recorder.lock().expect("the record").push(name.to_string());
        }));
        names
    }

    #[tokio::test]
    async fn an_unattended_call_above_the_ceiling_is_withheld_at_once_and_recorded() {
        let policy = ApprovalPolicy::unattended().with_ceiling(Some(Tier::Write));
        let names = recorded(&policy.clone());

        assert!(policy.withheld("run_command", Tier::Host));
        let pending = policy.expect_decision("call", Tier::Host);
        assert!(pending.is_none(), "nobody is there to answer");
        assert!(!policy.waiting());
        let decision = policy.awaited_decision("call", Tier::Host, pending).await;

        assert_eq!(decision, Decision::Withheld);
        assert_eq!(decision.refusal(), Some(Refusal::Policy));
        assert_eq!(*names.lock().expect("the record"), ["run_command"]);
    }

    #[tokio::test]
    async fn an_unattended_call_within_the_ceiling_runs_unrecorded() {
        let policy = ApprovalPolicy::unattended().with_ceiling(Some(Tier::Host));
        let names = recorded(&policy);

        for tier in [Tier::Read, Tier::Write, Tier::Host] {
            assert!(!policy.withheld("write_file", tier), "{tier:?}");
            assert_eq!(
                policy.await_decision("call", tier).await,
                Decision::Approved,
                "{tier:?}"
            );
        }
        assert!(names.lock().expect("the record").is_empty());
    }

    #[tokio::test]
    async fn an_unattended_run_without_a_ceiling_approves_everything() {
        let policy = ApprovalPolicy::unattended();
        assert!(policy.is_auto());
        for tier in TIERS {
            assert!(!policy.confirms(tier), "{tier:?}");
            assert!(!policy.withheld("send_message", tier), "{tier:?}");
            assert_eq!(
                policy.await_decision("call", tier).await,
                Decision::Approved,
                "{tier:?}"
            );
        }
    }

    #[test]
    fn an_attended_chat_above_the_ceiling_asks_rather_than_withholds() {
        let policy = policy(true, Some(Tier::Write));
        assert!(!policy.withheld("run_command", Tier::Host));
        assert!(policy.expect_decision("call", Tier::Host).is_some());
    }
}
