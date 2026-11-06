use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::decrypt_value;
use abnegate_secret::encrypt_value;
use abnegate_secret::is_encrypted;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tempfile::NamedTempFile;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use uuid::Uuid;
use zone_server::db::migrations;
use zone_server::db::sealing::AiKey;
use zone_server::db::sealing::AiScope;
use zone_server::db::sealing::BATCH_SIZE;
use zone_server::db::sealing::Class;
use zone_server::db::sealing::Classified;
use zone_server::db::sealing::Column;
use zone_server::db::sealing::Options;
use zone_server::db::sealing::Outcome;
use zone_server::db::sealing::Report;
use zone_server::db::sealing::Sealer;
use zone_server::db::sealing::SealingError;
use zone_server::db::sealing::Table;
use zone_server::services::github_app::ApplicationId;
use zone_server::services::github_app::InstallationId;
use zone_server::services::github_app::StoredConfiguration;

/// Every test replays the whole migration chain into an empty database and dumps it.
static DATABASES: Semaphore = Semaphore::const_new(4);

const PG_DUMP: &str = "pg_dump";
const CUSTOM_FORMAT: &str = "-Fc";
const TAR_FORMAT: &str = "-Ft";
const DATA_ONLY: &str = "--data-only";
const ENVELOPE_OPENING: &str = "ENC[v1:";
const ENVELOPE_CLOSING: char = ']';
const SNAPSHOT_TABLES: &[&str] = &[
    "agent_logins",
    "organization_ai_settings",
    "projects",
    "sources",
    "sync_configs",
    "workspace_ai_settings",
];
const GITHUB: &str = "github";
const LINEAR: &str = "linear";
const PASSPHRASE: &str = "a-sealing-test-passphrase-of-at-least-32-bytes";
const MISTAKEN_PASSPHRASE: &str = "a-mistaken-sealing-passphrase-of-at-least-32-bytes";
const QUERY_LOG_TARGET: &str = "sqlx::query";
const REPORT_OPENING: &str = "Database ";
const BINARY_TIMEOUT: Duration = Duration::from_secs(120);
/// Enough of an archive's tail to cut into its last table's data and leave its contents whole.
const TRUNCATED_BYTES: usize = 8;

struct Scratch {
    admin: PgPool,
    pool: PgPool,
    name: String,
    _slot: SemaphorePermit<'static>,
}

struct Tenant {
    organization: Uuid,
    workspace: Uuid,
}

impl Scratch {
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
        let name = format!("seal_secrets_{}", Uuid::new_v4().simple());
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
        migrations::run(&pool).await.unwrap();
        Self {
            admin,
            pool,
            name,
            _slot: slot,
        }
    }

    /// The scratch database's URL in the form both sqlx and libpq read.
    fn url(&self) -> String {
        let mut url =
            reqwest::Url::parse(&std::env::var("TEST_DATABASE_URL").expect("disposable database"))
                .expect("a database URL");
        url.set_path(&self.name);
        url.to_string()
    }

    async fn backup(&self, excluded: &[&str]) -> NamedTempFile {
        let mut options = vec![CUSTOM_FORMAT.to_string()];
        options.extend(
            excluded
                .iter()
                .map(|table| format!("--exclude-table-data={table}")),
        );
        self.dump(&options).await
    }

    async fn dump(&self, options: &[String]) -> NamedTempFile {
        let archive = NamedTempFile::new().unwrap();
        let mut command = tokio::process::Command::new(PG_DUMP);
        command
            .args(options)
            .arg("-f")
            .arg(archive.path())
            .arg("-d")
            .arg(self.url())
            .env("LC_ALL", "C");
        let output = command.output().await.expect("pg_dump is installed");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        archive
    }

    async fn run(&self, key: &Arc<MasterKey>, dry_run: bool, backup: &Path) -> Report {
        self.attempt(key, dry_run, backup)
            .await
            .expect("the run completes")
    }

    async fn attempt(
        &self,
        key: &Arc<MasterKey>,
        dry_run: bool,
        backup: &Path,
    ) -> Result<Report, SealingError> {
        Sealer::new(
            self.pool.clone(),
            Arc::clone(key),
            Options::new(dry_run, backup.to_path_buf()),
        )
        .run()
        .await
    }

    async fn tenant(&self) -> Tenant {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Sealing',$1::text)")
            .bind(organization)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Sealing',$1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(&self.pool)
        .await
        .unwrap();
        Tenant {
            organization,
            workspace,
        }
    }

    async fn project(&self, tenant: &Tenant, token: Option<&str>) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects(id,workspace_id,name,github_access_token) VALUES($1,$2,'Sealing',$3)",
        )
        .bind(id)
        .bind(tenant.workspace)
        .bind(token)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    async fn source(&self, tenant: &Tenant, credentials: Option<&str>, config: &Value) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO sources(id,workspace_id,name,source_type,config,credentials_encrypted) VALUES($1,$2,$1::text,'github',$3::text::jsonb,$4)",
        )
        .bind(id)
        .bind(tenant.workspace)
        .bind(config.to_string())
        .bind(credentials)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    async fn sync_config(
        &self,
        tenant: &Tenant,
        provider: &str,
        config: &Value,
        webhook_secret: Option<&str>,
    ) -> Uuid {
        let project = self.project(tenant, None).await;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO sync_configs(id,project_id,provider,config,webhook_secret_encrypted) VALUES($1,$2,$3,$4::text::jsonb,$5)",
        )
        .bind(id)
        .bind(project)
        .bind(provider)
        .bind(config.to_string())
        .bind(webhook_secret)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    async fn agent_login(&self, tenant: &Tenant, credential: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_logins(id,organization_id,agent,credential) VALUES($1,$2,'claude',$3)",
        )
        .bind(id)
        .bind(tenant.organization)
        .bind(credential)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    async fn ai_settings(&self, tenant: &Tenant, scope: AiScope, value: &str) {
        let table = scope.table();
        let (owner, id) = match scope {
            AiScope::Organization => ("organization_id", tenant.organization),
            AiScope::Workspace => ("workspace_id", tenant.workspace),
        };
        let columns: Vec<&str> = AI_KEYS.iter().copied().map(AiKey::column).collect();
        let placeholders: Vec<String> = (2..columns.len() + 2)
            .map(|index| format!("${index}"))
            .collect();
        let sql = format!(
            "INSERT INTO {table}({owner},{}) VALUES($1,{})",
            columns.join(","),
            placeholders.join(",")
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(id);
        for key in AI_KEYS {
            query = query.bind(format!("{value}-{}", key.column()));
        }
        query.execute(&self.pool).await.unwrap();
    }

    async fn text(&self, column: Column, id: Uuid) -> Option<String> {
        let table = column.table();
        let name = column.name();
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {name} FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn config(&self, table: &str, id: Uuid) -> Value {
        let text: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT config::text FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    async fn snapshot(&self) -> Vec<String> {
        let mut snapshot = Vec::new();
        for table in SNAPSHOT_TABLES {
            let rows: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT coalesce(string_agg(row_to_json(t)::text, '|' ORDER BY id), '') FROM {table} t"
            )))
            .fetch_one(&self.pool)
            .await
            .unwrap();
            snapshot.push(rows);
        }
        snapshot
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .unwrap();
        self.admin.close().await;
    }
}

