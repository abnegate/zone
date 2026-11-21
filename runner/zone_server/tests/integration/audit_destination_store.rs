//! The audit destination store: scoped records with sealed secrets, unique names per scope,
//! and a delivery cursor that starts at the snapshot horizon.

use abnegate_secret::{MasterKey, SecretValue};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::audit::Kind;
use zone_server::db::audit::destinations::{
    self, DUPLICATE_KEY, DestinationChanges, DestinationError, DestinationRecord, NewDestination,
};
use zone_server::db::audit::position::Position;
use zone_server::db::audit::scope::Scope;

use crate::common::Database;
use crate::common::audit::{database, settle};

const PREFIX: &str = "audit_destination_store";
const ENVELOPE_OPENING: &str = "ENC[v1:";
const SECRETS: &str = "{\"signing_secret\":\"whsec-original\"}";
const ROTATED_SECRETS: &str = "{\"signing_secret\":\"whsec-rotated\"}";
const NAME: &str = "Splunk";
const OTHER_NAME: &str = "Datadog";
const URL: &str = "https://siem.example.com/hook";
const LAN_URL: &str = "http://10.0.0.5/hook";
const CHECK_VIOLATION: &str = "23514";

struct Fixture {
    database: Database,
    key: MasterKey,
    user: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let database = database(PREFIX).await;
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users(id,email,password_hash) VALUES($1,$1::text || '@example.com','unused')",
        )
        .bind(user)
        .execute(&database.pool)
        .await
        .unwrap();
        Self {
            database,
            key: MasterKey::generate().expect("a key"),
            user,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn organization(&self) -> Scope {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Audit',$1::text)")
            .bind(id)
            .execute(self.pool())
            .await
            .unwrap();
        Scope::Organization(id)
    }

    fn new_destination(&self, scope: Scope, name: &str) -> NewDestination {
        NewDestination {
            scope,
            kind: Kind::Webhook,
            name: name.into(),
            enabled: true,
            settings: settings(URL),
            secrets: SecretValue::new(SECRETS),
            created_by: self.user,
        }
    }

    async fn attempt(&self, new: &NewDestination) -> Result<DestinationRecord, DestinationError> {
        let mut connection = self.pool().acquire().await.unwrap();
        destinations::create(&mut connection, &self.key, new).await
    }

    async fn create(&self, scope: Scope, name: &str) -> DestinationRecord {
        self.attempt(&self.new_destination(scope, name))
            .await
            .unwrap()
    }

    async fn update(
        &self,
        scope: Scope,
        id: Uuid,
        changes: DestinationChanges,
    ) -> Result<Option<DestinationRecord>, DestinationError> {
        destinations::update(self.pool(), &self.key, scope, id, &changes).await
    }

    async fn stored_secrets(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT secrets FROM audit_destinations WHERE id = $1")
            .bind(id)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn cursor(&self, id: Uuid) -> Option<Position> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT transaction_id, sequence FROM audit_deliveries WHERE destination_id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .unwrap();
        row.map(|(transaction_id, sequence)| Position {
            transaction_id,
            sequence,
        })
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn ids(&self, scope: Scope) -> Vec<Uuid> {
        destinations::list(self.pool(), scope)
            .await
            .unwrap()
            .into_iter()
            .map(|record| record.id)
            .collect()
    }

    async fn log(&self, scope: Scope) {
        sqlx::query(
            "INSERT INTO audit_logs(organization_id,action,resource_type) VALUES($1,'user.login','user')",
        )
        .bind(scope.organization_id())
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn rows_above(&self, position: Position) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_logs WHERE (transaction_id, sequence) > ($1, $2)",
        )
        .bind(position.transaction_id)
        .bind(position.sequence)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn settings(url: &str) -> Value {
    json!({"url": url})
}

fn renamed(name: &str) -> DestinationChanges {
    DestinationChanges {
        name: Some(name.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn secrets_are_stored_as_an_envelope_that_opens_only_under_the_key() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;

    let created = fixture.create(scope, NAME).await;

    let stored = fixture.stored_secrets(created.id).await;
    assert!(stored.starts_with(ENVELOPE_OPENING), "{stored}");
    assert!(!stored.contains("whsec-original"), "{stored}");
    assert_eq!(created.sealed_secrets, stored);
    assert_eq!(created.secrets(&fixture.key).unwrap().expose(), SECRETS);
    let other = MasterKey::generate().expect("a key");
    assert!(matches!(
        created.secrets(&other),
        Err(DestinationError::Unopenable(id, _)) if id == created.id
    ));
    assert!(!format!("{created:?}").contains("whsec-original"));
    assert!(!format!("{:?}", fixture.new_destination(scope, NAME)).contains("whsec-original"));

    assert_eq!(created.scope(), scope);
    assert_eq!(created.kind, Kind::Webhook.as_str());
    assert_eq!(created.name, NAME);
    assert!(created.enabled);
    assert_eq!(created.settings, settings(URL));
    assert_eq!(created.created_by, Some(fixture.user));
    assert_eq!(
        destinations::find(fixture.pool(), created.id)
            .await
            .unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        destinations::get(fixture.pool(), scope, created.id)
            .await
            .unwrap(),
        Some(created)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_name_is_unique_within_a_scope_and_free_in_another() {
    let fixture = Fixture::new().await;
    let first = fixture.organization().await;
    let second = fixture.organization().await;
    fixture.create(first, NAME).await;
    fixture.create(Scope::Server, NAME).await;

    for scope in [first, Scope::Server] {
        assert!(
            matches!(
                fixture.attempt(&fixture.new_destination(scope, NAME)).await,
                Err(DestinationError::Duplicate)
            ),
            "{scope:?}"
        );
    }
    fixture.create(second, NAME).await;
    let other = fixture.create(first, OTHER_NAME).await;
    assert!(matches!(
        fixture.update(first, other.id, renamed(NAME)).await,
        Err(DestinationError::Duplicate)
    ));

    assert_eq!(fixture.count("audit_destinations").await, 4);
    assert_eq!(
        fixture.count("audit_deliveries").await,
        4,
        "a refused create leaves no delivery behind"
    );
    let unique: bool =
        sqlx::query_scalar("SELECT indisunique FROM pg_index WHERE indexrelid = $1::regclass")
            .bind(DUPLICATE_KEY)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(unique, "{DUPLICATE_KEY} is the unique index on names");
    fixture.cleanup().await;
}

#[tokio::test]
async fn another_scope_never_reaches_a_destination() {
    let fixture = Fixture::new().await;
    let owner = fixture.organization().await;
    let outsider = fixture.organization().await;
    let owned = fixture.create(owner, NAME).await;
    let server = fixture.create(Scope::Server, NAME).await;

    for (scope, id) in [
        (outsider, owned.id),
        (Scope::Server, owned.id),
        (owner, server.id),
    ] {
        assert_eq!(
            destinations::get(fixture.pool(), scope, id).await.unwrap(),
            None,
            "{scope:?}"
        );
        let mut connection = fixture.pool().acquire().await.unwrap();
        assert_eq!(
            destinations::lock(&mut connection, scope, id)
                .await
                .unwrap(),
            None,
            "{scope:?}"
        );
        drop(connection);
        let disabled = DestinationChanges {
            enabled: Some(false),
            ..Default::default()
        };
        assert_eq!(
            fixture.update(scope, id, disabled).await.unwrap(),
            None,
            "{scope:?}"
        );
        assert!(
            !destinations::delete(fixture.pool(), scope, id)
                .await
                .unwrap(),
            "{scope:?}"
        );
    }

    assert_eq!(fixture.ids(owner).await, [owned.id]);
    assert_eq!(fixture.ids(Scope::Server).await, [server.id]);
    assert!(fixture.ids(outsider).await.is_empty());
    for record in [&owned, &server] {
        let found = destinations::find(fixture.pool(), record.id).await.unwrap();
        assert_eq!(
            found.as_ref(),
            Some(record),
            "nothing changed across scopes"
        );
    }
    let mut connection = fixture.pool().acquire().await.unwrap();
    assert_eq!(
        destinations::lock(&mut connection, owner, owned.id)
            .await
            .unwrap(),
        Some(owned)
    );
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_update_writes_only_what_changed() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;
    let created = fixture.create(scope, NAME).await;

    let unchanged = fixture
        .update(scope, created.id, DestinationChanges::default())
        .await
        .unwrap();
    assert_eq!(unchanged.as_ref(), Some(&created));

    let disabled = fixture
        .update(
            scope,
            created.id,
            DestinationChanges {
                name: Some(OTHER_NAME.into()),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("the destination");
    assert_eq!(disabled.name, OTHER_NAME);
    assert!(!disabled.enabled);
    assert_eq!(disabled.settings, created.settings);
    assert_eq!(disabled.sealed_secrets, created.sealed_secrets);
    assert_eq!(disabled.created_at, created.created_at);
    assert!(disabled.updated_at > created.updated_at);

    let replaced = fixture
        .update(
            scope,
            created.id,
            DestinationChanges {
                settings: Some(settings(LAN_URL)),
                secrets: Some(SecretValue::new(ROTATED_SECRETS)),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("the destination");
    assert_eq!(replaced.name, OTHER_NAME);
    assert!(!replaced.enabled);
    assert_eq!(replaced.settings, settings(LAN_URL));
    let stored = fixture.stored_secrets(created.id).await;
    assert!(stored.starts_with(ENVELOPE_OPENING), "{stored}");
    assert!(!stored.contains("whsec-rotated"), "{stored}");
    assert_eq!(
        replaced.secrets(&fixture.key).unwrap().expose(),
        ROTATED_SECRETS
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_destination_or_its_organization_removes_its_delivery() {
    let fixture = Fixture::new().await;
    let kept = fixture.organization().await;
    let deleted = fixture.organization().await;
    let removed = fixture.create(kept, NAME).await;
    let survivor = fixture.create(kept, OTHER_NAME).await;
    fixture.create(deleted, NAME).await;
    fixture.create(deleted, OTHER_NAME).await;

    assert!(
        destinations::delete(fixture.pool(), kept, removed.id)
            .await
            .unwrap()
    );
    assert!(
        !destinations::delete(fixture.pool(), kept, removed.id)
            .await
            .unwrap()
    );
    assert_eq!(fixture.cursor(removed.id).await, None);

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(deleted.organization_id())
        .execute(fixture.pool())
        .await
        .unwrap();

    assert!(fixture.ids(deleted).await.is_empty());
    assert_eq!(fixture.ids(kept).await, [survivor.id]);
    assert_eq!(fixture.count("audit_destinations").await, 1);
    assert_eq!(fixture.count("audit_deliveries").await, 1);
    assert!(fixture.cursor(survivor.id).await.is_some());
    fixture.cleanup().await;
}

#[tokio::test]
async fn delivery_starts_at_or_above_every_final_row_and_below_every_later_one() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;
    for _ in 0..3 {
        fixture.log(scope).await;
        fixture.log(Scope::Server).await;
    }
    settle(fixture.pool()).await;

    let created = fixture.create(scope, NAME).await;

    let start = fixture.cursor(created.id).await.expect("a delivery");
    assert_eq!(start.sequence, i64::MAX);
    assert_eq!(
        fixture.rows_above(start).await,
        0,
        "history before the destination is served by the export"
    );
    let (attempts, batch, lease, delivered): (i32, Option<i64>, Option<Uuid>, i64) =
        sqlx::query_as(
            "SELECT attempts, batch_transaction_id, lease_owner, delivered_count FROM audit_deliveries WHERE destination_id = $1",
        )
        .bind(created.id)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!((attempts, batch, lease, delivered), (0, None, None, 0));

    fixture.log(scope).await;
    settle(fixture.pool()).await;
    assert_eq!(
        fixture.rows_above(start).await,
        1,
        "a row written after the destination is delivered"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_create_beside_its_own_audit_row_starts_below_that_row() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;
    let mut transaction = fixture.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO audit_logs(organization_id,action,resource_type) VALUES($1,'audit_destination.created','audit_destination')",
    )
    .bind(scope.organization_id())
    .execute(&mut *transaction)
    .await
    .unwrap();

    let created = destinations::create(
        &mut transaction,
        &fixture.key,
        &fixture.new_destination(scope, NAME),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    settle(fixture.pool()).await;

    let start = fixture.cursor(created.id).await.expect("a delivery");
    assert_eq!(
        fixture.rows_above(start).await,
        1,
        "the row committed with the destination is delivered"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_rolled_back_create_leaves_nothing() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;
    let mut transaction = fixture.pool().begin().await.unwrap();

    destinations::create(
        &mut transaction,
        &fixture.key,
        &fixture.new_destination(scope, NAME),
    )
    .await
    .unwrap();
    transaction.rollback().await.unwrap();

    assert_eq!(fixture.count("audit_destinations").await, 0);
    assert_eq!(fixture.count("audit_deliveries").await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_table_refuses_an_unknown_kind() {
    let fixture = Fixture::new().await;
    let scope = fixture.organization().await;

    let error = sqlx::query(
        "INSERT INTO audit_destinations (organization_id, kind, name, enabled, settings, secrets) \
         VALUES ($1, 'kafka', $2, true, '{}'::jsonb, 'sealed')",
    )
    .bind(scope.organization_id())
    .bind(NAME)
    .execute(fixture.pool())
    .await
    .expect_err("an unknown kind is refused");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some(CHECK_VIOLATION),
        "{error}"
    );
    assert_eq!(fixture.count("audit_destinations").await, 0);
    fixture.cleanup().await;
}
