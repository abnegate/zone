use sqlx::migrate::MigrateError;
use sqlx::{ConnectOptions, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::migrations;

struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
}

impl Database {
    async fn new() -> Self {
        let admin =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("disposable database"))
                .await
                .unwrap();
        // Identifiers contain only a fixed prefix and UUID hexadecimal.
        let name = format!("task_migration_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .unwrap();
        let options = (*admin.connect_options()).clone().database(&name);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        Self { admin, pool, name }
    }

    async fn through(&self, version: i64) {
        sqlx::migrate!("./migrations")
            .run_to(version, &self.pool)
            .await
            .unwrap();
    }

    async fn task(&self) -> Uuid {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let task = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Migration',$1::text)")
            .bind(organization)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Migration',$1::text)").bind(workspace).bind(organization).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO tasks(id,workspace_id,title,description) VALUES($1,$2,'Migration','Regression')").bind(task).bind(workspace).execute(&self.pool).await.unwrap();
        task
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {}", self.name)))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
async fn task_migration_metadata_defers_scans_and_fences_only_admissions() {
    let database = Database::new().await;
    database.through(16).await;
    let task = database.task().await;
    let run = Uuid::new_v4();
    sqlx::query("INSERT INTO task_runs(id,task_id,status) VALUES($1,$2,'running')")
        .bind(run)
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    database.through(17).await;
    let unvalidated: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_constraint WHERE conname IN ('task_runs_triggered_by_fkey','tasks_active_run_id_fkey') AND NOT convalidated").fetch_one(&database.pool).await.unwrap();
    assert_eq!(
        unvalidated, 2,
        "metadata migration must not scan either foreign key"
    );
    let rejected = sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,'running')")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap_err();
    assert_eq!(
        rejected.as_database_error().unwrap().code().as_deref(),
        Some("55000")
    );
    sqlx::query("UPDATE tasks SET title='Concurrent edit' WHERE id=$1")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE task_runs SET status='completed' WHERE id=$1")
        .bind(run)
        .execute(&database.pool)
        .await
        .unwrap();
    migrations::run(&database.pool).await.unwrap();
    let validated: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_constraint WHERE conname IN ('task_runs_triggered_by_fkey','tasks_active_run_id_fkey') AND convalidated").fetch_one(&database.pool).await.unwrap();
    assert_eq!(validated, 2);
    sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,'running')")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    migrations::run(&database.pool).await.unwrap();
    database.cleanup().await;
}

#[tokio::test]
async fn task_migration_repairs_cancelled_concurrent_build_with_live_writes() {
    let database = Database::new().await;
    database.through(16).await;
    let task = database.task().await;
    sqlx::query(
        "INSERT INTO task_runs(task_id,status) SELECT $1,'completed' FROM generate_series(1,20000)",
    )
    .bind(task)
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,'running'),($1,'running')")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    database.through(18).await;
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM task_runs WHERE status='running'")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(active, 1);
    let mut blocker = database.pool.acquire().await.unwrap();
    sqlx::query("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT count(*) FROM task_runs")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let pool = database.pool.clone();
    let migrating = tokio::spawn(async move { migrations::run(&pool).await });
    let process: i32 = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let process: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_progress_create_index WHERE relid='task_runs'::regclass AND index_relid=to_regclass('task_runs_active')").fetch_optional(&database.pool).await.unwrap();
            if let Some(process) = process { break process; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    // Concurrent index construction permits ordinary writes while an older
    // reader deliberately holds up completion. The admission fence still holds.
    tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query("UPDATE tasks SET title='Still writable' WHERE id=$1")
            .bind(task)
            .execute(&database.pool),
    )
    .await
    .unwrap()
    .unwrap();
    let rejected = sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,'running')")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap_err();
    assert_eq!(
        rejected.as_database_error().unwrap().code().as_deref(),
        Some("55000")
    );
    migrating.abort();
    assert!(migrating.await.unwrap_err().is_cancelled());
    // Keep the old snapshot open: disconnect detection must terminate the query
    // and release the advisory lock without help from that blocking reader.
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let alive: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1)")
                    .bind(process)
                    .fetch_one(&database.pool)
                    .await
                    .unwrap();
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("aborted migration retained its backend and advisory lock");
    sqlx::query("ROLLBACK")
        .execute(&mut *blocker)
        .await
        .unwrap();
    drop(blocker);
    let invalid: bool = sqlx::query_scalar(
        "SELECT NOT indisvalid FROM pg_index WHERE indexrelid='task_runs_active'::regclass",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(
        invalid,
        "cancelled build must leave actual invalid-index recovery work"
    );
    let recorded: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=19)")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(!recorded);
    // Fail-closed checksum validation must happen before touching that index.
    for version in [19_i64, 20, 21] {
        sqlx::query("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES($1,'drift',true,$2,0)").bind(version).bind(vec![0_u8]).execute(&database.pool).await.unwrap();
        assert!(
            matches!(migrations::run(&database.pool).await, Err(MigrateError::VersionMismatch(found)) if found==version)
        );
        let invalid: bool = sqlx::query_scalar(
            "SELECT NOT indisvalid FROM pg_index WHERE indexrelid='task_runs_active'::regclass",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(invalid);
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version=$1")
            .bind(version)
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let (first, second) = tokio::join!(
        migrations::run(&database.pool),
        migrations::run(&database.pool)
    );
    first.unwrap();
    second.unwrap();
    let valid: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_index WHERE indexrelid IN ('task_runs_active'::regclass,'task_runs_heartbeat'::regclass) AND indisvalid").fetch_one(&database.pool).await.unwrap();
    assert_eq!(valid, 2);
    let fenced: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname='task_runs_migration_admission')",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(!fenced);
    database.cleanup().await;
}

