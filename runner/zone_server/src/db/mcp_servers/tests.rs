use std::collections::BTreeMap;
use std::time::Duration;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::Transaction;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::services::mcp_servers::transport::Transport;

use super::*;

const ENVELOPE_OPENING: &str = "ENC[";
const TOKEN: &str = "ghp_mcp_server_token";
const AUTHORIZATION: &str = "Bearer mcp-header-secret";
const POLL: Duration = Duration::from_millis(20);
const PATIENCE: Duration = Duration::from_secs(20);
const BLOCKED: &str = "SELECT $2 = ANY(pg_blocking_pids($1))";
const BACKEND: &str = "SELECT pg_backend_pid()";

fn database() -> String {
    std::env::var("TEST_DATABASE_URL").expect("isolated test database")
}

async fn pool() -> PgPool {
    PgPool::connect(&database())
        .await
        .expect("the test database accepts connections")
}

async fn session() -> (PgPool, i32) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database())
        .await
        .expect("the test database accepts connections");
    let backend = sqlx::query_scalar(BACKEND)
        .fetch_one(&pool)
        .await
        .expect("the session's backend");
    (pool, backend)
}

async fn holder(transaction: &mut Transaction<'_, Postgres>) -> i32 {
    sqlx::query_scalar(BACKEND)
        .fetch_one(&mut **transaction)
        .await
        .expect("the transaction's backend")
}

/// Returns once `backend` waits on a lock that `blocker` holds, or is queued for ahead of it.
async fn waits(pool: &PgPool, backend: i32, blocker: i32) {
    tokio::time::timeout(PATIENCE, async {
        while !sqlx::query_scalar::<_, bool>(BLOCKED)
            .bind(backend)
            .bind(blocker)
            .fetch_one(pool)
            .await
            .expect("the sessions blocking the backend")
        {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .expect("the session waits on the blocker's lock");
}

struct Fixture {
    pool: PgPool,
    key: MasterKey,
    organization: Uuid,
    workspace: Uuid,
    owner: Uuid,
    admin: Uuid,
    member: Uuid,
    outsider: Uuid,
}

async fn organization(pool: &PgPool) -> Uuid {
    let organization = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Servers', $1::text)")
        .bind(organization)
        .execute(pool)
        .await
        .expect("an organization");
    organization
}

async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
    let workspace = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, 'Servers', $1::text)",
    )
    .bind(workspace)
    .bind(organization)
    .execute(pool)
    .await
    .expect("a workspace");
    workspace
}

async fn user(pool: &PgPool) -> Uuid {
    let user = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, $1::text || '@zone.test', 'test-only')",
    )
    .bind(user)
    .execute(pool)
    .await
    .expect("a user");
    user
}

async fn join_organization(pool: &PgPool, organization: Uuid, user: Uuid, role: OrgRole) {
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, $3)",
    )
    .bind(organization)
    .bind(user)
    .bind(role.as_str())
    .execute(pool)
    .await
    .expect("an organization membership");
}

async fn join_workspace(pool: &PgPool, workspace: Uuid, user: Uuid, role: WorkspaceRole) {
    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(workspace)
        .bind(user)
        .bind(role.as_str())
        .execute(pool)
        .await
        .expect("a workspace membership");
}

async fn tenant() -> Fixture {
    let pool = pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let owner = user(&pool).await;
    let admin = user(&pool).await;
    let member = user(&pool).await;
    let outsider = user(&pool).await;
    join_organization(&pool, organization, owner, OrgRole::Owner).await;
    join_organization(&pool, organization, admin, OrgRole::Admin).await;
    join_organization(&pool, organization, member, OrgRole::Member).await;
    Fixture {
        pool,
        key: MasterKey::generate().expect("a key"),
        organization,
        workspace,
        owner,
        admin,
        member,
        outsider,
    }
}

fn secrets(entries: &[(&str, &str)]) -> BTreeMap<String, SecretValue> {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), SecretValue::new(*value)))
        .collect()
}

fn stdio(name: &str) -> NewMcpServer {
    NewMcpServer {
        name: name.to_owned(),
        transport: Transport::Stdio.as_str().to_owned(),
        command: Some("npx".to_owned()),
        arguments: vec![
            "-y".to_owned(),
            "@modelcontextprotocol/server-github".to_owned(),
        ],
        url: None,
        tools: None,
        enabled: true,
        environment: BTreeMap::new(),
        headers: BTreeMap::new(),
    }
}

fn http(name: &str) -> NewMcpServer {
    NewMcpServer {
        name: name.to_owned(),
        transport: Transport::Http.as_str().to_owned(),
        command: None,
        arguments: Vec::new(),
        url: Some("http://127.0.0.1:9000/mcp".to_owned()),
        tools: Some(vec!["search".to_owned()]),
        enabled: true,
        environment: secrets(&[("GITHUB_TOKEN", TOKEN)]),
        headers: secrets(&[("Authorization", AUTHORIZATION)]),
    }
}

impl Fixture {
    async fn create(&self, new: NewMcpServer) -> McpServerRecord {
        create_authorized(&self.pool, &self.key, self.organization, self.admin, new)
            .await
            .expect("an admin creates a server")
    }

    async fn update(
        &self,
        server: Uuid,
        changes: McpServerChanges,
    ) -> McpServerResult<McpServerRecord> {
        update_authorized(
            &self.pool,
            &self.key,
            self.organization,
            server,
            self.admin,
            changes,
        )
        .await
    }

    async fn stored(&self, server: Uuid) -> Option<String> {
        sqlx::query_scalar("SELECT secrets FROM mcp_servers WHERE id = $1")
            .bind(server)
            .fetch_one(&self.pool)
            .await
            .expect("the server's row")
    }