const AI_KEYS: &[AiKey] = &[
    AiKey::LitellmKey,
    AiKey::OpenaiApiKey,
    AiKey::AnthropicApiKey,
    AiKey::RunpodApiKey,
    AiKey::BedrockAccessKey,
    AiKey::BedrockSecretKey,
];

fn key() -> Arc<MasterKey> {
    Arc::new(MasterKey::generate().expect("a key"))
}

fn envelope(credential: &str, key: &MasterKey) -> String {
    encrypt_value(&SecretValue::new(credential), key).expect("a seal")
}

/// A value sealed before envelopes existed: the body of an envelope, bare.
fn legacy(credential: &str, key: &MasterKey) -> String {
    envelope(credential, key)
        .strip_prefix(ENVELOPE_OPENING)
        .and_then(|body| body.strip_suffix(ENVELOPE_CLOSING))
        .expect("an envelope")
        .to_string()
}

fn opened(stored: &str, key: &MasterKey) -> String {
    assert!(is_encrypted(stored), "{stored} is an envelope");
    decrypt_value(stored, key)
        .expect("the envelope opens")
        .expose()
        .to_string()
}

fn ambiguous() -> String {
    let mut bytes = [0_u8; 32];
    rand::fill(&mut bytes);
    STANDARD.encode(bytes)
}

fn github_app(private_key: &str) -> Value {
    json!({"github_app": {"application_id": 1, "installation_id": 2, "private_key": private_key}})
}

