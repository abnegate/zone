use sqlx::migrate::MigrateError;
use sqlx::{ConnectOptions, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::{migrations, tasks};

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

    async fn reconcilable(&self, count: i64) -> Vec<Uuid> {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Migration',$1::text)")
            .bind(organization)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Migration',$1::text)").bind(workspace).bind(organization).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO tasks(id,workspace_id,title,description,status) SELECT gen_random_uuid(),$1::uuid,'Migration','Regression','in_progress' FROM generate_series(1,$2::bigint)").bind(workspace).bind(count).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO task_runs(id,task_id,status,completed_at) SELECT gen_random_uuid(),id,'completed',NOW() FROM tasks WHERE workspace_id=$1").bind(workspace).execute(&self.pool).await.unwrap();
        sqlx::query("UPDATE tasks SET active_run_id=task_runs.id FROM task_runs WHERE task_runs.task_id=tasks.id AND tasks.workspace_id=$1").bind(workspace).execute(&self.pool).await.unwrap();
        sqlx::query_scalar("SELECT id FROM tasks WHERE workspace_id=$1 ORDER BY id")
            .bind(workspace)
            .fetch_all(&self.pool)
            .await
            .unwrap()
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
    // 030 and 031 retire the 'running'-only indexes once their supersets exist.
    let valid: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_index WHERE indexrelid IN ('task_runs_active_waiting'::regclass,'task_runs_heartbeat_waiting'::regclass) AND indisvalid").fetch_one(&database.pool).await.unwrap();
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

#[tokio::test]
async fn reconcile_skips_tasks_another_server_locked() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let tasks = database.reconcilable(2).await;
    let (held, free) = (tasks[0], tasks[1]);
    let mut holder = database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM tasks WHERE id=$1 FOR UPDATE")
        .bind(held)
        .execute(&mut *holder)
        .await
        .unwrap();

    let changed = tokio::time::timeout(
        Duration::from_secs(10),
        zone_server::db::recovery::reconcile(&database.pool),
    )
    .await
    .expect("reconcile waited on a task row another server had locked")
    .unwrap();
    assert_eq!(changed, 1, "reconcile did not skip the locked task");

    let skipped: (String, Option<Uuid>) =
        sqlx::query_as("SELECT status,active_run_id FROM tasks WHERE id=$1")
            .bind(held)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(skipped.0, "in_progress");
    assert!(skipped.1.is_some(), "a skipped task lost its active run");
    let reconciled: (String, Option<Uuid>) =
        sqlx::query_as("SELECT status,active_run_id FROM tasks WHERE id=$1")
            .bind(free)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(reconciled, ("review".to_string(), None));

    holder.rollback().await.unwrap();
    assert_eq!(
        zone_server::db::recovery::reconcile(&database.pool)
            .await
            .unwrap(),
        1,
        "a released task was never reconciled"
    );
    let released: (String, Option<Uuid>) =
        sqlx::query_as("SELECT status,active_run_id FROM tasks WHERE id=$1")
            .bind(held)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(released, ("review".to_string(), None));
    database.cleanup().await;
}

#[tokio::test]
async fn reconcile_bounds_each_batch() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let total = 105;
    database.reconcilable(total).await;

    let first = zone_server::db::recovery::reconcile(&database.pool)
        .await
        .unwrap();
    assert!(
        first < total as u64,
        "one pass reconciled all {total} tasks, so the batch holds unbounded task locks"
    );

    let mut drained = first;
    while drained < total as u64 {
        let pass = zone_server::db::recovery::reconcile(&database.pool)
            .await
            .unwrap();
        assert!(pass > 0, "reconciliation stalled at {drained} of {total}");
        drained += pass;
    }
    assert_eq!(drained, total as u64);
    assert_eq!(
        zone_server::db::recovery::reconcile(&database.pool)
            .await
            .unwrap(),
        0
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE active_run_id IS NOT NULL OR status<>'review'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(pending, 0, "tasks remained unreconciled after draining");
    database.cleanup().await;
}

#[tokio::test]
async fn waiting_runs_hold_the_admission_slot_the_running_index_used_to_hold() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let task = database.task().await;

    for legacy in ["task_runs_active", "task_runs_heartbeat"] {
        let surviving: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(format!("public.{legacy}"))
            .fetch_one(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            surviving, None,
            "{legacy} still indexes only running runs, so a parked run frees the slot"
        );
    }

    sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,'waiting')")
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    for competitor in ["waiting", "running"] {
        let rejected = sqlx::query("INSERT INTO task_runs(task_id,status) VALUES($1,$2)")
            .bind(task)
            .bind(competitor)
            .execute(&database.pool)
            .await
            .unwrap_err();
        assert_eq!(
            rejected.as_database_error().unwrap().code().as_deref(),
            Some("23505"),
            "a {competitor} run was admitted beside a parked one"
        );
    }
    database.cleanup().await;
}