    async fn read(&self, server: Uuid) -> McpServerRecord {
        get_authorized(
            &self.pool,
            &self.key,
            self.organization,
            server,
            self.member,
        )
        .await
        .expect("a member reads the server")
    }

    async fn version(&self, server: Uuid) -> String {
        sqlx::query_scalar("SELECT xmin::text FROM mcp_servers WHERE id = $1")
            .bind(server)
            .fetch_one(&self.pool)
            .await
            .expect("the row's version")
    }

    async fn replace_secrets(&self, server: Uuid, sealed: &str) {
        sqlx::query("UPDATE mcp_servers SET secrets = $2 WHERE id = $1")
            .bind(server)
            .bind(sealed)
            .execute(&self.pool)
            .await
            .expect("the stored secrets are replaced");
    }

    async fn overrides(
        &self,
        pool: &PgPool,
        changes: BTreeMap<Uuid, Option<bool>>,
    ) -> McpServerResult<BTreeMap<Uuid, bool>> {
        set_workspace_overrides_authorized(
            pool,
            self.organization,
            self.workspace,
            self.admin,
            changes,
        )
        .await
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE organization_id = $1"
        )))
        .bind(self.organization)
        .fetch_one(&self.pool)
        .await
        .expect("a count")
    }
}

fn exposed(map: &BTreeMap<String, SecretValue>) -> BTreeMap<&str, &str> {
    map.iter()
        .map(|(name, value)| (name.as_str(), value.expose()))
        .collect()
}

#[tokio::test]
async fn secrets_round_trip_sealed() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;

    let stored = fixture.stored(created.id).await.expect("sealed secrets");
    assert!(stored.starts_with(ENVELOPE_OPENING), "{stored}");
    assert!(!stored.contains(TOKEN));
    assert!(!stored.contains(AUTHORIZATION));

    let read = get_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.member,
    )
    .await
    .expect("a member reads the server");
    assert_eq!(
        exposed(&read.secrets.environment),
        BTreeMap::from([("GITHUB_TOKEN", TOKEN)])
    );
    assert_eq!(
        exposed(&read.secrets.headers),
        BTreeMap::from([("Authorization", AUTHORIZATION)])
    );
    assert_eq!(read.secrets, created.secrets);
    assert_eq!(read.url.as_deref(), Some("http://127.0.0.1:9000/mcp"));
    assert_eq!(read.tools, Some(vec!["search".to_owned()]));
    assert_eq!(read.created_by, Some(fixture.admin));

    let loaded = for_organization(&fixture.pool, &fixture.key, fixture.organization)
        .await
        .expect("the hub loader reads the servers");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].secrets, created.secrets);
}

#[tokio::test]
async fn no_secrets_store_null() {
    let fixture = tenant().await;
    let created = fixture.create(stdio("github")).await;

    assert_eq!(fixture.stored(created.id).await, None);
    assert_eq!(created.secrets, Secrets::default());
    assert_eq!(
        created.arguments,
        ["-y", "@modelcontextprotocol/server-github"]
    );
}

fn sealed_document(secrets: &Secrets) -> String {
    let key = MasterKey::generate().expect("a key");
    let sealed = seal(secrets, &key)
        .expect("secrets that seal")
        .expect("a sealed document");
    assert!(sealed.starts_with(ENVELOPE_OPENING), "{sealed}");
    assert_eq!(
        &open(Uuid::new_v4(), Some(&sealed), &key).expect("a document that opens"),
        secrets
    );
    key.open_strict(&sealed)
        .expect("an envelope that opens")
        .expose()
        .to_owned()
}

#[test]
fn the_sealed_document_keeps_its_bytes() {
    assert_eq!(
        sealed_document(&Secrets {
            environment: secrets(&[
                ("NPM_TOKEN", "npm \"quoted\" \\ and\nbroken é"),
                ("GITHUB_TOKEN", TOKEN),
                ("A\tTAB", ""),
            ]),
            headers: secrets(&[("X-Api-Key", "key"), ("Authorization", AUTHORIZATION)]),
        }),
        concat!(
            r#"{"environment":{"A\tTAB":"","GITHUB_TOKEN":"ghp_mcp_server_token","#,
            r#""NPM_TOKEN":"npm \"quoted\" \\ and\nbroken é"},"#,
            r#""headers":{"Authorization":"Bearer mcp-header-secret","X-Api-Key":"key"}}"#,
        )
    );
    assert_eq!(
        sealed_document(&Secrets {
            environment: secrets(&[("GITHUB_TOKEN", TOKEN)]),
            headers: BTreeMap::new(),
        }),
        r#"{"environment":{"GITHUB_TOKEN":"ghp_mcp_server_token"},"headers":{}}"#
    );
    assert_eq!(
        sealed_document(&Secrets {
            environment: BTreeMap::new(),
            headers: secrets(&[("Authorization", AUTHORIZATION)]),
        }),
        r#"{"environment":{},"headers":{"Authorization":"Bearer mcp-header-secret"}}"#
    );
}

#[test]
fn a_document_sealed_by_an_earlier_build_opens() {
    let key = MasterKey::generate().expect("a key");
    let server = Uuid::new_v4();
    let sealed = |document: &str| {
        encrypt_value(&SecretValue::new(document), &key).expect("a document that seals")
    };

    let both = sealed(concat!(
        r#"{"environment":{"GITHUB_TOKEN":"ghp_mcp_server_token"},"#,
        r#""headers":{"Authorization":"Bearer mcp-header-secret"}}"#,
    ));
    assert_eq!(
        open(server, Some(&both), &key).expect("a document that opens"),
        Secrets {
            environment: secrets(&[("GITHUB_TOKEN", TOKEN)]),
            headers: secrets(&[("Authorization", AUTHORIZATION)]),
        }
    );

    let without_headers = sealed(r#"{"environment":{"GITHUB_TOKEN":"ghp_mcp_server_token"}}"#);
    assert_eq!(
        open(server, Some(&without_headers), &key).expect("a document that opens"),
        Secrets {
            environment: secrets(&[("GITHUB_TOKEN", TOKEN)]),
            headers: BTreeMap::new(),
        }
    );
}

#[tokio::test]
async fn secrets_sealed_under_another_key_do_not_open() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let other = MasterKey::generate().expect("a key");

    let error = get_authorized(
        &fixture.pool,
        &other,
        fixture.organization,
        created.id,
        fixture.member,
    )
    .await
    .expect_err("another key opens nothing");

    assert!(matches!(&error, McpServerError::Sealing(_)), "{error:?}");
    assert!(!error.to_string().contains(TOKEN), "{error}");
}

