//! The fleet-wide sweep the production worker runs, on a database of its own
//! so it cannot take another test's firings.
use crate::common::Database;
use chrono::Duration;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::reminders;
use zone_server::db::reminders::Delivered;
use zone_server::db::reminders::Reminder;
use zone_server::db::reminders::Sweep;

const PREFIX: &str = "reminder_sweep";
const LEASE: std::time::Duration = std::time::Duration::from_secs(3600);

struct Firings {
    chat: Uuid,
    watch: Uuid,
}

async fn seed(pool: &PgPool) -> Firings {
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
    let mut watches = Vec::new();
    for _ in 0..2 {
        let created = reminders::create(pool, workspace, user, chat, reminder(Some("Check it.")))
            .await
            .unwrap();
        watches.push(serde_json::from_value::<Uuid>(created["id"].clone()).unwrap());
    }
    sqlx::query(
        "UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' \
         WHERE workspace_id = $1 AND prompt IS NULL",
    )
    .bind(workspace)
    .execute(pool)
    .await
    .unwrap();
    let (watch, spent) = (watches[0], watches[1]);
    for (reminder, attempts, claimed) in [
        (watch, 0, None),
        (spent, 3, Some(Utc::now() - Duration::days(1))),
    ] {
        sqlx::query(
            "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, prompt, \
             attempts, claimed_at) VALUES ($1, $2, $3, $4, 'Check it.', $5, $6)",
        )
        .bind(reminder)
        .bind(workspace)
        .bind(chat)
        .bind(user)
        .bind(attempts)
        .bind(claimed)
        .execute(pool)
        .await
        .unwrap();
    }
    Firings { chat, watch }
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
    let first = seed(pool).await;
    let second = seed(pool).await;

    let mut deliveries = Vec::new();
    for _ in 0..3 {
        deliveries.push(reminders::deliver_next(pool, Sweep::All).await.unwrap());
    }
    let mut claimed = Vec::new();
    for _ in 0..3 {
        claimed.push(
            reminders::claim_turn(pool, Sweep::All, LEASE)
                .await
                .unwrap()
                .map(|turn| turn.reminder_id),
        );
    }
    let delivered = (
        messages(pool, first.chat).await,
        messages(pool, second.chat).await,
    );
    let spent: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reminder_turns WHERE attempts >= 3")
        .fetch_one(pool)
        .await
        .unwrap();
    database.cleanup().await;

    assert_eq!(
        deliveries,
        [Delivered::Settled, Delivered::Settled, Delivered::Nothing],
        "each workspace's due reminder is the fleet's to deliver"
    );
    assert_eq!(delivered, (1, 1));
    let mut taken: Vec<Uuid> = claimed[..2].iter().flatten().copied().collect();
    taken.sort();
    let mut expected = vec![first.watch, second.watch];
    expected.sort();
    assert_eq!(
        taken, expected,
        "each workspace's owed turn is the fleet's to claim"
    );
    assert_eq!(claimed[2], None, "nothing else is owed");
    assert_eq!(
        spent, 0,
        "every workspace's spent turn is the fleet's to give up on"
    );
}