#[tokio::test]
async fn a_dry_run_counts_every_column_and_class_and_writes_nothing() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    scratch
        .source(&tenant, Some(&legacy("ghp_source", &key)), &json!({}))
        .await;
    scratch
        .source(&tenant, Some("ghp_plaintext_source"), &json!({}))
        .await;
    scratch
        .source(&tenant, Some(&envelope("ghp_sealed", &key)), &json!({}))
        .await;
    scratch
        .project(&tenant, Some("ghp_plaintext_project"))
        .await;
    scratch
        .project(&tenant, Some(&legacy("ghp_project", &key)))
        .await;
    scratch.project(&tenant, Some(&ambiguous())).await;
    scratch.project(&tenant, Some("")).await;
    scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"owner": "abnegate", "repo": "zone", "token": "ghp_sync"}),
            Some(&legacy("whsec", &key)),
        )
        .await;
    scratch
        .sync_config(
            &tenant,
            LINEAR,
            &json!({"api_key": "lin_api_plaintext", "team_id": "team"}),
            Some("linear-signing-secret"),
        )
        .await;
    scratch
        .agent_login(&tenant, &legacy("{\"access\":\"token\"}", &key))
        .await;
    scratch
        .ai_settings(&tenant, AiScope::Organization, "sk-organization")
        .await;
    scratch
        .ai_settings(&tenant, AiScope::Workspace, "sk-workspace")
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    let report = scratch.run(&key, true, backup.path()).await;

    assert_eq!(scratch.snapshot().await, before, "a dry run writes nothing");
    assert!(report.dry_run());
    assert_eq!(report.written(), 0);
    assert!(report.is_clean());
    let expected = [
        (
            Column::SourceCredentials,
            Class::Legacy,
            1,
            Outcome::Resealed,
        ),
        (
            Column::SourceCredentials,
            Class::Plaintext,
            1,
            Outcome::Reported,
        ),
        (
            Column::SourceCredentials,
            Class::Envelope,
            1,
            Outcome::Skipped,
        ),
        (Column::ProjectToken, Class::Plaintext, 1, Outcome::Sealed),
        (Column::ProjectToken, Class::Legacy, 1, Outcome::Resealed),
        (Column::ProjectToken, Class::Ambiguous, 1, Outcome::Reported),
        (Column::ProjectToken, Class::Empty, 1, Outcome::Skipped),
        (
            Column::GithubSyncToken,
            Class::Plaintext,
            1,
            Outcome::Sealed,
        ),
        (Column::WebhookSecret, Class::Legacy, 1, Outcome::Resealed),
        (
            Column::WebhookSecret,
            Class::Plaintext,
            1,
            Outcome::Reported,
        ),
        (
            Column::AgentLoginCredential,
            Class::Legacy,
            1,
            Outcome::Resealed,
        ),
        (Column::LinearApiKey, Class::Plaintext, 1, Outcome::Reported),
    ];
    for (column, class, count, outcome) in expected {
        assert_eq!(report.count(column, class), count, "{column} {class}");
        assert_eq!(
            report.outcome(column, outcome),
            count,
            "{column} {class} is {outcome}"
        );
    }
    for scope in [AiScope::Organization, AiScope::Workspace] {
        for ai_key in AI_KEYS {
            let column = Column::AiSetting {
                scope,
                key: *ai_key,
            };
            assert_eq!(report.count(column, Class::Plaintext), 1, "{column}");
            assert_eq!(report.outcome(column, Outcome::Reported), 1, "{column}");
        }
    }

    let printed = report.to_string();
    assert!(printed.contains("Dry run"), "{printed}");
    for column in Column::ALL {
        assert!(printed.contains(&column.to_string()), "{printed}");
    }
    assert!(!printed.contains("ghp_plaintext_project"), "{printed}");
    assert!(!printed.contains("sk-organization"), "{printed}");
    scratch.cleanup().await;
}

#[tokio::test]
async fn legacy_values_are_resealed_in_every_sealed_column() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let source = scratch
        .source(&tenant, Some(&legacy("ghp_source", &key)), &json!({}))
        .await;
    let project = scratch
        .project(&tenant, Some(&legacy("ghp_project", &key)))
        .await;
    let sync = scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"owner": "abnegate", "token": legacy("ghp_sync", &key)}),
            Some(&legacy("whsec", &key)),
        )
        .await;
    let login = scratch
        .agent_login(&tenant, &legacy("{\"access\":\"token\"}", &key))
        .await;
    let backup = scratch.backup(&[]).await;

    let report = scratch.run(&key, false, backup.path()).await;

    let expected = [
        (Column::SourceCredentials, source, "ghp_source"),
        (Column::ProjectToken, project, "ghp_project"),
        (Column::WebhookSecret, sync, "whsec"),
        (Column::GithubSyncToken, sync, "ghp_sync"),
        (
            Column::AgentLoginCredential,
            login,
            "{\"access\":\"token\"}",
        ),
    ];
    for (column, id, credential) in expected {
        let stored = scratch.text(column, id).await.expect("a value");
        assert_eq!(opened(&stored, &key), credential, "{column}");
        assert_eq!(report.outcome(column, Outcome::Resealed), 1, "{column}");
    }
    assert_eq!(report.written(), 5);
    assert!(report.is_clean());
    scratch.cleanup().await;
}

#[tokio::test]
async fn plaintext_is_sealed_only_in_lenient_columns() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let project = scratch.project(&tenant, Some("ghp_project_token")).await;
    let sync = scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"owner": "abnegate", "repo": "zone", "direction": "bidirectional", "token": "ghp_sync_token"}),
            None,
        )
        .await;
    let backup = scratch.backup(&[]).await;

    let report = scratch.run(&key, false, backup.path()).await;

    let token = scratch
        .text(Column::ProjectToken, project)
        .await
        .expect("a token");
    assert_eq!(opened(&token, &key), "ghp_project_token");
    let config = scratch.config("sync_configs", sync).await;
    assert_eq!(
        opened(config["token"].as_str().expect("a token"), &key),
        "ghp_sync_token"
    );
    assert_eq!(config["owner"], "abnegate");
    assert_eq!(config["repo"], "zone");
    assert_eq!(config["direction"], "bidirectional");
    assert_eq!(report.outcome(Column::ProjectToken, Outcome::Sealed), 1);
    assert_eq!(report.outcome(Column::GithubSyncToken, Outcome::Sealed), 1);
    assert_eq!(report.written(), 2);
    scratch.cleanup().await;
}