#[tokio::test]
async fn secrets_that_do_not_open_name_the_server_and_never_the_sealed_text() {
    let fixture = tenant().await;
    let broken = fixture.create(http("docs")).await;
    let healthy = fixture.create(http("wiki")).await;
    let other = MasterKey::generate().expect("a key");
    let foreign = encrypt_value(&SecretValue::new("{}"), &other).expect("an envelope");
    fixture.replace_secrets(broken.id, &foreign).await;
    let expected = format!(
        "The secrets of MCP server {} did not open: Decryption failed: invalid key or corrupted value",
        broken.id
    );

    let listed = list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.member,
    )
    .await
    .expect_err("one unopenable server fails the list");
    assert!(
        matches!(&listed, McpServerError::Sealing(message) if *message == expected),
        "{listed:?}"
    );
    assert!(!listed.to_string().contains(&foreign), "{listed}");

    let read = get_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        broken.id,
        fixture.member,
    )
    .await
    .expect_err("the server does not open");
    assert_eq!(read.to_string(), expected);
    let updated = fixture
        .update(
            broken.id,
            McpServerChanges {
                enabled: Some(false),
                ..McpServerChanges::default()
            },
        )
        .await
        .map(|_| ())
        .expect_err("an unopenable server is not updated");
    assert_eq!(updated.to_string(), expected);
    assert!(fixture.read(healthy.id).await.enabled);

    let unreadable = encrypt_value(&SecretValue::new(TOKEN), &fixture.key).expect("an envelope");
    fixture.replace_secrets(broken.id, &unreadable).await;
    let malformed = fixture
        .update(broken.id, McpServerChanges::default())
        .await
        .map(|_| ())
        .expect_err("secrets that are not the sealed document do not open");
    assert_eq!(
        malformed.to_string(),
        format!(
            "The stored secrets of MCP server {} are malformed",
            broken.id
        )
    );
    assert!(!malformed.to_string().contains(TOKEN), "{malformed}");
}

#[tokio::test]
async fn the_loader_leaves_out_an_unopenable_server_that_still_fails_the_list() {
    let fixture = tenant().await;
    let broken = fixture.create(http("docs")).await;
    let healthy = fixture.create(http("wiki")).await;
    let other = MasterKey::generate().expect("a key");
    let foreign = encrypt_value(&SecretValue::new("{}"), &other).expect("an envelope");
    fixture.replace_secrets(broken.id, &foreign).await;

    let loaded = for_organization(&fixture.pool, &fixture.key, fixture.organization)
        .await
        .expect("the hub loader reads the servers that open");
    let names: Vec<&str> = loaded.iter().map(|server| server.name.as_str()).collect();
    assert_eq!(names, ["wiki"]);
    assert_eq!(loaded[0].secrets, healthy.secrets);

    let listed = list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.member,
    )
    .await;
    assert!(
        matches!(listed, Err(McpServerError::Sealing(_))),
        "{listed:?}"
    );

    delete_authorized(
        &fixture.pool,
        fixture.organization,
        broken.id,
        fixture.admin,
    )
    .await
    .expect("an admin deletes a server whose secrets do not open");
    let listed = list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.member,
    )
    .await
    .expect("the list opens again");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn header_names_that_differ_only_by_case_are_invalid() {
    let fixture = tenant().await;
    let expected = "`headers` names `authorization` twice: header names ignore case";

    let mut new = http("docs");
    new.headers = secrets(&[
        ("Authorization", AUTHORIZATION),
        ("authorization", "second"),
    ]);
    let created = create_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.admin,
        new,
    )
    .await;
    assert!(
        matches!(&created, Err(McpServerError::Invalid(message)) if message == expected),
        "{created:?}"
    );
    assert_eq!(fixture.count("mcp_servers").await, 0);

    let created = fixture.create(http("docs")).await;
    let sealed = fixture.stored(created.id).await;
    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                headers: Some(BTreeMap::from([
                    ("Authorization".to_owned(), None),
                    ("authorization".to_owned(), Some(SecretValue::new("second"))),
                ])),
                ..McpServerChanges::default()
            },
        )
        .await
        .map(|_| ());
    assert!(
        matches!(&updated, Err(McpServerError::Invalid(message)) if message == expected),
        "{updated:?}"
    );
    assert_eq!(fixture.stored(created.id).await, sealed);
}

#[tokio::test]
async fn only_administrators_create() {
    let fixture = tenant().await;

    let member = create_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.member,
        stdio("member"),
    )
    .await;
    assert!(
        matches!(
            member,
            Err(McpServerError::Forbidden(ORGANIZATION_FORBIDDEN))
        ),
        "{member:?}"
    );

    let outsider = create_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.outsider,
        stdio("outsider"),
    )
    .await;
    assert!(
        matches!(
            outsider,
            Err(McpServerError::NotFound(ORGANIZATION_NOT_FOUND))
        ),
        "{outsider:?}"
    );

    let owner = create_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.owner,
        stdio("owner"),
    )
    .await;
    assert_eq!(owner.expect("an owner creates a server").name, "owner");
    fixture.create(stdio("admin")).await;

    assert!(
        authorize_admin(&fixture.pool, fixture.organization, fixture.admin)
            .await
            .is_ok()
    );
    assert!(matches!(
        authorize_admin(&fixture.pool, fixture.organization, fixture.member).await,
        Err(McpServerError::Forbidden(_))
    ));
    assert_eq!(fixture.count("mcp_servers").await, 2);
}

