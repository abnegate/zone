use std::time::Duration;

use uuid::Uuid;

use super::{Fleet, UNREGISTERED, enforced, unenforced_on};
use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::state::State;
use crate::db::reminders::{self, PendingTurn, Sweep};
use crate::db::testing::queued_behind;

const LEASE: Duration = Duration::from_secs(3600);
const ANOTHER_CLAIM_WITHIN: Duration = Duration::from_secs(5);

impl Fleet {
    /// A firing that owes its chat a turn.
    async fn owed(&self) -> Uuid {
        let chat = self.chat().await;
        let reminder: Uuid = sqlx::query_scalar(
            "INSERT INTO reminders (workspace_id, chat_id, created_by, content, due_at, prompt) \
             VALUES ($1, $2, $3, 'Check', NOW(), 'Check the build') RETURNING id",
        )
        .bind(self.workspace)
        .bind(chat)
        .bind(self.user)
        .fetch_one(self.pool())
        .await
        .expect("a reminder");
        sqlx::query_scalar(
            "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, prompt) \
             VALUES ($1, $2, $3, $4, 'Check the build') RETURNING id",
        )
        .bind(reminder)
        .bind(self.workspace)
        .bind(chat)
        .bind(self.user)
        .fetch_one(self.pool())
        .await
        .expect("an owed turn")
    }

    async fn claim_turn(&self, claimant: Claimant) -> Option<PendingTurn> {
        reminders::claim_turn(self.pool(), Sweep::All, LEASE, claimant)
            .await
            .expect("the claim")
    }

    async fn turn_claim(&self, turn: Uuid) -> (i32, bool) {
        sqlx::query_as("SELECT attempts, claimed_at IS NOT NULL FROM reminder_turns WHERE id = $1")
            .bind(turn)
            .fetch_one(self.pool())
            .await
            .expect("the owed turn")
    }
}

#[tokio::test]
async fn a_turn_is_claimed_only_by_a_runner_that_may_take_it() {
    let fleet = Fleet::new().await;
    let short = fleet.short().await;
    let eligible = fleet.runner(State::Active, "2.0.0", "gpu").await;
    let owed = fleet.owed().await;

    let mut refused = Vec::new();
    for (why, runner) in short {
        refused.push((why, fleet.claim_turn(enforced(runner)).await));
    }
    let unregistered = fleet.claim_turn(UNREGISTERED).await;
    let claimed = fleet.claim_turn(enforced(eligible)).await;
    let released = reminders::release_turn(fleet.pool(), owed, 1)
        .await
        .expect("the release");
    let after_release = fleet.turn_claim(owed).await;
    let (_, revoked) = short[1];
    let unenforced = fleet.claim_turn(unenforced_on(revoked)).await;
    let stale = reminders::release_turn(fleet.pool(), owed, 2)
        .await
        .expect("the stale release");
    fleet.remove().await;

    for (why, claim) in refused {
        assert_eq!(claim, None, "a {why} runner claimed a reminder turn");
    }
    assert_eq!(unregistered, None, "an unregistered runner claimed a turn");
    let claimed = claimed.expect("an eligible runner claims the turn");
    assert_eq!((claimed.id, claimed.attempts), (owed, 1));
    assert!(released, "the claim was given back");
    assert_eq!(
        after_release,
        (0, false),
        "a turn given back is owed again with no attempt counted"
    );
    let unenforced = unenforced.expect("an unenforced claimant claims the turn");
    assert_eq!((unenforced.id, unenforced.attempts), (owed, 1));
    assert!(!stale, "a release fenced on another claim's attempts");
}

#[tokio::test]
async fn a_revoked_runner_cannot_renew_its_turn() {
    let fleet = Fleet::new().await;
    let runner = fleet.runner(State::Active, "2.0.0", "").await;
    fleet.owed().await;
    let turn = fleet
        .claim_turn(enforced(runner))
        .await
        .expect("the runner claims the turn");

    let before = reminders::renew_turn(
        fleet.pool(),
        turn.id,
        turn.attempts,
        LEASE,
        enforced(runner),
    )
    .await
    .expect("the renewal");
    fleet.revoke(runner).await;
    let after = reminders::renew_turn(
        fleet.pool(),
        turn.id,
        turn.attempts,
        LEASE,
        enforced(runner),
    )
    .await
    .expect("the renewal");
    let unenforced = reminders::renew_turn(
        fleet.pool(),
        turn.id,
        turn.attempts,
        LEASE,
        Claimant::UNENFORCED,
    )
    .await
    .expect("the renewal");
    fleet.remove().await;

    assert!(before, "an active runner renews its turn");
    assert!(!after, "a revoked runner renewed its turn");
    assert!(
        unenforced,
        "an unenforced renewal is fenced on the claim alone"
    );
}

#[tokio::test]
async fn labels_are_read_from_the_turns_own_workspace() {
    let fleet = Fleet::new().await;
    let other: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) \
         VALUES ($1, 'Other', gen_random_uuid()::text) RETURNING id",
    )
    .bind(fleet.organization)
    .fetch_one(fleet.pool())
    .await
    .expect("another workspace");
    fleet.require(other, "gpu").await;
    let runner = fleet.runner(State::Active, "2.0.0", "").await;
    fleet.owed().await;

    let claimed = fleet.claim_turn(enforced(runner)).await;
    fleet.remove().await;

    assert!(
        claimed.is_some(),
        "another workspace's labels kept this one's turn from a runner"
    );
}

#[tokio::test]
async fn a_turn_is_not_claimed_on_another_processs_registration_of_the_runner() {
    let fleet = Fleet::new().await;
    let runner = fleet.runner(State::Active, "2.0.0", "").await;
    fleet.require(fleet.workspace, "gpu").await;
    fleet.take_over(runner, "2.0.0", "gpu").await;
    let owed = fleet.owed().await;

    let claimed = fleet.claim_turn(enforced(runner)).await;
    let claim = fleet.turn_claim(owed).await;
    fleet.remove().await;

    assert_eq!(
        claimed, None,
        "an unlabelled process claimed a turn on another process's labels"
    );
    assert_eq!(claim, (0, false));
}

#[tokio::test]
async fn a_claim_waiting_on_its_runners_revoke_holds_up_no_other_runners_claim() {
    let fleet = Fleet::new().await;
    let revoking = fleet.runner(State::Active, "2.0.0", "").await;
    let other = fleet.runner(State::Active, "2.0.0", "").await;
    let owed = fleet.owed().await;

    let mut revoke = fleet.pool().begin().await.expect("a revoke");
    let backend: i32 =
        sqlx::query_scalar("SELECT pg_backend_pid() FROM fleet_runners WHERE id = $1 FOR UPDATE")
            .bind(revoking)
            .fetch_one(&mut *revoke)
            .await
            .expect("the revoked runner's row");
    let waiting = tokio::spawn({
        let pool = fleet.pool().clone();
        async move { reminders::claim_turn(&pool, Sweep::All, LEASE, enforced(revoking)).await }
    });
    queued_behind(fleet.pool(), backend).await;
    let claimed =
        tokio::time::timeout(ANOTHER_CLAIM_WITHIN, fleet.claim_turn(enforced(other))).await;
    revoke.rollback().await.expect("the revoke rolls back");
    let waited = waiting.await;
    fleet.remove().await;

    let claimed = claimed
        .expect("another runner's claim waited on the revoke")
        .expect("the other runner claims the owed turn");
    assert_eq!(claimed.id, owed);
    assert_eq!(
        waited.expect("the waiting claim").expect("the claim"),
        None,
        "the turn the other runner holds was claimed twice"
    );
}
