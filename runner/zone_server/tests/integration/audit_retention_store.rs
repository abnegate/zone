use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::audit::retentions;
use zone_server::db::audit::scope::Scope;

use crate::common::Database;
use crate::common::audit::database;

const PREFIX: &str = "audit_retention_store";
const CHECK_VIOLATION: &str = "23514";
const UNIQUE_VIOLATION: &str = "23505";
const DAYS_CHECK: &str = "audit_retentions_days_check";
const SCOPE_KEY: &str = "audit_retentions_scope_idx";

struct Fixture {
    database: Database,
}

impl Fixture {
    async fn new() -> Self {
        Self {
            database: database(PREFIX).await,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn organization(&self) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Audit', $1::text)")
            .bind(organization)
            .execute(self.pool())
            .await
            .unwrap();
        organization
    }

    async fn user(&self) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $1::text, 'audit')")
            .bind(user)
            .execute(self.pool())
            .await
            .unwrap();
        user
    }

    async fn rows(&self, scope: Scope) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM audit_retentions \
             WHERE organization_id IS NOT DISTINCT FROM $1",
        )
        .bind(scope.organization_id())
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn violation(error: &sqlx::Error, code: &str) -> Option<String> {
    let database = error.as_database_error()?;
    assert_eq!(database.code().as_deref(), Some(code), "{error}");
    database.constraint().map(str::to_owned)
}

#[tokio::test]
async fn setting_retention_inserts_once_then_updates_the_organization_row() {
    let fixture = Fixture::new().await;
    let organization = fixture.organization().await;
    let scope = Scope::Organization(organization);
    let first_user = fixture.user().await;
    let second_user = fixture.user().await;

    let first = retentions::set(fixture.pool(), scope, 30, first_user)
        .await
        .unwrap();
    assert_eq!(first.organization_id, Some(organization));
    assert_eq!(first.scope(), scope);
    assert_eq!(first.days, 30);
    assert_eq!(first.updated_by, Some(first_user));
    assert_eq!(first.swept_at, None);
    assert_eq!(first.deleted_count, 0);

    let second = retentions::set(fixture.pool(), scope, 7, second_user)
        .await
        .unwrap();
    assert_eq!(second.days, 7);
    assert_eq!(second.updated_by, Some(second_user));
    assert_eq!(second.created_at, first.created_at);
    assert!(second.updated_at >= first.updated_at);
    assert_eq!(fixture.rows(scope).await, 1);
    assert_eq!(
        retentions::get(fixture.pool(), scope).await.unwrap(),
        Some(second)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_server_scope_keeps_a_single_row() {
    let fixture = Fixture::new().await;
    let user = fixture.user().await;

    retentions::set(fixture.pool(), Scope::Server, 90, user)
        .await
        .unwrap();
    let updated = retentions::set(fixture.pool(), Scope::Server, 30, user)
        .await
        .unwrap();
    assert_eq!(updated.organization_id, None);
    assert_eq!(updated.scope(), Scope::Server);
    assert_eq!(updated.days, 30);
    assert_eq!(fixture.rows(Scope::Server).await, 1);

    let second =
        sqlx::query("INSERT INTO audit_retentions (organization_id, days) VALUES (NULL, 5)")
            .execute(fixture.pool())
            .await
            .expect_err("a second server row is refused");
    assert_eq!(
        violation(&second, UNIQUE_VIOLATION).as_deref(),
        Some(SCOPE_KEY)
    );
    assert_eq!(
        retentions::get(fixture.pool(), Scope::Server)
            .await
            .unwrap()
            .map(|record| record.days),
        Some(30)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn clearing_one_scope_leaves_the_others() {
    let fixture = Fixture::new().await;
    let user = fixture.user().await;
    let organization = fixture.organization().await;
    let other = fixture.organization().await;
    let scope = Scope::Organization(organization);
    retentions::set(fixture.pool(), Scope::Server, 90, user)
        .await
        .unwrap();
    retentions::set(fixture.pool(), scope, 30, user)
        .await
        .unwrap();
    retentions::set(fixture.pool(), Scope::Organization(other), 60, user)
        .await
        .unwrap();

    let cleared = retentions::clear(fixture.pool(), scope)
        .await
        .unwrap()
        .expect("the organization's period is cleared");
    assert_eq!(cleared.days, 30);
    assert_eq!(retentions::get(fixture.pool(), scope).await.unwrap(), None);
    assert_eq!(
        retentions::clear(fixture.pool(), scope).await.unwrap(),
        None,
        "nothing is left to clear"
    );
    assert_eq!(
        retentions::get(fixture.pool(), Scope::Organization(other))
            .await
            .unwrap()
            .map(|record| record.days),
        Some(60)
    );

    let all = retentions::all(fixture.pool()).await.unwrap();
    let scopes: Vec<(Scope, i32)> = all
        .iter()
        .map(|record| (record.scope(), record.days))
        .collect();
    assert_eq!(
        scopes,
        vec![(Scope::Server, 90), (Scope::Organization(other), 60)]
    );

    let server = retentions::clear(fixture.pool(), Scope::Server)
        .await
        .unwrap()
        .expect("the server period is cleared");
    assert_eq!(server.organization_id, None);
    assert_eq!(
        retentions::get(fixture.pool(), Scope::Organization(other))
            .await
            .unwrap()
            .map(|record| record.days),
        Some(60)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sweep_is_recorded_on_its_own_scope() {
    let fixture = Fixture::new().await;
    let user = fixture.user().await;
    let organization = fixture.organization().await;
    let scope = Scope::Organization(organization);
    retentions::set(fixture.pool(), Scope::Server, 90, user)
        .await
        .unwrap();
    retentions::set(fixture.pool(), scope, 30, user)
        .await
        .unwrap();

    retentions::swept(fixture.pool(), scope, 12).await.unwrap();
    let swept = retentions::get(fixture.pool(), scope)
        .await
        .unwrap()
        .unwrap();
    assert!(swept.swept_at.is_some());
    assert_eq!(swept.deleted_count, 12);
    let server = retentions::get(fixture.pool(), Scope::Server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(server.swept_at, None);
    assert_eq!(server.deleted_count, 0);

    retentions::swept(fixture.pool(), Scope::Server, 3)
        .await
        .unwrap();
    retentions::swept(fixture.pool(), scope, 0).await.unwrap();
    assert_eq!(
        retentions::get(fixture.pool(), Scope::Server)
            .await
            .unwrap()
            .unwrap()
            .deleted_count,
        3
    );
    assert_eq!(
        retentions::get(fixture.pool(), scope)
            .await
            .unwrap()
            .unwrap()
            .deleted_count,
        0,
        "the count is the latest sweep's"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_period_below_one_day_is_refused() {
    let fixture = Fixture::new().await;
    let user = fixture.user().await;
    let organization = fixture.organization().await;

    let refused = retentions::set(fixture.pool(), Scope::Organization(organization), 0, user)
        .await
        .expect_err("zero days is refused");
    assert_eq!(
        violation(&refused, CHECK_VIOLATION).as_deref(),
        Some(DAYS_CHECK)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_an_organization_deletes_its_period() {
    let fixture = Fixture::new().await;
    let user = fixture.user().await;
    let organization = fixture.organization().await;
    let scope = Scope::Organization(organization);
    retentions::set(fixture.pool(), scope, 30, user)
        .await
        .unwrap();

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert_eq!(retentions::get(fixture.pool(), scope).await.unwrap(), None);
    fixture.cleanup().await;
}
