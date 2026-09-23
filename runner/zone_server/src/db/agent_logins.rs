//! The coding agent sign-ins Zone keeps for each organization.

use chrono::{DateTime, Utc};
use sqlx::{Executor, PgConnection, Postgres};
use uuid::Uuid;
use zone_core::SecretValue;

use super::DbResult;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentLoginRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub agent: String,
    pub credential: Option<SecretValue>,
    pub label: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Upsert<'a> {
    pub organization_id: Uuid,
    pub agent: &'a str,
    pub credential: Option<&'a str>,
    pub label: Option<&'a str>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn get<'e, E>(
    executor: E,
    organization_id: Uuid,
    agent: &str,
) -> DbResult<Option<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(
        r#"
        SELECT id, organization_id, agent, credential, label, expires_at, created_at, updated_at
        FROM agent_logins
        WHERE organization_id = $1 AND agent = $2
        "#,
    )
    .bind(organization_id)
    .bind(agent)
    .fetch_optional(executor)
    .await
}

pub async fn list<'e, E>(executor: E, organization_id: Uuid) -> DbResult<Vec<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(
        r#"
        SELECT id, organization_id, agent, credential, label, expires_at, created_at, updated_at
        FROM agent_logins
        WHERE organization_id = $1
        ORDER BY agent
        "#,
    )
    .bind(organization_id)
    .fetch_all(executor)
    .await
}

/// The row lock lasts until the caller's transaction ends, and only a
/// transaction keeps it past this statement: call it inside one.
pub async fn lock(
    connection: &mut PgConnection,
    organization_id: Uuid,
    agent: &str,
) -> DbResult<Option<AgentLoginRow>> {
    sqlx::query_as(
        r#"
        SELECT id, organization_id, agent, credential, label, expires_at, created_at, updated_at
        FROM agent_logins
        WHERE organization_id = $1 AND agent = $2
        FOR UPDATE
        "#,
    )
    .bind(organization_id)
    .bind(agent)
    .fetch_optional(connection)
    .await
}

pub async fn upsert<'e, E>(executor: E, login: &Upsert<'_>) -> DbResult<AgentLoginRow>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(
        r#"
        INSERT INTO agent_logins (organization_id, agent, credential, label, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (organization_id, agent) DO UPDATE SET
            credential = EXCLUDED.credential,
            label = EXCLUDED.label,
            expires_at = EXCLUDED.expires_at,
            updated_at = NOW()
        RETURNING id, organization_id, agent, credential, label, expires_at, created_at, updated_at
        "#,
    )
    .bind(login.organization_id)
    .bind(login.agent)
    .bind(login.credential)
    .bind(login.label)
    .bind(login.expires_at)
    .fetch_one(executor)
    .await
}

