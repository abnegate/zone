//! The fleet-wide sweep the production worker runs, on a database of its own
//! so it cannot take another test's firings.
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::reminders;
use zone_server::db::reminders::Delivered;
use zone_server::db::reminders::Reminder;
use zone_server::db::reminders::Sweep;

use crate::common::Database;

const PREFIX: &str = "reminder_sweep";
const LEASE: std::time::Duration = std::time::Duration::from_secs(3600);

struct Workspace {
    chat: Uuid,
    watch: Uuid,
}

async fn workspace(pool: &PgPool) -> Workspace {
    let organization = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let user = Uuid::new_v4();
    let chat = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Sweep', $1::text)")
        .bind(organization)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Sweep', $1::text)",
    )
    .bind(workspace)
    .bind(organization)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name) \
         VALUES ($1, $1::text, 'test-only', 'Sweep')",
    )
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(workspace)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chats (id, workspace_id, title, model_name) VALUES ($1, $2, 'Sweep', 'test')",
    )
    .bind(chat)
    .bind(workspace)
    .execute(pool)
    .await
    .unwrap();
    let reminder = |prompt: Option<&str>| Reminder {
        content: "Sweep".into(),
        due_at: (Utc::now() + Duration::hours(1)).fixed_offset(),
        rrule: None,
        prompt: prompt.map(Into::into),
        timing_mode: None,
        time_zone: None,
    };
    reminders::create(pool, workspace, user, chat, reminder(None))
        .await
        .unwrap();
    let created = reminders::create(pool, workspace, user, chat, reminder(Some("Check it.")))
        .await
        .unwrap();
    let watch: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    sqlx::query(
        "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' \
         WHERE workspace_id = $1 AND prompt IS NULL",
    )
    .bind(workspace)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, prompt) \
         VALUES ($1, $2, $3, $4, 'Check it.')",
    )
    .bind(watch)
    .bind(workspace)
    .bind(chat)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    Workspace { chat, watch }
}

async fn messages(pool: &PgPool, chat: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
        .bind(chat)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_fleet_sweep_takes_every_workspace_s_firings() {
    let database = Database::new(PREFIX).await;
    migrations::run(&database.pool).await.unwrap();
    let pool = &database.pool;
    let first = workspace(pool).await;
    let second = workspace(pool).await;

    for _ in 0..2 {
        assert_eq!(
            reminders::deliver_next(pool, Sweep::All).await.unwrap(),
            Delivered::Settled,
            "each workspace's due reminder is the fleet's to deliver"
        );
    }
    assert_eq!(
        reminders::deliver_next(pool, Sweep::All).await.unwrap(),
        Delivered::Nothing
    );
    assert_eq!(messages(pool, first.chat).await, 1);
    assert_eq!(messages(pool, second.chat).await, 1);

    let mut claimed = Vec::new();
    while let Some(turn) = reminders::claim_turn(pool, Sweep::All, LEASE)
        .await
        .unwrap()
    {
        claimed.push(turn.reminder_id);
    }
    claimed.sort();
    let mut expected = vec![first.watch, second.watch];
    expected.sort();
    assert_eq!(
        claimed, expected,
        "each workspace's owed turn is the fleet's to claim"
    );

    database.cleanup().await;
}
