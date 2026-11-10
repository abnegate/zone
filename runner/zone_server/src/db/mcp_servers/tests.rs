use std::collections::BTreeMap;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::*;

const ENVELOPE_OPENING: &str = "ENC[";
const TOKEN: &str = "ghp_mcp_server_token";
const AUTHORIZATION: &str = "Bearer mcp-header-secret";

async fn pool() -> PgPool {
    PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
        .await
        .expect("the test database accepts connections")
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

async fn join_workspace(pool: &PgPool, workspace: Uuid, user: Uuid, role: &str) {
    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(workspace)
        .bind(user)
        .bind(role)
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
        transport: "stdio".to_owned(),
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
        transport: "http".to_owned(),
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
    assert!(updated.updated_at >= created.updated_at);
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
    assert_eq!(other.count("mcp_servers").await, 1);
}

#[tokio::test]
async fn workspace_overrides_are_set_and_cleared() {
    let fixture = tenant().await;
    join_workspace(&fixture.pool, fixture.workspace, fixture.member, "admin").await;
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
    join_workspace(&fixture.pool, fixture.workspace, fixture.member, "viewer").await;
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
    join_workspace(&fixture.pool, fixture.workspace, fixture.admin, "owner").await;
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
    join_workspace(&other.pool, other.workspace, fixture.admin, "owner").await;
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
async fn deleting_the_organization_removes_its_servers_and_overrides() {
    let fixture = tenant().await;
    join_workspace(&fixture.pool, fixture.workspace, fixture.admin, "owner").await;
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
    join_workspace(&fixture.pool, fixture.workspace, fixture.admin, "owner").await;
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

#[tokio::test]
async fn has_enabled_sees_only_switched_on_servers() {
    let fixture = tenant().await;
    assert!(
        !has_enabled(&fixture.pool, fixture.organization)
            .await
            .expect("no servers")
    );

    let created = fixture
        .create(NewMcpServer {
            enabled: false,
            ..stdio("github")
        })
        .await;
    assert!(
        !has_enabled(&fixture.pool, fixture.organization)
            .await
            .expect("a disabled server")
    );

    fixture
        .update(
            created.id,
            McpServerChanges {
                enabled: Some(true),
                ..McpServerChanges::default()
            },
        )
        .await
        .expect("an admin enables the server");
    assert!(
        has_enabled(&fixture.pool, fixture.organization)
            .await
            .expect("an enabled server")
    );
    assert!(
        !has_enabled(&fixture.pool, Uuid::new_v4())
            .await
            .expect("another organization")
    );
}