#[tokio::test]
async fn a_run_stores_one_pending_question_and_only_known_statuses() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let task = database.task().await;
    let run = Uuid::new_v4();
    sqlx::query("INSERT INTO task_runs(id,task_id,status) VALUES($1,$2,'running')")
        .bind(run)
        .bind(task)
        .execute(&database.pool)
        .await
        .unwrap();
    let unasked: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT pending_question FROM task_runs WHERE id=$1")
            .bind(run)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(unasked, None, "a fresh run must not look like it is asking");

    let question = serde_json::json!({
        "tool_call_id": "call_1",
        "questions": [{"id": "colour", "prompt": "Which colour?", "required": true}],
    });
    sqlx::query("UPDATE task_runs SET status='waiting',pending_question=$2 WHERE id=$1")
        .bind(run)
        .bind(&question)
        .execute(&database.pool)
        .await
        .unwrap();
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT pending_question FROM task_runs WHERE id=$1")
            .bind(run)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(stored, question);

    // 'pending' is in the set because a query once selected for it; the check
    // has never admitted it, so no predicate may name it either.
    for unknown in ["parked", "pending"] {
        let rejected = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE task_runs SET status='{unknown}' WHERE id=$1"
        )))
        .bind(run)
        .execute(&database.pool)
        .await
        .unwrap_err();
        assert_eq!(
            rejected.as_database_error().unwrap().code().as_deref(),
            Some("23514"),
            "the widened status check must still be a closed set, and '{unknown}' is outside it"
        );
    }
    database.cleanup().await;
}

#[tokio::test]
async fn a_parked_run_keeps_its_lease_and_gives_it_back_once() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let task = database.task().await;
    let run = tasks::create_task_run(&database.pool, task).await.unwrap();
    let owner = Uuid::new_v4();
    assert!(
        tasks::claim_task_run(&database.pool, run.id, owner)
            .await
            .unwrap()
    );
    let question = serde_json::json!({"tool_call_id": "call_1", "questions": []});
    assert!(
        tasks::park_task_run(&database.pool, run.id, owner, question.clone())
            .await
            .unwrap()
    );
    assert!(
        !tasks::park_task_run(&database.pool, run.id, owner, question)
            .await
            .unwrap(),
        "parking an already parked run must not replace the question being answered"
    );

    // Without these the heartbeat loop cancels the whole pipeline within one tick.
    assert!(
        tasks::heartbeat_task_run(&database.pool, run.id, owner)
            .await
            .unwrap(),
        "a parked run lost its lease on the first heartbeat"
    );
    let execution = tasks::Execution {
        task,
        run: run.id,
        owner,
        actor: None,
    };
    assert!(
        execution.authorized(&database.pool, false).await.unwrap(),
        "a parked run lost its writer authorization"
    );
    assert!(
        tasks::owns_task_run(&database.pool, run.id, Some(owner))
            .await
            .unwrap()
    );
    assert!(
        tasks::add_owned_task_run_log(
            &database.pool,
            run.id,
            Some(owner),
            "acting",
            "tool",
            "info",
            "Waiting on an answer",
            None,
        )
        .await
        .unwrap(),
        "a parked run could not announce that it is stalled"
    );

    assert!(
        tasks::resume_task_run(&database.pool, run.id, owner)
            .await
            .unwrap()
    );
    assert!(
        !tasks::resume_task_run(&database.pool, run.id, owner)
            .await
            .unwrap(),
        "a second answer resumed the run twice"
    );
    let resumed = tasks::get_task_run(&database.pool, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.status, "running");
    assert_eq!(resumed.pending_question, None);
    database.cleanup().await;
}