#[tokio::test]
async fn members_read_and_outsiders_do_not() {
    let fixture = tenant().await;
    fixture.create(stdio("beta")).await;
    fixture.create(stdio("alpha")).await;

    let listed = list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.member,
    )
    .await
    .expect("a member lists the servers");
    let names: Vec<&str> = listed.iter().map(|server| server.name.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);

    let outsider = list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.outsider,
    )
    .await;
    assert!(
        matches!(
            outsider,
            Err(McpServerError::NotFound(ORGANIZATION_NOT_FOUND))
        ),
        "{outsider:?}"
    );
}

#[tokio::test]
async fn a_duplicate_name_conflicts() {
    let fixture = tenant().await;
    fixture.create(stdio("github")).await;

    let duplicate = create_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.admin,
        stdio("github"),
    )
    .await;

    assert!(
        matches!(duplicate, Err(McpServerError::Conflict(DUPLICATE_NAME))),
        "{duplicate:?}"
    );
}

#[tokio::test]
async fn a_renamed_duplicate_conflicts() {
    let fixture = tenant().await;
    fixture.create(stdio("github")).await;
    let other = fixture.create(stdio("gitlab")).await;

    let renamed = fixture
        .update(
            other.id,
            McpServerChanges {
                name: Some("github".to_owned()),
                ..McpServerChanges::default()
            },
        )
        .await;

    assert!(
        matches!(renamed, Err(McpServerError::Conflict(DUPLICATE_NAME))),
        "{renamed:?}"
    );
}

#[tokio::test]
async fn the_schema_refuses_invalid_rows() {
    let fixture = tenant().await;
    let cases = [
        (stdio(&"a".repeat(33)), INVALID_NAME),
        (stdio("double__underscore"), INVALID_NAME),
        (stdio("trailing_"), INVALID_NAME),
        (
            NewMcpServer {
                transport: "websocket".to_owned(),
                ..http("socket")
            },
            INVALID_TRANSPORT,
        ),
        (
            NewMcpServer {
                transport: Transport::Sse.as_str().to_owned(),
                ..http("events")
            },
            INVALID_TRANSPORT,
        ),
        (
            NewMcpServer {
                url: Some("http://127.0.0.1/mcp".to_owned()),
                ..stdio("both")
            },
            INVALID_TARGET,
        ),
    ];
    for (new, message) in cases {
        let name = new.name.clone();
        let error = create_authorized(
            &fixture.pool,
            &fixture.key,
            fixture.organization,
            fixture.admin,
            new,
        )
        .await
        .expect_err("the row is refused");
        assert!(
            matches!(&error, McpServerError::Invalid(text) if text == message),
            "{name}: {error:?}"
        );
    }
    fixture.create(stdio(&"a".repeat(32))).await;
}

#[tokio::test]
async fn a_check_the_module_does_not_know_stays_a_database_error() {
    let pool = pool().await;
    let mut connection = pool.acquire().await.expect("a connection");
    sqlx::query(
        "CREATE TEMPORARY TABLE probes (value integer CONSTRAINT probes_value_check CHECK (value > 0))",
    )
    .execute(&mut *connection)
    .await
    .expect("a table with a check");
    let refused = sqlx::query("INSERT INTO probes (value) VALUES (0)")
        .execute(&mut *connection)
        .await
        .expect_err("the check refuses the row");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("probes_value_check")
    );

    let classified = classify(refused);

    assert!(
        matches!(classified, McpServerError::Database(_)),
        "{classified:?}"
    );
}

#[tokio::test]
async fn an_update_that_changes_nothing_writes_nothing() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let version = fixture.version(created.id).await;
    let sealed = fixture.stored(created.id).await;

    for changes in [
        McpServerChanges::default(),
        McpServerChanges {
            name: Some(created.name.clone()),
            command: None,
            arguments: Some(created.arguments.clone()),
            url: created.url.clone(),
            tools: Some(created.tools.clone()),
            enabled: Some(created.enabled),
            environment: Some(BTreeMap::from([("GITHUB_TOKEN".to_owned(), None)])),
            headers: Some(BTreeMap::from([(
                "Authorization".to_owned(),
                Some(SecretValue::new(AUTHORIZATION)),
            )])),
        },
    ] {
        fixture
            .update(created.id, changes)
            .await
            .expect("an admin repeats the stored values");

        assert_eq!(fixture.version(created.id).await, version);
        assert_eq!(fixture.stored(created.id).await, sealed);
        let read = fixture.read(created.id).await;
        assert_eq!(read.updated_at, created.updated_at);
        assert_eq!(read.secrets, created.secrets);
    }
}