#[tokio::test]
async fn plaintext_in_strict_and_mixed_columns_is_reported_and_unchanged() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let source = scratch
        .source(&tenant, Some("ghp_plaintext_source"), &json!({}))
        .await;
    let login = scratch
        .agent_login(&tenant, "{\"access\":\"plaintext\"}")
        .await;
    let sync = scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({}),
            Some("plaintext-webhook-secret"),
        )
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(scratch.snapshot().await, before);
    assert_eq!(
        scratch
            .text(Column::SourceCredentials, source)
            .await
            .as_deref(),
        Some("ghp_plaintext_source")
    );
    assert_eq!(
        scratch
            .text(Column::AgentLoginCredential, login)
            .await
            .as_deref(),
        Some("{\"access\":\"plaintext\"}")
    );
    assert_eq!(
        scratch.text(Column::WebhookSecret, sync).await.as_deref(),
        Some("plaintext-webhook-secret")
    );
    for column in [
        Column::SourceCredentials,
        Column::AgentLoginCredential,
        Column::WebhookSecret,
    ] {
        assert_eq!(report.count(column, Class::Plaintext), 1, "{column}");
        assert_eq!(report.outcome(column, Outcome::Reported), 1, "{column}");
    }
    assert_eq!(report.written(), 0);
    scratch.cleanup().await;
}

#[tokio::test]
async fn an_ambiguous_value_is_reported_and_unchanged() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    scratch
        .source(&tenant, Some(&envelope("ghp_source", &key)), &json!({}))
        .await;
    let base64 = ambiguous();
    let shaped = scratch.project(&tenant, Some(&base64)).await;
    let unclosed = scratch.project(&tenant, Some("ENC[v1:abc")).await;
    let sync = scratch
        .sync_config(&tenant, GITHUB, &json!({"token": ambiguous()}), None)
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(scratch.snapshot().await, before);
    assert_eq!(
        scratch.text(Column::ProjectToken, shaped).await.as_deref(),
        Some(base64.as_str())
    );
    assert_eq!(
        scratch
            .text(Column::ProjectToken, unclosed)
            .await
            .as_deref(),
        Some("ENC[v1:abc")
    );
    assert_eq!(report.count(Column::ProjectToken, Class::Ambiguous), 2);
    assert_eq!(report.outcome(Column::ProjectToken, Outcome::Reported), 2);
    assert_eq!(report.count(Column::GithubSyncToken, Class::Ambiguous), 1);
    assert!(scratch.config("sync_configs", sync).await["token"].is_string());
    assert_eq!(report.written(), 0);
    scratch.cleanup().await;
}

#[tokio::test]
async fn envelopes_are_skipped_byte_for_byte() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    scratch
        .source(&tenant, Some(&envelope("ghp_source", &key)), &json!({}))
        .await;
    scratch
        .project(&tenant, Some(&envelope("ghp_project", &key)))
        .await;
    scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"token": envelope("ghp_sync", &key)}),
            Some(&envelope("whsec", &key)),
        )
        .await;
    scratch
        .agent_login(&tenant, &envelope("{\"access\":\"token\"}", &key))
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(scratch.snapshot().await, before);
    for column in [
        Column::SourceCredentials,
        Column::ProjectToken,
        Column::WebhookSecret,
        Column::GithubSyncToken,
        Column::AgentLoginCredential,
    ] {
        assert_eq!(report.count(column, Class::Envelope), 1, "{column}");
        assert_eq!(report.outcome(column, Outcome::Skipped), 1, "{column}");
    }
    assert_eq!(report.written(), 0);
    scratch.cleanup().await;
}

#[tokio::test]
async fn a_second_run_writes_nothing() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    scratch
        .source(&tenant, Some(&legacy("ghp_source", &key)), &json!({}))
        .await;
    scratch.project(&tenant, Some("ghp_project")).await;
    scratch
        .project(&tenant, Some(&legacy("ghp_legacy_project", &key)))
        .await;
    scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"token": "ghp_sync"}),
            Some(&legacy("whsec", &key)),
        )
        .await;
    scratch
        .agent_login(&tenant, &legacy("{\"access\":\"token\"}", &key))
        .await;
    let backup = scratch.backup(&[]).await;

    let first = scratch.run(&key, false, backup.path()).await;
    assert_eq!(first.written(), 6);
    let sealed = scratch.snapshot().await;

    let second = scratch.run(&key, false, backup.path()).await;

    assert_eq!(second.written(), 0);
    assert_eq!(scratch.snapshot().await, sealed);
    assert_eq!(second.count(Column::ProjectToken, Class::Envelope), 2);
    scratch.cleanup().await;
}