#[tokio::test]
async fn task_migration_observes_completion_after_reconciliation_lock_wait() {
    let database = Database::new().await;
    database.through(16).await;
    let task = database.task().await;
    let first = Uuid::new_v4();
    let latest = Uuid::new_v4();
    sqlx::query("INSERT INTO task_runs(id,task_id,status,started_at) VALUES($1,$3,'running',NOW()-INTERVAL '1 minute'),($2,$3,'running',NOW())")
        .bind(first).bind(latest).bind(task).execute(&database.pool).await.unwrap();
    database.through(17).await;
    let mut completing = database.pool.begin().await.unwrap();
    // This is the old worker's exact terminal mutation shape: task_runs only.
    sqlx::query("UPDATE task_runs SET status='completed',completed_at=NOW() WHERE id=$1")
        .bind(latest)
        .execute(&mut *completing)
        .await
        .unwrap();
    let pool = database.pool.clone();
    let migrating =
        tokio::spawn(async move { sqlx::migrate!("./migrations").run_to(18, &pool).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%Lock live runs%')")
                .fetch_one(&database.pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    completing.commit().await.unwrap();
    migrating.await.unwrap().unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM task_runs WHERE id=$1")
        .bind(latest)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(
        status, "completed",
        "reconciliation overwrote a committed legacy completion"
    );
    let active: Option<Uuid> = sqlx::query_scalar("SELECT active_run_id FROM tasks WHERE id=$1")
        .bind(task)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(active, Some(first));
    migrations::run(&database.pool).await.unwrap();
    // Legacy completion after final validation must not leave a terminal active pointer.
    sqlx::query("UPDATE task_runs SET status='completed',completed_at=NOW() WHERE id=$1")
        .bind(first)
        .execute(&database.pool)
        .await
        .unwrap();
    zone_server::db::recovery::reconcile(&database.pool)
        .await
        .unwrap();
    let task: (String, Option<Uuid>) =
        sqlx::query_as("SELECT status,active_run_id FROM tasks WHERE id=$1")
            .bind(task)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(task, ("review".to_string(), None));
    database.cleanup().await;
}

#[tokio::test]
async fn task_migration_rejects_unexpected_indexes_and_dirty_ledger() {
    let database = Database::new().await;
    database.through(18).await;
    sqlx::query("CREATE INDEX task_runs_active ON task_runs(id)")
        .execute(&database.pool)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT 'task_runs_active'::regclass::oid::bigint")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(migrations::run(&database.pool).await.is_err());
    let after: i64 = sqlx::query_scalar("SELECT 'task_runs_active'::regclass::oid::bigint")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    sqlx::query("UPDATE _sqlx_migrations SET success=false WHERE version=18")
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(matches!(
        migrations::run(&database.pool).await,
        Err(MigrateError::Dirty(18))
    ));
    let after: i64 = sqlx::query_scalar("SELECT 'task_runs_active'::regclass::oid::bigint")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    database.cleanup().await;
}

#[tokio::test]
async fn legacy_completion_and_recovery_both_commit_without_losing_success() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let task = database.task().await;
    let run = Uuid::new_v4();
    sqlx::query("INSERT INTO task_runs(id,task_id,status) VALUES($1,$2,'completed')")
        .bind(run)
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET active_run_id=$2,status='in_progress' WHERE id=$1")
        .bind(task)
        .bind(run)
        .execute(&database.pool)
        .await
        .unwrap();
    let mut legacy = database.pool.begin().await.unwrap();
    sqlx::query("UPDATE task_runs SET status='completed',completed_at=NOW() WHERE id=$1")
        .bind(run)
        .execute(&mut *legacy)
        .await
        .unwrap();
    let pool = database.pool.clone();
    let recovering = tokio::spawn(async move { zone_server::db::recovery::reconcile(&pool).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT task_runs.id%')").fetch_one(&database.pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    legacy.commit().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), recovering)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        1
    );
    let state: (String, Option<Uuid>, String) = sqlx::query_as("SELECT tasks.status,tasks.active_run_id,task_runs.status FROM tasks JOIN task_runs ON task_runs.task_id=tasks.id WHERE tasks.id=$1").bind(task).fetch_one(&database.pool).await.unwrap();
    assert_eq!(state, ("review".into(), None, "completed".into()));
    database.cleanup().await;
}

#[tokio::test]
async fn task_migration_works_with_one_connection_fresh_and_applied() {
    let database = Database::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect_with(database.pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(10), migrations::run(&pool))
            .await
            .unwrap()
            .unwrap();
    }
    pool.close().await;
    database.cleanup().await;
}

#[tokio::test]
async fn migration_command_needs_only_database_and_propagates_failures() {
    let database = Database::new().await;
    let url = database.pool.connect_options().to_url_lossy().to_string();
    for expected in [true, false] {
        if !expected {
            sqlx::query("UPDATE _sqlx_migrations SET checksum='\\x00'::bytea WHERE version=21")
                .execute(&database.pool)
                .await
                .unwrap();
        }
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_zone-server"))
                .arg("--migrate-only")
                .env_clear()
                .env("DATABASE_URL", &url)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            output.status.success(),
            expected,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    database.cleanup().await;
}