#[tokio::test]
async fn every_real_change_moves_updated_at() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let cases = [
        (
            "tools",
            McpServerChanges {
                tools: Some(None),
                ..McpServerChanges::default()
            },
        ),
        (
            "enabled",
            McpServerChanges {
                enabled: Some(false),
                ..McpServerChanges::default()
            },
        ),
        (
            "a secret value",
            McpServerChanges {
                environment: Some(BTreeMap::from([(
                    "GITHUB_TOKEN".to_owned(),
                    Some(SecretValue::new("rotated")),
                )])),
                ..McpServerChanges::default()
            },
        ),
        (
            "a removed header",
            McpServerChanges {
                headers: Some(BTreeMap::new()),
                ..McpServerChanges::default()
            },
        ),
        (
            "name",
            McpServerChanges {
                name: Some("wiki".to_owned()),
                ..McpServerChanges::default()
            },
        ),
        (
            "url",
            McpServerChanges {
                url: Some("http://127.0.0.1:9001/mcp".to_owned()),
                ..McpServerChanges::default()
            },
        ),
    ];

    let mut previous = created.updated_at;
    let mut unmoved = Vec::new();
    for (changed, changes) in cases {
        fixture
            .update(created.id, changes)
            .await
            .expect("an admin changes the server");
        let moved = fixture.read(created.id).await.updated_at;
        if moved <= previous {
            unmoved.push(changed);
        }
        previous = moved;
    }
    assert_eq!(unmoved, Vec::<&str>::new());

    let read = fixture.read(created.id).await;
    assert_eq!(read.name, "wiki");
    assert_eq!(read.tools, None);
    assert!(!read.enabled);
    assert_eq!(
        exposed(&read.secrets.environment),
        BTreeMap::from([("GITHUB_TOKEN", "rotated")])
    );
    assert!(read.secrets.headers.is_empty());
}

#[tokio::test]
async fn a_change_moves_updated_at_past_a_stored_value_ahead_of_the_clock() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let ahead: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE mcp_servers SET updated_at = now() + interval '1 hour' WHERE id = $1 \
         RETURNING updated_at",
    )
    .bind(created.id)
    .fetch_one(&fixture.pool)
    .await
    .expect("the stored revision is moved ahead");

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                enabled: Some(false),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin turns the server off");

    assert!(
        updated.updated_at > ahead,
        "{} did not move past {ahead}",
        updated.updated_at
    );
    assert_eq!(
        updated.updated_at - ahead,
        chrono::Duration::microseconds(1)
    );
    assert_eq!(
        fixture.read(created.id).await.updated_at,
        updated.updated_at
    );
}

#[tokio::test]
async fn a_change_beside_repeated_secrets_does_not_reseal_them() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let sealed = fixture.stored(created.id).await;

    fixture
        .update(
            created.id,
            McpServerChanges {
                enabled: Some(false),
                environment: Some(BTreeMap::from([("GITHUB_TOKEN".to_owned(), None)])),
                headers: Some(BTreeMap::from([(
                    "Authorization".to_owned(),
                    Some(SecretValue::new(AUTHORIZATION)),
                )])),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin turns the server off");

    assert!(!fixture.read(created.id).await.enabled);
    assert_eq!(fixture.stored(created.id).await, sealed);
}

#[tokio::test]
async fn two_updates_of_one_server_keep_both_secrets() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let (one, first) = session().await;
    let (two, second) = session().await;
    let mut holding = fixture.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM mcp_servers WHERE id = $1 FOR UPDATE")
        .bind(created.id)
        .execute(&mut *holding)
        .await
        .expect("the server is held");
    let held = holder(&mut holding).await;

    let environment = update_authorized(
        &one,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.admin,
        McpServerChanges {
            tools: Some(None),
            environment: Some(BTreeMap::from([
                ("GITHUB_TOKEN".to_owned(), None),
                ("ADDED".to_owned(), Some(SecretValue::new("first"))),
            ])),
            ..McpServerChanges::default()
        },
    );
    let headers = async {
        waits(&fixture.pool, first, held).await;
        update_authorized(
            &two,
            &fixture.key,
            fixture.organization,
            created.id,
            fixture.owner,
            McpServerChanges {
                tools: Some(Some(vec!["fetch".to_owned()])),
                headers: Some(BTreeMap::from([
                    ("Authorization".to_owned(), None),
                    ("X-Added".to_owned(), Some(SecretValue::new("second"))),
                ])),
                ..McpServerChanges::default()
            },
        )
        .await
    };
    let release = async {
        waits(&fixture.pool, second, first).await;
        holding.commit().await.expect("the server is released");
    };
    let (environment, headers, ()) = tokio::join!(environment, headers, release);

    environment.expect("the first update lands");
    headers.expect("the second update lands");
    let read = fixture.read(created.id).await;
    assert_eq!(
        exposed(&read.secrets.environment),
        BTreeMap::from([("ADDED", "first"), ("GITHUB_TOKEN", TOKEN)])
    );
    assert_eq!(
        exposed(&read.secrets.headers),
        BTreeMap::from([("Authorization", AUTHORIZATION), ("X-Added", "second")])
    );
    assert!(
        read.tools.is_none() || read.tools == Some(vec!["fetch".to_owned()]),
        "{:?}",
        read.tools
    );
}

#[tokio::test]
async fn an_update_changes_only_the_given_fields() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                enabled: Some(false),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin updates the server");

    assert!(!updated.enabled);
    assert_eq!(updated.name, created.name);
    assert_eq!(updated.url, created.url);
    assert_eq!(updated.tools, created.tools);
    assert_eq!(updated.secrets, created.secrets);
    assert!(fixture.stored(created.id).await.is_some());
    assert!(updated.updated_at > created.updated_at);
    let read = get_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.member,
    )
    .await
    .expect("the server");
    assert_eq!(read.secrets, created.secrets);
    assert!(!read.enabled);
}

#[tokio::test]
async fn a_tools_change_leaves_the_secrets_sealed_as_they_were() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let sealed = fixture.stored(created.id).await;

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                tools: Some(None),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin updates the tools");

    assert_eq!(updated.tools, None);
    assert_eq!(
        fixture.stored(created.id).await,
        sealed,
        "the secrets are not resealed"
    );
}

#[tokio::test]
async fn an_arguments_change_keeps_the_command() {
    let fixture = tenant().await;
    let created = fixture.create(stdio("github")).await;

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                arguments: Some(vec!["--read-only".to_owned()]),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin updates the arguments");

    assert_eq!(updated.arguments, ["--read-only"]);
    assert_eq!(updated.command.as_deref(), Some("npx"));
}