#[tokio::test]
async fn an_interrupted_run_resumes() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let rows = BATCH_SIZE * 2 + 1;
    for index in 0..rows {
        scratch
            .project(&tenant, Some(&legacy(&format!("ghp_{index}"), &key)))
            .await;
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE github_access_token IS NOT NULL ORDER BY id",
    )
    .fetch_all(&scratch.pool)
    .await
    .unwrap();
    assert_eq!(ids.len(), rows);
    let (committed, unsealed) = ids.split_at(BATCH_SIZE);
    // Identifiers are a UUID the test generated.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION interrupt() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'interrupted'; END $$; \
         CREATE TRIGGER interrupt BEFORE UPDATE ON projects FOR EACH ROW \
         WHEN (OLD.id = '{}') EXECUTE FUNCTION interrupt();",
        unsealed[1]
    )))
    .execute(&scratch.pool)
    .await
    .unwrap();
    let backup = scratch.backup(&[]).await;

    let interrupted = scratch.attempt(&key, false, backup.path()).await;

    assert!(
        matches!(interrupted, Err(SealingError::Database(_))),
        "the run stops in its second batch: {interrupted:?}"
    );
    for id in committed {
        let stored = scratch
            .text(Column::ProjectToken, *id)
            .await
            .expect("a token");
        assert!(
            opened(&stored, &key).starts_with("ghp_"),
            "the first batch committed"
        );
    }
    for id in unsealed {
        let stored = scratch
            .text(Column::ProjectToken, *id)
            .await
            .expect("a token");
        assert!(!is_encrypted(&stored), "the second batch rolled back");
    }

    sqlx::raw_sql("DROP TRIGGER interrupt ON projects; DROP FUNCTION interrupt();")
        .execute(&scratch.pool)
        .await
        .unwrap();
    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(
        report.outcome(Column::ProjectToken, Outcome::Resealed),
        unsealed.len() as u64
    );
    assert_eq!(
        report.outcome(Column::ProjectToken, Outcome::Skipped),
        committed.len() as u64
    );
    for id in &ids {
        let stored = scratch
            .text(Column::ProjectToken, *id)
            .await
            .expect("a token");
        assert!(opened(&stored, &key).starts_with("ghp_"));
    }
    let second = scratch.run(&key, false, backup.path()).await;
    assert_eq!(second.written(), 0);
    scratch.cleanup().await;
}

#[tokio::test]
async fn a_stale_compare_and_swap_writes_nothing() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let stored = legacy("ghp_project", &key);
    let project = scratch.project(&tenant, Some(&stored)).await;
    let sealed = envelope("ghp_project", &key);
    let mut connection = scratch.pool.acquire().await.unwrap();

    assert!(
        !Column::ProjectToken
            .replace(&mut connection, project, "an outdated value", &sealed)
            .await
            .unwrap()
    );
    assert_eq!(
        scratch.text(Column::ProjectToken, project).await,
        Some(stored)
    );

    let sync = scratch
        .sync_config(
            &tenant,
            GITHUB,
            &json!({"token": "ghp_old", "hook": 1}),
            None,
        )
        .await;
    sqlx::query(
        "UPDATE sync_configs SET config = '{\"token\": \"ghp_new\", \"hook\": 2}'::jsonb WHERE id = $1",
    )
    .bind(sync)
    .execute(&scratch.pool)
    .await
    .unwrap();
    assert!(
        !Column::GithubSyncToken
            .replace(&mut connection, sync, "ghp_old", &envelope("ghp_old", &key))
            .await
            .unwrap(),
        "a token another writer changed is left alone"
    );
    assert_eq!(
        scratch.config("sync_configs", sync).await,
        json!({"token": "ghp_new", "hook": 2})
    );

    sqlx::query(
        "UPDATE sync_configs SET config = '{\"token\": \"ghp_new\", \"hook\": 3}'::jsonb WHERE id = $1",
    )
    .bind(sync)
    .execute(&scratch.pool)
    .await
    .unwrap();
    let resealed = envelope("ghp_new", &key);
    assert!(
        Column::GithubSyncToken
            .replace(&mut connection, sync, "ghp_new", &resealed)
            .await
            .unwrap()
    );
    assert_eq!(
        scratch.config("sync_configs", sync).await,
        json!({"token": resealed, "hook": 3}),
        "sealing the token keeps every other change to the config"
    );

    assert!(matches!(
        Column::LinearApiKey
            .replace(&mut connection, sync, "lin_api", &sealed)
            .await,
        Err(SealingError::Counted(Column::LinearApiKey))
    ));
    drop(connection);
    scratch.cleanup().await;
}