pub async fn renew<'e, E>(
    executor: E,
    id: Uuid,
    credential: &str,
    expires_at: Option<DateTime<Utc>>,
) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE agent_logins SET credential = $2, expires_at = $3, updated_at = NOW() WHERE id = $1",
    )
    .bind(id)
    .bind(credential)
    .bind(expires_at)
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn delete<'e, E>(executor: E, organization_id: Uuid, agent: &str) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query("DELETE FROM agent_logins WHERE organization_id = $1 AND agent = $2")
        .bind(organization_id)
        .bind(agent)
        .execute(executor)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use sqlx::PgPool;
    use zone_core::llm::AgentKind;

    use super::*;

    const LOCK_NOT_AVAILABLE: &str = "55P03";
    const AGENT_CHECK: &str = "agent_logins_agent_check";
    const KEY_SHARE: &str = "SELECT id FROM agent_logins WHERE id = $1 FOR KEY SHARE NOWAIT";

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn organization(pool: &PgPool) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Agent logins', $1::text)",
        )
        .bind(organization)
        .execute(pool)
        .await
        .expect("an organization to sign agents in for");
        organization
    }

    async fn discard(pool: &PgPool, organization: Uuid) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(pool)
            .await
            .expect("the test organization can be deleted");
    }

    async fn backdate(pool: &PgPool, id: Uuid) -> DateTime<Utc> {
        sqlx::query_scalar(
            "UPDATE agent_logins SET updated_at = updated_at - INTERVAL '1 day' WHERE id = $1 RETURNING updated_at",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the login's last write can be moved into the past")
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 9, day, 4, 0, 0).unwrap()
    }

    fn claude(organization_id: Uuid) -> Upsert<'static> {
        Upsert {
            organization_id,
            agent: AgentKind::Claude.as_str(),
            credential: Some("sealed-claude"),
            label: Some("Claude Max"),
            expires_at: Some(at(23)),
        }
    }

    fn codex(organization_id: Uuid) -> Upsert<'static> {
        Upsert {
            organization_id,
            agent: AgentKind::Codex.as_str(),
            credential: None,
            label: None,
            expires_at: None,
        }
    }

    fn exposed(login: &AgentLoginRow) -> Option<&str> {
        login.credential.as_ref().map(SecretValue::expose)
    }

    fn code(error: &sqlx::Error) -> Option<String> {
        error
            .as_database_error()
            .and_then(|database| database.code())
            .map(|code| code.into_owned())
    }

    #[tokio::test]
    async fn upsert_stores_a_login_and_replaces_it_whole() {
        let pool = pool().await;
        let organization = organization(&pool).await;

        let stored = upsert(&pool, &claude(organization)).await.unwrap();
        assert_eq!(stored.organization_id, organization);
        assert_eq!(stored.agent, AgentKind::Claude.as_str());
        assert_eq!(exposed(&stored), Some("sealed-claude"));
        assert_eq!(stored.label.as_deref(), Some("Claude Max"));
        assert_eq!(stored.expires_at, Some(at(23)));

        let written = backdate(&pool, stored.id).await;
        let replaced = upsert(
            &pool,
            &Upsert {
                credential: Some("sealed-again"),
                label: Some("Claude Pro"),
                expires_at: Some(at(24)),
                ..claude(organization)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            replaced.id, stored.id,
            "signing in again replaces the organization's login instead of adding a second"
        );
        assert_eq!(exposed(&replaced), Some("sealed-again"));
        assert_eq!(replaced.label.as_deref(), Some("Claude Pro"));
        assert_eq!(replaced.expires_at, Some(at(24)));
        assert_eq!(replaced.created_at, stored.created_at);
        assert!(
            replaced.updated_at > written,
            "a replacement must record when it was written"
        );

        let cleared = upsert(
            &pool,
            &Upsert {
                credential: None,
                label: None,
                expires_at: None,
                ..claude(organization)
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.id, stored.id);
        assert_eq!(
            exposed(&cleared),
            None,
            "a replacement writes every field, so one it leaves out is cleared, not kept"
        );
        assert_eq!(cleared.label, None);
        assert_eq!(cleared.expires_at, None);

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn get_and_list_read_only_the_organizations_own_logins() {
        let pool = pool().await;
        let ours = organization(&pool).await;
        let theirs = organization(&pool).await;

        for agent in AgentKind::ALL {
            assert!(get(&pool, ours, agent.as_str()).await.unwrap().is_none());
        }
        upsert(&pool, &codex(ours)).await.unwrap();
        upsert(&pool, &claude(ours)).await.unwrap();
        upsert(
            &pool,
            &Upsert {
                credential: Some("sealed-theirs"),
                ..claude(theirs)
            },
        )
        .await
        .unwrap();

        let found = get(&pool, ours, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .expect("our claude login");
        assert_eq!(found.organization_id, ours);
        assert_eq!(
            exposed(&found),
            Some("sealed-claude"),
            "reading one organization's login must never return another's"
        );
        let found = get(&pool, ours, AgentKind::Codex.as_str())
            .await
            .unwrap()
            .expect("our codex login");
        assert_eq!(exposed(&found), None);
        assert!(
            get(&pool, theirs, AgentKind::Codex.as_str())
                .await
                .unwrap()
                .is_none()
        );

        let listed: Vec<String> = list(&pool, ours)
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.agent)
            .collect();
        assert_eq!(listed, ["claude", "codex"]);
        let listed = list(&pool, theirs).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(exposed(&listed[0]), Some("sealed-theirs"));
        assert!(list(&pool, Uuid::new_v4()).await.unwrap().is_empty());

        discard(&pool, ours).await;
        discard(&pool, theirs).await;
    }

    #[tokio::test]
    async fn lock_holds_the_login_until_its_transaction_ends() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = upsert(&pool, &claude(organization)).await.unwrap();
        let mut bystander = pool.acquire().await.unwrap();

        let mut transaction = pool.begin().await.unwrap();
        let locked = lock(&mut transaction, organization, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .expect("the stored login is there to lock");
        assert_eq!(locked.id, stored.id);
        assert_eq!(exposed(&locked), Some("sealed-claude"));

        let refused = sqlx::query(KEY_SHARE)
            .bind(stored.id)
            .execute(&mut *bystander)
            .await
            .expect_err("a row held FOR UPDATE refuses even a key-share lock");
        assert_eq!(code(&refused).as_deref(), Some(LOCK_NOT_AVAILABLE));

        renew(&mut *transaction, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        sqlx::query(KEY_SHARE)
            .bind(stored.id)
            .execute(&mut *bystander)
            .await
            .expect("the lock ends with its transaction");
        let renewed = get(&pool, organization, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .expect("the renewed login");
        assert_eq!(exposed(&renewed), Some("sealed-renewed"));

        let mut transaction = pool.begin().await.unwrap();
        assert!(
            lock(&mut transaction, organization, AgentKind::Codex.as_str())
                .await
                .unwrap()
                .is_none()
        );
        transaction.rollback().await.unwrap();

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn renew_writes_the_credential_and_expiry_and_keeps_the_label() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = upsert(&pool, &claude(organization)).await.unwrap();
        let written = backdate(&pool, stored.id).await;

        renew(&pool, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();

        let renewed = get(&pool, organization, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .expect("the renewed login");
        assert_eq!(renewed.id, stored.id);
        assert_eq!(exposed(&renewed), Some("sealed-renewed"));
        assert_eq!(renewed.expires_at, Some(at(24)));
        assert_eq!(
            renewed.label.as_deref(),
            Some("Claude Max"),
            "a renewal writes only the credential and its expiry, so the plan label stays"
        );
        assert_eq!(renewed.created_at, stored.created_at);
        assert!(renewed.updated_at > written);

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn renew_never_brings_back_a_deleted_login() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = upsert(&pool, &claude(organization)).await.unwrap();
        assert!(
            delete(&pool, organization, AgentKind::Claude.as_str())
                .await
                .unwrap()
        );

        renew(&pool, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();

        assert!(
            get(&pool, organization, AgentKind::Claude.as_str())
                .await
                .unwrap()
                .is_none(),
            "a refresh that finishes after a sign-out must not sign the organization back in"
        );

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn delete_signs_out_only_the_named_agent() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        upsert(&pool, &claude(organization)).await.unwrap();
        upsert(&pool, &codex(organization)).await.unwrap();

        assert!(
            delete(&pool, organization, AgentKind::Codex.as_str())
                .await
                .unwrap()
        );
        assert!(
            !delete(&pool, organization, AgentKind::Codex.as_str())
                .await
                .unwrap(),
            "a second sign-out finds nothing to remove"
        );
        assert!(
            get(&pool, organization, AgentKind::Codex.as_str())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get(&pool, organization, AgentKind::Claude.as_str())
                .await
                .unwrap()
                .is_some()
        );

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn only_an_agent_zone_drives_can_be_stored() {
        let pool = pool().await;
        let organization = organization(&pool).await;

        let refused = upsert(
            &pool,
            &Upsert {
                agent: "gemini",
                ..codex(organization)
            },
        )
        .await
        .expect_err("gemini is not an agent zone drives");
        let database = refused
            .as_database_error()
            .expect("the database itself refused the row");
        assert!(database.is_check_violation());
        assert_eq!(database.constraint(), Some(AGENT_CHECK));

        for agent in AgentKind::ALL {
            upsert(
                &pool,
                &Upsert {
                    agent: agent.as_str(),
                    ..codex(organization)
                },
            )
            .await
            .unwrap_or_else(|error| panic!("a {agent} login must be storable: {error}"));
        }

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn deleting_the_organization_deletes_its_logins() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        upsert(&pool, &claude(organization)).await.unwrap();
        upsert(&pool, &codex(organization)).await.unwrap();

        discard(&pool, organization).await;

        assert!(
            list(&pool, organization).await.unwrap().is_empty(),
            "an organization's logins must go with it"
        );
    }
}
