//! Every agent gauge is computed from one query capped by `limit`. The cap has
//! no signal attached, so once a workspace crosses it the gauges stay
//! plausible while describing only part of the window.
//!
//! Which part is the whole question. All the periods summarised from these rows
//! end at `now`, so the rows worth keeping are the newest ones. Ordering the
//! query oldest-first meant the cap threw those away, and the day and week
//! gauges went empty on the busiest workspaces -- the ones anyone would be
//! watching.

use crate::common;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use zone_server::db::analytics;

async fn workspace_with_runs(pool: &PgPool, count: i64) -> Uuid {
    let (_, workspace_id, _) = common::setup_test_data(pool).await;

    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (workspace_id, title, description) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(workspace_id)
    .bind("analytics window")
    .bind("runs spanning the whole lookback")
    .fetch_one(pool)
    .await
    .expect("task is created");

    let now = Utc::now().naive_utc();

    // One run per hour going back, so run 0 is the newest.
    for hour in 0..count {
        let finished = now - Duration::hours(hour + 1);
        sqlx::query(
            "INSERT INTO task_runs (task_id, status, started_at, completed_at)
             VALUES ($1, 'completed', $2, $3)",
        )
        .bind(task_id)
        .bind(finished - Duration::minutes(1))
        .bind(finished)
        .execute(pool)
        .await
        .expect("run is created");
    }

    workspace_id
}

#[tokio::test]
async fn the_run_cap_drops_the_oldest_runs_not_the_newest() {
    let pool = common::create_test_pool().await;
    let workspace_id = workspace_with_runs(&pool, 10).await;

    let now = Utc::now().naive_utc();
    let start = now - Duration::days(30);
    let limit = 4;

    let capped = analytics::finished_runs(&pool, workspace_id, start, now, limit)
        .await
        .expect("runs are readable")
        .rows;

    assert_eq!(capped.len(), limit as usize, "the cap is applied");

    let all = analytics::finished_runs(&pool, workspace_id, start, now, 100)
        .await
        .expect("runs are readable")
        .rows;
    assert_eq!(all.len(), 10, "the workspace really does have more runs");

    let newest = all.iter().map(|row| row.finished_at).max().expect("a run");
    let kept = capped
        .iter()
        .map(|row| row.finished_at)
        .max()
        .expect("a run");
    assert_eq!(
        kept, newest,
        "the most recent run must survive the cap; every period a gauge reports \
         ends at now, so losing the newest rows empties the short windows"
    );

    let oldest_kept = capped
        .iter()
        .map(|row| row.finished_at)
        .min()
        .expect("a run");
    let oldest = all.iter().map(|row| row.finished_at).min().expect("a run");
    assert!(
        oldest_kept > oldest,
        "the cap should have cost the oldest runs, not the newest"
    );
}

#[tokio::test]
async fn an_uncapped_window_returns_every_run_in_it() {
    let pool = common::create_test_pool().await;
    let workspace_id = workspace_with_runs(&pool, 5).await;

    let now = Utc::now().naive_utc();

    let inside = analytics::finished_runs(&pool, workspace_id, now - Duration::days(1), now, 100)
        .await
        .expect("runs are readable")
        .rows;
    assert_eq!(inside.len(), 5);

    // `[start, end)`: a window that ends before the newest run excludes it.
    let earlier = analytics::finished_runs(
        &pool,
        workspace_id,
        now - Duration::days(1),
        now - Duration::hours(2),
        100,
    )
    .await
    .expect("runs are readable")
    .rows;
    assert_eq!(earlier.len(), 4);
}

#[tokio::test]
async fn reaching_the_run_cap_exactly_is_not_passing_it() {
    let pool = common::create_test_pool().await;
    let workspace_id = workspace_with_runs(&pool, 4).await;

    let now = Utc::now().naive_utc();
    let start = now - Duration::days(1);

    let exact = analytics::finished_runs(&pool, workspace_id, start, now, 4)
        .await
        .expect("runs are readable");
    assert_eq!(exact.rows.len(), 4);
    assert!(
        !exact.truncated,
        "every run in the window was read, so nothing is missing"
    );

    let short = analytics::finished_runs(&pool, workspace_id, start, now, 3)
        .await
        .expect("runs are readable");
    assert_eq!(short.rows.len(), 3, "the cap still holds");
    assert!(short.truncated, "one run past the cap was left out");
}

#[tokio::test]
async fn a_run_still_going_is_not_a_finished_run() {
    let pool = common::create_test_pool().await;
    let workspace_id = workspace_with_runs(&pool, 2).await;
    let task_id: Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(&pool)
        .await
        .expect("the task exists");
    let now = Utc::now().naive_utc();
    for status in ["running", "waiting"] {
        sqlx::query("INSERT INTO task_runs (task_id, status, started_at) VALUES ($1, $2, $3)")
            .bind(task_id)
            .bind(status)
            .bind(now - Duration::minutes(5))
            .execute(&pool)
            .await
            .expect("an unfinished run is created");
    }

    let runs = analytics::finished_runs(&pool, workspace_id, now - Duration::days(1), now, 100)
        .await
        .expect("runs are readable")
        .rows;
    assert_eq!(
        runs.iter()
            .map(|row| row.status.as_str())
            .collect::<Vec<_>>(),
        vec!["completed", "completed"],
        "runs that have not finished have no outcome to count"
    );
}