#[tokio::test]
async fn counted_columns_are_counted_and_never_written() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    scratch
        .ai_settings(&tenant, AiScope::Organization, "sk-organization")
        .await;
    scratch
        .ai_settings(&tenant, AiScope::Workspace, "sk-workspace")
        .await;
    scratch
        .sync_config(
            &tenant,
            LINEAR,
            &json!({"api_key": "lin_api_plaintext", "team_id": "team"}),
            None,
        )
        .await;
    scratch
        .source(
            &tenant,
            None,
            &github_app(&envelope("-----BEGIN PRIVATE KEY-----", &key)),
        )
        .await;
    scratch
        .source(&tenant, None, &github_app("-----BEGIN PRIVATE KEY-----"))
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(scratch.snapshot().await, before);
    for scope in [AiScope::Organization, AiScope::Workspace] {
        for ai_key in AI_KEYS {
            let column = Column::AiSetting {
                scope,
                key: *ai_key,
            };
            assert_eq!(report.count(column, Class::Plaintext), 1, "{column}");
            assert_eq!(report.outcome(column, Outcome::Reported), 1, "{column}");
        }
    }
    assert_eq!(report.count(Column::LinearApiKey, Class::Plaintext), 1);
    assert_eq!(report.outcome(Column::LinearApiKey, Outcome::Reported), 1);
    assert_eq!(
        report.count(Column::GithubAppPrivateKey, Class::Envelope),
        1
    );
    assert_eq!(
        report.outcome(Column::GithubAppPrivateKey, Outcome::Skipped),
        1
    );
    assert_eq!(
        report.outcome(Column::GithubAppPrivateKey, Outcome::Reported),
        1
    );
    assert_eq!(report.written(), 0);
    scratch.cleanup().await;
}

#[tokio::test]
async fn the_backup_gate_refuses_anything_but_a_full_archive_of_this_database() {
    let scratch = Scratch::new().await;
    let other = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let project = scratch.project(&tenant, Some("ghp_project")).await;
    let directory = TempDir::new().unwrap();
    let empty = directory.path().join("empty.dump");
    tokio::fs::write(&empty, b"").await.unwrap();
    let text = directory.path().join("text.dump");
    tokio::fs::write(&text, b"not an archive").await.unwrap();
    let foreign = other.backup(&[]).await;
    let partial = scratch.backup(&["agent_logins"]).await;
    let incomplete = scratch.backup(&["organizations"]).await;
    let full = scratch.backup(&[]).await;
    let archived = tokio::fs::read(full.path()).await.unwrap();
    let truncated = directory.path().join("truncated.dump");
    tokio::fs::write(&truncated, &archived[..archived.len() - TRUNCATED_BYTES])
        .await
        .unwrap();
    let tar = scratch.dump(&[TAR_FORMAT.to_string()]).await;
    let data_only = scratch
        .dump(&[CUSTOM_FORMAT.to_string(), DATA_ONLY.to_string()])
        .await;

    for dry_run in [true, false] {
        assert!(matches!(
            scratch
                .attempt(&key, dry_run, &directory.path().join("missing.dump"))
                .await,
            Err(SealingError::BackupMissing { .. })
        ));
        assert!(matches!(
            scratch.attempt(&key, dry_run, directory.path()).await,
            Err(SealingError::BackupNotFile { .. })
        ));
        assert!(matches!(
            scratch.attempt(&key, dry_run, &empty).await,
            Err(SealingError::BackupEmpty { .. })
        ));
        assert!(matches!(
            scratch.attempt(&key, dry_run, &text).await,
            Err(SealingError::NotAnArchive { .. })
        ));
        match scratch.attempt(&key, dry_run, tar.path()).await {
            Err(SealingError::NotCustomFormat { format, .. }) => assert_eq!(format, "TAR"),
            result => panic!("a pg_dump -Ft archive is refused: {result:?}"),
        }
        match scratch.attempt(&key, dry_run, foreign.path()).await {
            Err(SealingError::DatabaseMismatch { expected, found }) => {
                assert_eq!(expected, scratch.name);
                assert_eq!(found, other.name);
            }
            result => panic!("an archive of another database is refused: {result:?}"),
        }
        match scratch.attempt(&key, dry_run, partial.path()).await {
            Err(SealingError::TableMissing { table }) => {
                assert_eq!(table, Table::new("public", "agent_logins"))
            }
            result => panic!("an archive without agent_logins rows is refused: {result:?}"),
        }
        match scratch.attempt(&key, dry_run, incomplete.path()).await {
            Err(SealingError::TableMissing { table }) => {
                assert_eq!(table, Table::new("public", "organizations"))
            }
            result => panic!(
                "an archive without the rows of a table sealing never writes is refused: {result:?}"
            ),
        }
        assert!(
            matches!(
                scratch.attempt(&key, dry_run, &truncated).await,
                Err(SealingError::Unreadable { .. })
            ),
            "an archive whose contents list but whose data stops short is refused"
        );
        assert!(
            matches!(
                scratch.attempt(&key, dry_run, data_only.path()).await,
                Err(SealingError::TableMissing { .. })
            ),
            "a data-only archive, which defines no table, is refused"
        );
    }

    sqlx::raw_sql("CREATE SCHEMA archive; CREATE TABLE archive.notes (body text);")
        .execute(&scratch.pool)
        .await
        .unwrap();
    let outside_public = scratch.backup(&["archive.notes"]).await;
    for dry_run in [true, false] {
        match scratch.attempt(&key, dry_run, outside_public.path()).await {
            Err(SealingError::TableMissing { table }) => {
                assert_eq!(table, Table::new("archive", "notes"))
            }
            result => panic!(
                "an archive without the rows of a table outside public is refused: {result:?}"
            ),
        }
    }

    assert_eq!(
        scratch.text(Column::ProjectToken, project).await.as_deref(),
        Some("ghp_project")
    );
    other.cleanup().await;
    scratch.cleanup().await;
}

