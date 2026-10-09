use sqlx::migrate::MigrateError;
use sqlx::{ConnectOptions, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};
use uuid::Uuid;
use zone_server::db::{ai_settings, memory, migrations, tasks};

/// Every test replays the whole chain into an empty database; more at once
/// only queue on the same disk writes, and the tests' own time bounds with them.
static DATABASES: Semaphore = Semaphore::const_new(4);

struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
    _slot: SemaphorePermit<'static>,
}

impl Database {
    async fn new() -> Self {
        let slot = DATABASES
            .acquire()
            .await
            .expect("the database slots are never closed");
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
        Self {
            admin,
            pool,
            name,
            _slot: slot,
        }
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

    async fn workspace(&self) -> (Uuid, Uuid) {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let owner = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Migration',$1::text)")
            .bind(organization)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Migration',$1::text)").bind(workspace).bind(organization).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES($1,$1::text,'migration')")
            .bind(owner)
            .execute(&self.pool)
            .await
            .unwrap();
        (workspace, owner)
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
async fn a_dirty_ledger_refuses_to_migrate() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET success=false WHERE version=1")
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(matches!(
        migrations::run(&database.pool).await,
        Err(MigrateError::Dirty(1))
    ));
    database.cleanup().await;
}

#[tokio::test]
async fn a_checksum_mismatch_refuses_to_migrate() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum='\\x00'::bytea WHERE version=1")
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(matches!(
        migrations::run(&database.pool).await,
        Err(MigrateError::VersionMismatch(1))
    ));
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
            sqlx::query("UPDATE _sqlx_migrations SET checksum='\\x00'::bytea WHERE version=1")
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

#[tokio::test]
async fn agent_providers_and_logins_are_accepted() {
    const CHECK_VIOLATION: &str = "23514";
    const AGENT_PROVIDERS: [&str; 2] = ["claude_code", "codex"];
    const ORGANIZATION_PROVIDER: &str =
        "UPDATE organization_ai_settings SET provider=$2 WHERE organization_id=$1";
    const WORKSPACE_PROVIDER: &str =
        "UPDATE workspace_ai_settings SET provider=$2 WHERE workspace_id=$1";
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let (workspace, _) = database.workspace().await;
    let organization: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id=$1")
            .bind(workspace)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO organization_ai_settings(organization_id,provider) VALUES($1,'bedrock')",
    )
    .bind(organization)
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO workspace_ai_settings(workspace_id) VALUES($1)")
        .bind(workspace)
        .execute(&database.pool)
        .await
        .unwrap();

    for (statement, target) in [
        (ORGANIZATION_PROVIDER, organization),
        (WORKSPACE_PROVIDER, workspace),
    ] {
        for provider in AGENT_PROVIDERS {
            sqlx::query(statement)
                .bind(target)
                .bind(provider)
                .execute(&database.pool)
                .await
                .unwrap_or_else(|error| panic!("{statement} must take {provider}: {error}"));
        }
        let refused = sqlx::query(statement)
            .bind(target)
            .bind("gemini")
            .execute(&database.pool)
            .await
            .expect_err("the provider check must remain a closed set");
        assert_eq!(
            refused.as_database_error().unwrap().code().as_deref(),
            Some(CHECK_VIOLATION)
        );
    }
    sqlx::query("INSERT INTO agent_logins(organization_id,agent) VALUES($1,'codex')")
        .bind(organization)
        .execute(&database.pool)
        .await
        .expect("an organization's sign-in is kept in agent_logins");
    database.cleanup().await;
}

#[tokio::test]
async fn failed_review_verdicts_and_unlink_events_are_accepted() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();

