use crate::common;
use crate::common::context::{Harness, answer};
use crate::reminder_turn::{make_due, until_requests};
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::fleet::claimant::Claimant;
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
    let turn = reminders::claim_turn(
        &harness.pool,
        sweep,
        UNLAPSING.duration,
        Claimant::UNENFORCED,
    )
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

async fn messages(harness: &Harness) -> Vec<String> {
    sqlx::query_scalar("SELECT role FROM messages WHERE chat_id = $1 ORDER BY created_at, id")
        .bind(harness.chat)
        .fetch_all(&harness.pool)
        .await
        .unwrap()
}

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
    let running =
        tokio::spawn(async move { run(&state, turn, UNLAPSING, Claimant::UNENFORCED).await });
    until_requests(&harness, 1).await;

    suspend(&harness, user).await;

    tokio::time::timeout(ENDED_WITHIN, running)
        .await
        .expect("a suspended member's turn kept generating")
        .expect("the ended turn did not panic");
}

#[tokio::test]
async fn a_recurring_reminder_of_a_departed_member_stops_firing() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let user = owner(&harness).await;
    let reminder = schedule(&harness, user, None, Some("FREQ=DAILY")).await;
    sqlx::query(
        "UPDATE organization_members SET is_active = FALSE \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization(&harness).await)
    .bind(user)
    .execute(&harness.pool)
    .await
    .unwrap();

    assert_eq!(
        reminders::deliver_next(&harness.pool, Sweep::Workspace(harness.workspace))
            .await
            .unwrap(),
        Delivered::Settled
    );

    assert_eq!(
        messages(&harness).await,
        Vec::<String>::new(),
        "the departed member's reminder was delivered"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM reminders WHERE id = $1")
        .bind(reminder)
        .fetch_one(&harness.pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled", "the schedule kept going");
}
