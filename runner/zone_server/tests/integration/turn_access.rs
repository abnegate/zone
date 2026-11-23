//! What a member started keeps running only while they keep their access.
//!
//! Halting a departed member's turns reaches only the replica that handled the
//! departure, so a turn running on another one has to notice for itself.

use crate::common;
use crate::common::context::{Harness, answer};
use crate::reminder_turn::{make_due, until_requests};
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::reminders::{self, Delivered, PendingTurn, Reminder, Sweep};
use zone_server::db::suspensions;
use zone_server::workers::reminders::{Lease, run};

const HANG: Duration = Duration::from_secs(120);
const ENDED_WITHIN: Duration = Duration::from_secs(15);
const UNLAPSING: Lease = Lease {
    duration: Duration::from_secs(600),
    renewal: Duration::from_secs(300),
};

async fn owner(harness: &Harness) -> Uuid {
    sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner")
}

async fn organization(harness: &Harness) -> Uuid {
    sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id = $1")
        .bind(harness.workspace)
        .fetch_one(&harness.pool)
        .await
        .expect("the harness workspace belongs to an organization")
}

async fn schedule(
    harness: &Harness,
    user: Uuid,
    prompt: Option<&str>,
    rrule: Option<&str>,
) -> Uuid {
    let created = reminders::create(
        &harness.pool,
        harness.workspace,
        user,
        harness.chat,
        Reminder {
            content: "Dependency check".into(),
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: rrule.map(str::to_string),
            prompt: prompt.map(str::to_string),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("the reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    make_due(harness, reminder).await;
    reminder
}

async fn claimed(harness: &Harness, reminder: Uuid) -> PendingTurn {
    let sweep = Sweep::Workspace(harness.workspace);
    assert_eq!(
        reminders::deliver_next(&harness.pool, sweep).await.unwrap(),
        Delivered::Settled
    );
    let turn = reminders::claim_turn(&harness.pool, sweep, UNLAPSING.duration)
        .await
        .unwrap()
        .expect("the owed firing is offered");
    assert_eq!(turn.reminder_id, reminder);
    turn
}

async fn suspend(harness: &Harness, user: Uuid) {
    let mut transaction = harness.pool.begin().await.unwrap();
    suspensions::suspend(&mut transaction, organization(harness).await, user)
        .await
        .expect("the member is suspended");
    transaction.commit().await.unwrap();
}

/// The suspension lands without halting anything here, as it does when
/// another replica handles it, and the turn still ends on its own.
#[tokio::test]
async fn a_turn_whose_author_is_suspended_elsewhere_ends() {
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![answer("Never delivered.").set_delay(HANG)],
    )
    .await;
    let user = owner(&harness).await;
    let reminder = schedule(
        &harness,
        user,
        Some("Report any dependency PR that has been red for a week."),
        None,
    )
    .await;
    let turn = claimed(&harness, reminder).await;
    let state = common::create_test_state(harness.config.clone(), harness.pool.clone());
    let running = tokio::spawn(async move { run(&state, turn, UNLAPSING).await });
    until_requests(&harness, 1).await;

    suspend(&harness, user).await;

    tokio::time::timeout(ENDED_WITHIN, running)
        .await
        .expect("a suspended member's turn kept generating")
        .expect("the ended turn did not panic");
}