#[tokio::test]
async fn a_kept_secret_keeps_its_value_and_an_absent_one_is_removed() {
    let fixture = tenant().await;
    let mut new = http("docs");
    new.environment = secrets(&[("GITHUB_TOKEN", TOKEN), ("REMOVED", "gone")]);
    let created = fixture.create(new).await;

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                environment: Some(BTreeMap::from([
                    ("GITHUB_TOKEN".to_owned(), None),
                    ("ADDED".to_owned(), Some(SecretValue::new("new"))),
                ])),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin updates the environment");

    let expected = BTreeMap::from([("ADDED", "new"), ("GITHUB_TOKEN", TOKEN)]);
    assert_eq!(exposed(&updated.secrets.environment), expected);
    assert_eq!(
        exposed(&updated.secrets.headers),
        BTreeMap::from([("Authorization", AUTHORIZATION)])
    );
    let read = get_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.member,
    )
    .await
    .expect("the server");
    assert_eq!(exposed(&read.secrets.environment), expected);
}

#[tokio::test]
async fn removing_every_secret_stores_null() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;

    let updated = fixture
        .update(
            created.id,
            McpServerChanges {
                environment: Some(BTreeMap::new()),
                headers: Some(BTreeMap::new()),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin removes the secrets");

    assert_eq!(updated.secrets, Secrets::default());
    assert_eq!(fixture.stored(created.id).await, None);
}

#[tokio::test]
async fn keeping_a_secret_that_was_never_saved_is_invalid() {
    let fixture = tenant().await;
    let created = fixture.create(http("docs")).await;
    let sealed = fixture.stored(created.id).await;

    let kept = fixture
        .update(
            created.id,
            McpServerChanges {
                headers: Some(BTreeMap::from([("X-Missing".to_owned(), None)])),
                ..McpServerChanges::default()
            },
        )
        .await;

    assert!(
        matches!(&kept, Err(McpServerError::Invalid(message)) if message == "Header X-Missing has no saved value"),
        "{kept:?}"
    );
    assert_eq!(fixture.stored(created.id).await, sealed);
}

#[tokio::test]
async fn only_administrators_update_and_delete() {
    let fixture = tenant().await;
    let created = fixture.create(stdio("github")).await;

    let update = update_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.member,
        McpServerChanges {
            enabled: Some(false),
            ..McpServerChanges::default()
        },
    )
    .await;
    assert!(
        matches!(update, Err(McpServerError::Forbidden(_))),
        "{update:?}"
    );

    let delete = delete_authorized(
        &fixture.pool,
        fixture.organization,
        created.id,
        fixture.member,
    )
    .await;
    assert!(
        matches!(delete, Err(McpServerError::Forbidden(_))),
        "{delete:?}"
    );

    let deleted = delete_authorized(
        &fixture.pool,
        fixture.organization,
        created.id,
        fixture.admin,
    )
    .await
    .expect("an admin deletes the server");
    assert_eq!(deleted.id, created.id);
    assert_eq!(deleted.name, "github");

    let again = delete_authorized(
        &fixture.pool,
        fixture.organization,
        created.id,
        fixture.admin,
    )
    .await;
    assert!(
        matches!(again, Err(McpServerError::NotFound(SERVER_NOT_FOUND))),
        "{again:?}"
    );
    let missing = fixture
        .update(created.id, McpServerChanges::default())
        .await;
    assert!(
        matches!(missing, Err(McpServerError::NotFound(SERVER_NOT_FOUND))),
        "{missing:?}"
    );
}

#[tokio::test]
async fn a_server_of_another_organization_is_not_found() {
    let fixture = tenant().await;
    let other = tenant().await;
    let created = other.create(stdio("github")).await;

    let read = get_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        created.id,
        fixture.admin,
    )
    .await;
    assert!(
        matches!(read, Err(McpServerError::NotFound(SERVER_NOT_FOUND))),
        "{read:?}"
    );

    let delete = delete_authorized(
        &fixture.pool,
        fixture.organization,
        created.id,
        fixture.admin,
    )
    .await;
    assert!(
        matches!(delete, Err(McpServerError::NotFound(SERVER_NOT_FOUND))),
        "{delete:?}"
    );
    let update = fixture
        .update(
            created.id,
            McpServerChanges {
                enabled: Some(false),
                name: Some("renamed".to_owned()),
                ..McpServerChanges::default()
            },
        )
        .await
        .map(|_| ());
    assert!(
        matches!(update, Err(McpServerError::NotFound(SERVER_NOT_FOUND))),
        "{update:?}"
    );

    assert_eq!(other.count("mcp_servers").await, 1);
    let kept = other.read(created.id).await;
    assert_eq!(kept.name, "github");
    assert!(kept.enabled);
    assert_eq!(kept.updated_at, created.updated_at);
}

#[tokio::test]
async fn workspace_overrides_are_set_and_cleared() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.member,
        WorkspaceRole::Admin,
    )
    .await;
    let github = fixture.create(stdio("github")).await;
    let docs = fixture.create(http("docs")).await;

    let set = set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.member,
        BTreeMap::from([(github.id, Some(false)), (docs.id, Some(true))]),
    )
    .await
    .expect("a workspace admin sets overrides");
    assert_eq!(set, BTreeMap::from([(github.id, false), (docs.id, true)]));

    let changed = set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.member,
        BTreeMap::from([(github.id, Some(true)), (docs.id, None)]),
    )
    .await
    .expect("a workspace admin changes overrides");
    assert_eq!(changed, BTreeMap::from([(github.id, true)]));
    assert_eq!(
        workspace_overrides(&fixture.pool, fixture.workspace)
            .await
            .expect("the overrides"),
        changed
    );
    assert_eq!(
        organization_for_workspace(&fixture.pool, fixture.workspace)
            .await
            .expect("the organization"),
        Some(fixture.organization)
    );
    assert_eq!(
        organization_for_workspace(&fixture.pool, Uuid::new_v4())
            .await
            .expect("no organization"),
        None
    );
}