#[tokio::test]
async fn the_sweeper_orphans_a_parked_run_whose_worker_died() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let task = database.task().await;
    let run = tasks::create_task_run(&database.pool, task).await.unwrap();
    let owner = Uuid::new_v4();
    assert!(
        tasks::claim_task_run(&database.pool, run.id, owner)
            .await
            .unwrap()
    );
    assert!(
        tasks::park_task_run(
            &database.pool,
            run.id,
            owner,
            serde_json::json!({"tool_call_id": "call_1", "questions": []}),
        )
        .await
        .unwrap()
    );
    assert_eq!(
        tasks::sweep_task_runs(&database.pool).await.unwrap(),
        0,
        "a waiter that is still heartbeating was swept out from under its question"
    );

    sqlx::query("UPDATE task_runs SET heartbeat_at=NOW()-INTERVAL '61 seconds' WHERE id=$1")
        .bind(run.id)
        .execute(&database.pool)
        .await
        .unwrap();
    assert_eq!(tasks::sweep_task_runs(&database.pool).await.unwrap(), 1);
    let swept = tasks::get_task_run(&database.pool, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(swept.status, "failed");
    assert_eq!(swept.error_message.as_deref(), Some("orphaned"));
    assert_eq!(
        swept.current_phase, None,
        "an orphaned run must not go on reading as waiting for an answer"
    );
    assert_eq!(
        swept.pending_question, None,
        "an orphaned run left an answerable card behind for a worker that is gone"
    );
    let active: Option<Uuid> = sqlx::query_scalar("SELECT active_run_id FROM tasks WHERE id=$1")
        .bind(task)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(active, None, "a swept waiter kept holding the task");

    tasks::create_task_run(&database.pool, task)
        .await
        .expect("the swept task must admit a new run");
    database.cleanup().await;
}

/// An interrupted `CREATE INDEX CONCURRENTLY` leaves the 027 name taken by an
/// invalid index. `IF NOT EXISTS` adopts it, records 027 over it, and 029 then
/// raises forever on an index no migration is left to rebuild.
#[tokio::test]
async fn task_migration_repairs_a_cancelled_waiting_index_build() {
    let database = Database::new().await;
    database.through(26).await;
    let task = database.task().await;
    sqlx::query(
        "INSERT INTO task_runs(task_id,status) SELECT $1,'completed' FROM generate_series(1,20000)",
    )
    .bind(task)
    .execute(&database.pool)
    .await
    .unwrap();

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
            let process: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_progress_create_index WHERE relid='task_runs'::regclass AND index_relid=to_regclass('task_runs_active_waiting')").fetch_optional(&database.pool).await.unwrap();
            if let Some(process) = process {
                break process;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    migrating.abort();
    assert!(migrating.await.unwrap_err().is_cancelled());
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
        "SELECT NOT indisvalid FROM pg_index WHERE indexrelid='task_runs_active_waiting'::regclass",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(
        invalid,
        "cancelled build must leave actual invalid-index recovery work"
    );
    let recorded: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=27)")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(!recorded);

    migrations::run(&database.pool).await.unwrap();

    let valid: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_index WHERE indexrelid IN ('task_runs_active_waiting'::regclass,'task_runs_heartbeat_waiting'::regclass) AND indisvalid").fetch_one(&database.pool).await.unwrap();
    assert_eq!(
        valid, 2,
        "the adopted leftover was recorded instead of rebuilt"
    );
    let validated: bool = sqlx::query_scalar(
        "SELECT convalidated FROM pg_constraint WHERE conname='task_runs_status_check'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(validated, "029 never got past its own guard");
    let recorded: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=29)")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(recorded);
    database.cleanup().await;
}