#[tokio::test]
async fn a_key_that_opens_nothing_writes_nothing() {
    let scratch = Scratch::new().await;
    let deployment = key();
    let mistaken = key();
    let tenant = scratch.tenant().await;
    scratch
        .source(
            &tenant,
            Some(&legacy("ghp_source", &deployment)),
            &json!({}),
        )
        .await;
    scratch.project(&tenant, Some("ghp_project")).await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    for dry_run in [true, false] {
        assert!(
            matches!(
                scratch.attempt(&mistaken, dry_run, backup.path()).await,
                Err(SealingError::KeyMismatch { unopened: 1 })
            ),
            "a run under the wrong key is refused, dry or not: {dry_run}"
        );
    }
    assert_eq!(scratch.snapshot().await, before);

    let sealed = scratch.run(&deployment, false, backup.path()).await;
    assert_eq!(sealed.written(), 2);
    scratch.cleanup().await;
}

fn binary(url: &str, encryption_key: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_zone-server"));
    command
        .env_clear()
        .env("DATABASE_URL", url)
        .env("ENCRYPTION_KEY", encryption_key)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .kill_on_drop(true);
    command
}

async fn finished(command: &mut tokio::process::Command) -> std::process::Output {
    tokio::time::timeout(BINARY_TIMEOUT, command.output())
        .await
        .expect("the seal mode finishes")
        .expect("the server runs")
}

#[tokio::test]
async fn the_seal_mode_runs_from_the_binary() {
    let scratch = Scratch::new().await;
    let tenant = scratch.tenant().await;
    let project = scratch.project(&tenant, Some("ghp_project")).await;
    let backup = scratch.backup(&[]).await;
    let archive: PathBuf = backup.path().to_path_buf();

    let dry = finished(
        binary(&scratch.url(), PASSPHRASE)
            .env("RUST_LOG", "sqlx=debug")
            .arg("--seal-secrets")
            .arg("--dry-run")
            .arg("--backup")
            .arg(&archive),
    )
    .await;
    let stdout = String::from_utf8_lossy(&dry.stdout);
    let stderr = String::from_utf8_lossy(&dry.stderr);
    assert!(dry.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("Dry run"), "{stdout}");
    assert!(
        stdout.starts_with(REPORT_OPENING) && !stdout.contains(QUERY_LOG_TARGET),
        "the report is alone on stdout: {stdout}"
    );
    assert!(
        stderr.contains(QUERY_LOG_TARGET),
        "logs go to stderr: {stderr}"
    );
    assert!(stdout.contains("projects.github_access_token"), "{stdout}");
    assert!(
        stdout.contains("organization_ai_settings.litellm_key"),
        "{stdout}"
    );
    assert_eq!(
        scratch.text(Column::ProjectToken, project).await.as_deref(),
        Some("ghp_project")
    );

    let unguarded = finished(
        binary(&scratch.url(), PASSPHRASE)
            .arg("--seal-secrets")
            .arg("--dry-run"),
    )
    .await;
    assert!(!unguarded.status.success());

    let sealed = finished(
        binary(&scratch.url(), PASSPHRASE)
            .arg("--seal-secrets")
            .arg("--backup")
            .arg(&archive),
    )
    .await;
    assert!(
        sealed.status.success(),
        "{}",
        String::from_utf8_lossy(&sealed.stderr)
    );
    let key = zone_server::secret::derive(PASSPHRASE).expect("a key");
    let stored = scratch
        .text(Column::ProjectToken, project)
        .await
        .expect("a token");
    assert_eq!(
        opened(&stored, &key),
        "ghp_project",
        "the binary seals under the key the server derives"
    );
    scratch.cleanup().await;
}

#[tokio::test]
async fn the_seal_mode_refuses_a_short_encryption_key() {
    let scratch = Scratch::new().await;
    let backup = scratch.backup(&[]).await;

    let output = finished(
        binary(&scratch.url(), "too-short")
            .arg("--seal-secrets")
            .arg("--dry-run")
            .arg("--backup")
            .arg(backup.path()),
    )
    .await;

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ENCRYPTION_KEY must be at least 32 bytes"),
        "{stderr}"
    );
    scratch.cleanup().await;
}

#[tokio::test]
async fn legacy_project_tokens_that_open_under_no_key_refuse_the_run() {
    let scratch = Scratch::new().await;
    let deployment = key();
    let mistaken = key();
    let tenant = scratch.tenant().await;
    let sealed = scratch
        .project(&tenant, Some(&legacy("ghp_legacy_project", &deployment)))
        .await;
    let plaintext = scratch
        .project(&tenant, Some("ghp_plaintext_project"))
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    for dry_run in [true, false] {
        assert!(
            matches!(
                scratch.attempt(&mistaken, dry_run, backup.path()).await,
                Err(SealingError::KeyMismatch { unopened: 1 })
            ),
            "plaintext is not sealed under a key the legacy token does not open under: {dry_run}"
        );
    }
    assert_eq!(scratch.snapshot().await, before, "nothing was written");

    let report = scratch.run(&deployment, false, backup.path()).await;
    assert_eq!(report.written(), 2);
    for (id, credential) in [
        (sealed, "ghp_legacy_project"),
        (plaintext, "ghp_plaintext_project"),
    ] {
        let stored = scratch
            .text(Column::ProjectToken, id)
            .await
            .expect("a token");
        assert_eq!(opened(&stored, &deployment), credential);
    }
    scratch.cleanup().await;
}