#[tokio::test]
async fn a_viewer_reads_overrides_and_cannot_write_them() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.member,
        WorkspaceRole::Viewer,
    )
    .await;
    let github = fixture.create(http("github")).await;

    let (servers, overrides) = workspace_list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.workspace,
        fixture.member,
    )
    .await
    .expect("a viewer reads the workspace's servers");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].secrets, github.secrets);
    assert!(overrides.is_empty());

    let write = set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.member,
        BTreeMap::from([(github.id, Some(false))]),
    )
    .await;
    assert!(
        matches!(write, Err(McpServerError::Forbidden(WORKSPACE_FORBIDDEN))),
        "{write:?}"
    );

    let outsider = workspace_list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        fixture.workspace,
        fixture.admin,
    )
    .await;
    assert!(
        matches!(outsider, Err(McpServerError::NotFound(WORKSPACE_NOT_FOUND))),
        "{outsider:?}"
    );
}

#[tokio::test]
async fn an_override_of_a_foreign_server_is_invalid() {
    let fixture = tenant().await;
    let other = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let own = fixture.create(stdio("github")).await;
    let foreign = other.create(stdio("github")).await;

    let changes = BTreeMap::from([(own.id, Some(false)), (foreign.id, Some(true))]);
    let result = set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.admin,
        changes,
    )
    .await;

    assert!(
        matches!(&result, Err(McpServerError::Invalid(message)) if message == FOREIGN_SERVER),
        "{result:?}"
    );
    assert!(
        workspace_overrides(&fixture.pool, fixture.workspace)
            .await
            .expect("the overrides")
            .is_empty()
    );
}

#[tokio::test]
async fn overrides_on_a_workspace_of_another_organization_are_not_found() {
    let fixture = tenant().await;
    let other = tenant().await;
    join_workspace(
        &other.pool,
        other.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let own = fixture.create(stdio("github")).await;

    let result = set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        other.workspace,
        fixture.admin,
        BTreeMap::from([(own.id, Some(false))]),
    )
    .await;
    assert!(
        matches!(result, Err(McpServerError::NotFound(WORKSPACE_NOT_FOUND))),
        "{result:?}"
    );

    let read = workspace_list_authorized(
        &fixture.pool,
        &fixture.key,
        fixture.organization,
        other.workspace,
        fixture.admin,
    )
    .await;
    assert!(
        matches!(read, Err(McpServerError::NotFound(WORKSPACE_NOT_FOUND))),
        "{read:?}"
    );
    assert!(
        workspace_overrides(&fixture.pool, other.workspace)
            .await
            .expect("the overrides")
            .is_empty()
    );
}

#[tokio::test]
async fn a_non_member_cannot_tell_a_workspace_that_exists_from_one_that_does_not() {
    let fixture = tenant().await;
    let server = fixture.create(stdio("github")).await;

    for workspace in [fixture.workspace, Uuid::new_v4()] {
        let read = workspace_list_authorized(
            &fixture.pool,
            &fixture.key,
            fixture.organization,
            workspace,
            fixture.outsider,
        )
        .await;
        let written = set_workspace_overrides_authorized(
            &fixture.pool,
            fixture.organization,
            workspace,
            fixture.outsider,
            BTreeMap::from([(server.id, Some(false))]),
        )
        .await;

        assert!(
            matches!(read, Err(McpServerError::NotFound(ORGANIZATION_NOT_FOUND))),
            "{workspace}: {read:?}"
        );
        assert!(
            matches!(
                written,
                Err(McpServerError::NotFound(ORGANIZATION_NOT_FOUND))
            ),
            "{workspace}: {written:?}"
        );
    }
}

#[tokio::test]
async fn deleting_the_organization_removes_its_servers_and_overrides() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let created = fixture.create(stdio("github")).await;
    set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.admin,
        BTreeMap::from([(created.id, Some(false))]),
    )
    .await
    .expect("an override");
    assert_eq!(fixture.count("workspace_mcp_servers").await, 1);

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(fixture.organization)
        .execute(&fixture.pool)
        .await
        .expect("the organization is deleted");

    assert_eq!(fixture.count("mcp_servers").await, 0);
    assert_eq!(fixture.count("workspace_mcp_servers").await, 0);
}

#[tokio::test]
async fn deleting_a_server_removes_its_overrides() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let created = fixture.create(stdio("github")).await;
    set_workspace_overrides_authorized(
        &fixture.pool,
        fixture.organization,
        fixture.workspace,
        fixture.admin,
        BTreeMap::from([(created.id, Some(true))]),
    )
    .await
    .expect("an override");

    delete_authorized(
        &fixture.pool,
        fixture.organization,
        created.id,
        fixture.admin,
    )
    .await
    .expect("an admin deletes the server");

    assert_eq!(fixture.count("workspace_mcp_servers").await, 0);
}

#[tokio::test]
async fn deleting_a_workspace_removes_its_overrides() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let created = fixture.create(stdio("github")).await;
    fixture
        .overrides(&fixture.pool, BTreeMap::from([(created.id, Some(false))]))
        .await
        .expect("an override");
    assert_eq!(fixture.count("workspace_mcp_servers").await, 1);

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(fixture.workspace)
        .execute(&fixture.pool)
        .await
        .expect("the workspace is deleted");

    assert_eq!(fixture.count("workspace_mcp_servers").await, 0);
    assert_eq!(fixture.count("mcp_servers").await, 1);
}