/// Every lock these take on `task_runs` is one ordinary traffic already holds,
/// and the boot holds sqlx's advisory lock while it queues for them: an
/// unbounded wait wedges every other instance instead of failing with 55P03.
#[test]
fn each_table_altering_migration_since_the_validation_bounds_its_lock_wait() {
    const FIRST: i64 = 21;
    const BOUND: &str = "SET LOCAL lock_timeout = '5s';";
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut bounded: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&directory).expect("migrations directory is readable") {
        let path = entry.expect("migration entry is readable").path();
        if !path.extension().is_some_and(|extension| extension == "sql") {
            continue;
        }
        let name = path
            .file_name()
            .expect("migration path has a file name")
            .to_string_lossy()
            .into_owned();
        let version: i64 = name
            .split('_')
            .next()
            .expect("a migration name opens with its version")
            .parse()
            .expect("a migration name opens with its version");
        let sql = std::fs::read_to_string(&path).expect("migration is readable");
        if version < FIRST
            || sql.lines().any(|line| line.trim() == "-- no-transaction")
            || !sql.to_ascii_uppercase().contains("ALTER TABLE")
        {
            continue;
        }
        let opening = sql
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("--"));
        assert_eq!(
            opening,
            Some(BOUND),
            "{name} alters a table inside sqlx's transaction without opening on `{BOUND}`, so it \
             queues behind any conflicting lock while the boot holds the migration advisory lock"
        );
        bounded.push(name);
    }
    bounded.sort();
    assert_eq!(
        bounded,
        [
            "021_task_validation.sql",
            "025_task_run_pending_question.sql",
            "026_task_run_waiting_status.sql",
            "029_task_run_waiting_validation.sql",
            "032_task_run_pending_wait.sql",
        ],
        "the set of table-altering migrations changed; a new one needs its own lock bound"
    );
}

/// sqlx hands a `-- no-transaction` file to the server as one simple query, and
/// a simple query carrying more than one statement runs inside an implicit
/// transaction block, which `CONCURRENTLY` is rejected in with 25001. Building
/// or dropping an index that way is the only reason any of these files gives up
/// sqlx's transaction, so both halves of the rule stand or fall together.
#[test]
fn each_no_transaction_migration_holds_one_concurrent_statement() {
    const MARKER: &str = "-- no-transaction";
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut checked: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&directory).expect("migrations directory is readable") {
        let path = entry.expect("migration entry is readable").path();
        if !path.extension().is_some_and(|extension| extension == "sql") {
            continue;
        }
        let sql = std::fs::read_to_string(&path).expect("migration is readable");
        if sql.lines().next().map(str::trim) != Some(MARKER) {
            continue;
        }
        let name = path
            .file_name()
            .expect("migration path has a file name")
            .to_string_lossy()
            .into_owned();
        let statements = sql
            .lines()
            .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
            .collect::<Vec<_>>()
            .join("\n");
        let count = statements
            .split(';')
            .filter(|statement| !statement.trim().is_empty())
            .count();
        assert_eq!(
            count, 1,
            "{name} opens on `{MARKER}`, so sqlx sends all {count} of its statements as one \
             batch: everything after the CONCURRENTLY one fails with 25001"
        );
        assert!(
            statements.to_ascii_uppercase().contains("CONCURRENTLY"),
            "{name} gives up sqlx's transaction without the CONCURRENTLY that is the only reason \
             to, so an interrupted run leaves it applied but unrecorded"
        );
        checked.push(name);
    }
    assert!(
        !checked.is_empty(),
        "no migration opens on `{MARKER}`; the walk stopped matching the files it guards"
    );
}