#[tokio::test]
async fn a_key_that_opens_only_counted_columns_writes_nothing() {
    let scratch = Scratch::new().await;
    let deployment = key();
    let mistaken = key();
    let tenant = scratch.tenant().await;
    let stored = StoredConfiguration::seal(
        ApplicationId::new(1),
        InstallationId::new(2),
        &SecretValue::new("-----BEGIN PRIVATE KEY-----"),
        &mistaken,
    )
    .expect("a sealed configuration");
    let mut config = json!({"owner": "abnegate"});
    stored.write(&mut config).expect("a written configuration");
    scratch.source(&tenant, None, &config).await;
    scratch
        .project(&tenant, Some(&legacy("ghp_legacy_project", &deployment)))
        .await;
    scratch
        .project(&tenant, Some("ghp_plaintext_project"))
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    for dry_run in [true, false] {
        assert!(
            matches!(
                scratch.attempt(&mistaken, dry_run, backup.path()).await,
                Err(SealingError::KeyMismatch { unopened: 1 })
            ),
            "a key that opens only a counted column does not seal the columns it writes: {dry_run}"
        );
    }
    assert_eq!(scratch.snapshot().await, before, "nothing was written");
    scratch.cleanup().await;
}

#[tokio::test]
async fn an_unopenable_envelope_in_a_written_column_is_reported_and_unchanged() {
    let scratch = Scratch::new().await;
    let key = key();
    let foreign = envelope("ghp_foreign_source", &self::key());
    let tenant = scratch.tenant().await;
    let source = scratch.source(&tenant, Some(&foreign), &json!({})).await;
    let project = scratch
        .project(&tenant, Some(&legacy("ghp_project", &key)))
        .await;
    let backup = scratch.backup(&[]).await;

    let report = scratch.run(&key, false, backup.path()).await;

    assert_eq!(
        scratch.text(Column::SourceCredentials, source).await,
        Some(foreign)
    );
    assert_eq!(
        report.count(Column::SourceCredentials, Class::Unopenable),
        1
    );
    assert_eq!(
        report.outcome(Column::SourceCredentials, Outcome::Reported),
        1
    );
    let token = scratch
        .text(Column::ProjectToken, project)
        .await
        .expect("a token");
    assert_eq!(opened(&token, &key), "ghp_project");
    assert_eq!(report.written(), 1);
    assert!(report.is_clean());
    scratch.cleanup().await;
}

#[tokio::test]
async fn the_github_app_key_is_read_from_a_stored_configuration() {
    let scratch = Scratch::new().await;
    let key = key();
    let tenant = scratch.tenant().await;
    let private_key = SecretValue::new("-----BEGIN PRIVATE KEY-----");
    let stored = StoredConfiguration::seal(
        ApplicationId::new(1),
        InstallationId::new(2),
        &private_key,
        &key,
    )
    .expect("a sealed configuration");
    let mut config = json!({"owner": "abnegate"});
    stored.write(&mut config).expect("a written configuration");
    let source = scratch.source(&tenant, None, &config).await;

    let rows = Column::GithubAppPrivateKey
        .page(&scratch.pool, None, BATCH_SIZE)
        .await
        .unwrap();

    match rows.as_slice() {
        [(id, value)] => {
            assert_eq!(*id, source);
            assert_eq!(Classified::of(value, &key).class(), Class::Envelope);
            assert_eq!(opened(value, &key), private_key.expose());
        }
        rows => panic!("one private key is read: {} rows", rows.len()),
    }
    scratch.cleanup().await;
}

#[tokio::test]
async fn the_seal_mode_fails_under_the_wrong_key() {
    let scratch = Scratch::new().await;
    let deployment = zone_server::secret::derive(PASSPHRASE).expect("a key");
    let tenant = scratch.tenant().await;
    scratch
        .project(&tenant, Some(&legacy("ghp_legacy_project", &deployment)))
        .await;
    scratch
        .project(&tenant, Some("ghp_plaintext_project"))
        .await;
    let backup = scratch.backup(&[]).await;
    let before = scratch.snapshot().await;

    for dry_run in [true, false] {
        let mut command = binary(&scratch.url(), MISTAKEN_PASSPHRASE);
        command.arg("--seal-secrets");
        if dry_run {
            command.arg("--dry-run");
        }
        let output = finished(command.arg("--backup").arg(backup.path())).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{dry_run}: {stderr}");
        assert!(stderr.contains("ENCRYPTION_KEY"), "{dry_run}: {stderr}");
        assert!(output.stdout.is_empty(), "{dry_run}: no report is printed");
    }
    assert_eq!(scratch.snapshot().await, before, "nothing was written");
    scratch.cleanup().await;
}