#[tokio::test]
async fn an_override_of_a_server_deleted_meanwhile_is_invalid() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let kept = fixture.create(stdio("github")).await;
    let deleted = fixture.create(stdio("gitlab")).await;
    let (session, backend) = session().await;
    let mut deleting = fixture.pool.begin().await.expect("a transaction");
    sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
        .bind(deleted.id)
        .execute(&mut *deleting)
        .await
        .expect("the server is deleted, not yet committed");
    let uncommitted = holder(&mut deleting).await;

    let overriding = fixture.overrides(
        &session,
        BTreeMap::from([(kept.id, Some(false)), (deleted.id, Some(false))]),
    );
    let commit = async {
        waits(&fixture.pool, backend, uncommitted).await;
        deleting.commit().await.expect("the delete commits");
    };
    let (result, ()) = tokio::join!(overriding, commit);

    assert!(
        matches!(&result, Err(McpServerError::Invalid(message)) if message == FOREIGN_SERVER),
        "{result:?}"
    );
    assert_eq!(fixture.count("workspace_mcp_servers").await, 0);
}

#[tokio::test]
async fn opposite_overrides_of_one_workspace_take_turns() {
    let fixture = tenant().await;
    join_workspace(
        &fixture.pool,
        fixture.workspace,
        fixture.admin,
        WorkspaceRole::Owner,
    )
    .await;
    let github = fixture.create(stdio("github")).await.id;
    let gitlab = fixture.create(stdio("gitlab")).await.id;
    let (low, high) = (github.min(gitlab), github.max(gitlab));
    fixture
        .overrides(
            &fixture.pool,
            BTreeMap::from([(low, Some(false)), (high, Some(false))]),
        )
        .await
        .expect("both overrides exist");
    let (one, first) = session().await;
    let (two, second) = session().await;
    let mut holding = fixture.pool.begin().await.expect("a transaction");
    sqlx::query(
        "SELECT 1 FROM workspace_mcp_servers WHERE workspace_id = $1 AND server_id = $2 FOR UPDATE",
    )
    .bind(fixture.workspace)
    .bind(low)
    .execute(&mut *holding)
    .await
    .expect("the lower override is held");
    let held = holder(&mut holding).await;

    let earlier = fixture.overrides(&one, BTreeMap::from([(low, Some(true)), (high, None)]));
    let later = async {
        waits(&fixture.pool, first, held).await;
        fixture
            .overrides(&two, BTreeMap::from([(low, None), (high, Some(true))]))
            .await
    };
    let release = async {
        waits(&fixture.pool, second, first).await;
        holding.commit().await.expect("the override is released");
    };
    let (earlier, later, ()) = tokio::join!(earlier, later, release);

    assert_eq!(
        earlier.expect("the earlier change lands"),
        BTreeMap::from([(low, true)])
    );
    assert_eq!(
        later.expect("the later change lands"),
        BTreeMap::from([(high, true)])
    );
    assert_eq!(
        workspace_overrides(&fixture.pool, fixture.workspace)
            .await
            .expect("the overrides"),
        BTreeMap::from([(high, true)])
    );
}

#[tokio::test]
async fn revisions_follow_every_change_to_a_server() {
    let fixture = tenant().await;
    assert!(
        revisions(&fixture.pool, fixture.organization)
            .await
            .expect("no servers")
            .is_empty()
    );
    let github = fixture.create(stdio("github")).await;
    let docs = fixture.create(http("docs")).await;
    assert_eq!(
        revisions(&fixture.pool, fixture.organization)
            .await
            .expect("the created servers"),
        BTreeMap::from([(github.id, github.updated_at), (docs.id, docs.updated_at)])
    );

    let updated = fixture
        .update(
            github.id,
            McpServerChanges {
                tools: Some(Some(vec!["search".to_owned()])),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin updates the tools");
    delete_authorized(&fixture.pool, fixture.organization, docs.id, fixture.admin)
        .await
        .expect("an admin deletes the server");

    assert_ne!(updated.updated_at, github.updated_at);
    assert_eq!(
        revisions(&fixture.pool, fixture.organization)
            .await
            .expect("the changed servers"),
        BTreeMap::from([(github.id, updated.updated_at)])
    );
    assert!(
        revisions(&fixture.pool, Uuid::new_v4())
            .await
            .expect("another organization")
            .is_empty()
    );
}

#[tokio::test]
async fn offered_leaves_out_what_the_organization_or_the_workspace_switched_off() {
    let fixture = tenant().await;
    let github = fixture
        .create(NewMcpServer {
            environment: secrets(&[("GITHUB_TOKEN", TOKEN)]),
            ..stdio("github")
        })
        .await;
    let docs = fixture.create(http("docs")).await;
    fixture
        .create(NewMcpServer {
            enabled: false,
            ..stdio("disabled")
        })
        .await;
    let elsewhere = workspace(&fixture.pool, fixture.organization).await;
    for (server, enabled) in [(github.id, true), (docs.id, false)] {
        sqlx::query(
            "INSERT INTO workspace_mcp_servers (workspace_id, server_id, organization_id, enabled) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(fixture.workspace)
        .bind(server)
        .bind(fixture.organization)
        .bind(enabled)
        .execute(&fixture.pool)
        .await
        .expect("an override");
    }

    let here = offered(&fixture.pool, fixture.organization, fixture.workspace)
        .await
        .expect("the servers the workspace is offered");
    let unset = offered(&fixture.pool, fixture.organization, elsewhere)
        .await
        .expect("the servers a workspace with no overrides is offered");

    let names = |records: &[McpServerRecord]| -> Vec<String> {
        records.iter().map(|record| record.name.clone()).collect()
    };
    assert_eq!(names(&here), ["github"]);
    assert_eq!(names(&unset), ["docs", "github"]);
    assert_eq!(here[0].id, github.id);
    assert_eq!(
        here[0].secrets,
        Secrets::default(),
        "the secrets are left unopened"
    );
}