    let verdict: bool = sqlx::query_scalar(
        "SELECT convalidated FROM pg_constraint \
         WHERE conrelid = 'task_reviews'::regclass AND conname = 'task_reviews_verdict_check'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(verdict, "task_reviews_verdict_check must be validated");

    let event_type: bool = sqlx::query_scalar(
        "SELECT convalidated FROM pg_constraint \
         WHERE conrelid = 'sync_events'::regclass AND conname = 'sync_events_event_type_check'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(event_type, "sync_events_event_type_check must be validated");

    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname='task_reviews_verdict_check'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(
        definition.contains("failed"),
        "verdict check must admit failed: {definition}"
    );

    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname='sync_events_event_type_check'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(
        definition.contains("unlink"),
        "event type check must admit unlink: {definition}"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn ai_settings_default_to_unrouted_completions() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let (workspace, _) = database.workspace().await;
    let organization: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id=$1")
            .bind(workspace)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO organization_ai_settings(organization_id,provider,openai_api_key) \
         VALUES($1,'openai','sk-saved-long-ago')",
    )
    .bind(organization)
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workspace_ai_settings(workspace_id,litellm_host) \
         VALUES($1,'http://localhost:11434')",
    )
    .bind(workspace)
    .execute(&database.pool)
    .await
    .unwrap();
    let routed: (bool, bool) = sqlx::query_as(
        "SELECT o.completions_routed, w.completions_routed \
         FROM organization_ai_settings o, workspace_ai_settings w \
         WHERE o.organization_id=$1 AND w.workspace_id=$2",
    )
    .bind(organization)
    .bind(workspace)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(routed, (false, false));
    let settings = ai_settings::get_effective_ai_settings(&database.pool, organization, workspace)
        .await
        .unwrap();
    assert_eq!(settings.provider, "openai");
    assert!(
        settings.openai_api_key.is_none() && settings.litellm_host.is_none(),
        "an unrouted row must not lend its endpoint to completions"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn a_memory_entry_name_is_unique_per_person() {
    const NAME: &str = "Deploy window";
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let (workspace, owner) = database.workspace().await;
    let entry = "INSERT INTO knowledge_entries(workspace_id,created_by,category,title,content) VALUES($1,$2,$3,$4,'Body')";
    sqlx::query(entry)
        .bind(workspace)
        .bind(owner)
        .bind(memory::FACT_CATEGORY)
        .bind(NAME)
        .execute(&database.pool)
        .await
        .expect("the first write of a name must be taken");
    let refused = sqlx::query(entry)
        .bind(workspace)
        .bind(owner)
        .bind(memory::FACT_CATEGORY)
        .bind(NAME)
        .execute(&database.pool)
        .await
        .unwrap_err();
    assert_eq!(
        refused.as_database_error().unwrap().code().as_deref(),
        Some("23505"),
        "a second entry took a name the first already holds"
    );
    database.cleanup().await;
}

#[tokio::test]
async fn workspace_theme_defaults_match_the_product() {
    let database = Database::new().await;
    migrations::run(&database.pool).await.unwrap();
    let (workspace, _) = database.workspace().await;
    sqlx::query("INSERT INTO workspace_themes(workspace_id) VALUES($1)")
        .bind(workspace)
        .execute(&database.pool)
        .await
        .unwrap();
    let theme: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT primary_color_light, secondary_color_light, primary_color_dark, \
         secondary_color_dark, font_family, border_radius \
         FROM workspace_themes WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(
        theme,
        (
            "#0011d9".into(),
            "#ecf9ff".into(),
            "#00f3ff".into(),
            "#ecf9ff".into(),
            "nunito".into(),
            "large".into()
        )
    );
    database.cleanup().await;
}

/// 004 adds the chat's login key under the brief lock of an unvalidated
/// constraint, and 005 proves the rows while chats stay writable. A login kept
/// before them keeps its id, and an organization may then hold a second one.
#[tokio::test]
async fn the_chat_login_key_is_added_unvalidated_and_proven_after() {
    const VALIDATED: &str = "SELECT convalidated FROM pg_constraint \
         WHERE conrelid = 'chats'::regclass AND conname = 'chats_agent_login_id_fkey'";
    const SIGN_IN: &str =
        "INSERT INTO agent_logins(organization_id,agent) VALUES($1,'claude') RETURNING id";
    let database = Database::new().await;
    database.through(2).await;
    let (workspace, _) = database.workspace().await;
    let organization: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id=$1")
            .bind(workspace)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    let kept: Uuid = sqlx::query_scalar(SIGN_IN)
        .bind(organization)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    sqlx::query(SIGN_IN)
        .bind(organization)
        .fetch_one(&database.pool)
        .await
        .expect_err("the initial schema keeps one login per agent");

    database.through(4).await;
    let added: bool = sqlx::query_scalar(VALIDATED)
        .fetch_one(&database.pool)
        .await
        .expect("004 adds the chat's login key");
    database.through(5).await;
    let proven: bool = sqlx::query_scalar(VALIDATED)
        .fetch_one(&database.pool)
        .await
        .expect("005 keeps the chat's login key");

    assert!(!added, "004 must not scan chats under its exclusive lock");
    assert!(proven, "005 validates what 004 added");
    let logins: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM agent_logins WHERE organization_id=$1")
            .bind(organization)
            .fetch_all(&database.pool)
            .await
            .unwrap();
    assert_eq!(logins, [kept], "004 must keep the login saved before it");
    sqlx::query(SIGN_IN)
        .bind(organization)
        .fetch_one(&database.pool)
        .await
        .expect("004 lets an organization keep a second login of one agent");
    database.cleanup().await;
}

/// Every lock a later migration takes on a live table is one ordinary traffic
/// already holds, and the boot holds sqlx's advisory lock while it queues for
/// them: an unbounded wait wedges every other instance instead of failing with
/// 55P03. The initial schema creates an empty database, so it is exempt. So is
/// a migration released without the bound: every database that ran it records
/// its checksum, and adding the bound now would stop each of them at boot.
#[test]
fn each_table_altering_migration_after_the_initial_schema_bounds_its_lock_wait() {
    const FIRST: i64 = 2;
    const RELEASED: [&str; 2] = ["002_chat_offline.sql", "003_chat_context_tokens.sql"];
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
            || RELEASED.contains(&name.as_str())
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
            "004_agent_login_usage.sql",
            "005_agent_login_usage_validation.sql",
            "006_runpod_api_key.sql",
            "007_devices.sql",
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
    }
}
