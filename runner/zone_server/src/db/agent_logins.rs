//! The coding agent sign-ins Zone keeps for each organization, several per agent.

use abnegate_secret::SecretValue;
use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::{Executor, PgConnection, Postgres};
use uuid::Uuid;
use zone_core::llm::Window;

use super::DbResult;
use crate::services::login::usage::Snapshot;

macro_rules! columns {
    () => {
        "id, organization_id, agent, account, credential, label, expires_at, windows, headroom, \
         usage_fetched_at, exhausted_until, last_used_at, created_at, updated_at"
    };
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentLoginRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub agent: String,
    /// The account the agent's service knows the login by. Two logins of one agent with the same
    /// account are one login; a login with none is never merged with another.
    pub account: Option<String>,
    pub credential: Option<SecretValue>,
    pub label: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub windows: Option<Json<Vec<Window>>>,
    pub headroom: Option<f64>,
    pub usage_fetched_at: Option<DateTime<Utc>>,
    pub exhausted_until: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Insert<'a> {
    pub organization_id: Uuid,
    pub agent: &'a str,
    pub account: Option<&'a str>,
    pub credential: Option<&'a str>,
    pub label: Option<&'a str>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl AgentLoginRow {
    /// The usage last read for the login, or `None` when none has been.
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.usage_fetched_at.map(|fetched_at| Snapshot {
            windows: self
                .windows
                .as_ref()
                .map(|windows| windows.0.clone())
                .unwrap_or_default(),
            headroom: self.headroom,
            fetched_at,
        })
    }
}

pub async fn get<'e, E>(executor: E, id: Uuid) -> DbResult<Option<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_logins WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(executor)
    .await
}

/// Every login of the organization, by agent, oldest first within each.
pub async fn list<'e, E>(executor: E, organization_id: Uuid) -> DbResult<Vec<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_logins WHERE organization_id = $1 ORDER BY agent, created_at, id"
    ))
    .bind(organization_id)
    .fetch_all(executor)
    .await
}

/// The organization's logins of `agent`, oldest first.
pub async fn list_for<'e, E>(
    executor: E,
    organization_id: Uuid,
    agent: &str,
) -> DbResult<Vec<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_logins WHERE organization_id = $1 AND agent = $2 ORDER BY created_at, id"
    ))
    .bind(organization_id)
    .bind(agent)
    .fetch_all(executor)
    .await
}

/// The row lock lasts until the caller's transaction ends, and only a
/// transaction keeps it past this statement: call it inside one.
pub async fn lock(connection: &mut PgConnection, id: Uuid) -> DbResult<Option<AgentLoginRow>> {
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_logins WHERE id = $1 FOR UPDATE"
    ))
    .bind(id)
    .fetch_optional(connection)
    .await
}

/// Stores a new login, or, for an account the organization already holds a login of, replaces
/// that login's credential, label and expiry and ends its exhaustion.
pub async fn insert<'e, E>(executor: E, login: &Insert<'_>) -> DbResult<AgentLoginRow>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "INSERT INTO agent_logins (organization_id, agent, account, credential, label, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (organization_id, agent, account) WHERE account IS NOT NULL DO UPDATE SET \
             credential = EXCLUDED.credential, \
             label = EXCLUDED.label, \
             expires_at = EXCLUDED.expires_at, \
             exhausted_until = NULL, \
             updated_at = NOW() \
         RETURNING ",
        columns!()
    ))
    .bind(login.organization_id)
    .bind(login.agent)
    .bind(login.account)
    .bind(login.credential)
    .bind(login.label)
    .bind(login.expires_at)
    .fetch_one(executor)
    .await
}

/// Names the account of login `id`, which named none, so a sign-in to that account replaces it.
pub async fn identify<'e, E>(executor: E, id: Uuid, account: &str) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE agent_logins SET account = $2, updated_at = NOW() WHERE id = $1 AND account IS NULL",
    )
    .bind(id)
    .bind(account)
    .execute(executor)
    .await?;

    Ok(())
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

/// Writes `snapshot` over the usage last read for the login.
pub async fn observe<'e, E>(executor: E, id: Uuid, snapshot: &Snapshot) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE agent_logins SET windows = $2, headroom = $3, usage_fetched_at = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(&snapshot.windows))
    .bind(snapshot.headroom)
    .bind(snapshot.fetched_at)
    .execute(executor)
    .await?;

    Ok(())
}

/// Marks the login exhausted until `until`, or later when it already was.
pub async fn exhaust<'e, E>(executor: E, id: Uuid, until: DateTime<Utc>) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "UPDATE agent_logins SET exhausted_until = GREATEST(exhausted_until, $2) WHERE id = $1",
    )
    .bind(id)
    .bind(until)
    .execute(executor)
    .await?;

    Ok(())
}

/// Records that a turn started on the login at `at`, unless a later one already did.
pub async fn touch<'e, E>(executor: E, id: Uuid, at: DateTime<Utc>) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("UPDATE agent_logins SET last_used_at = GREATEST(last_used_at, $2) WHERE id = $1")
        .bind(id)
        .bind(at)
        .execute(executor)
        .await?;

    Ok(())
}

/// Removes the organization's login `id`, returning it, or `None` when the organization holds no
/// such login.
pub async fn delete<'e, E>(
    executor: E,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "DELETE FROM agent_logins WHERE organization_id = $1 AND id = $2 RETURNING ",
        columns!()
    ))
    .bind(organization_id)
    .bind(id)
    .fetch_optional(executor)
    .await
}

/// Removes every login the organization holds of `agent`, returning them.
pub async fn delete_all<'e, E>(
    executor: E,
    organization_id: Uuid,
    agent: &str,
) -> DbResult<Vec<AgentLoginRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "DELETE FROM agent_logins WHERE organization_id = $1 AND agent = $2 RETURNING ",
        columns!()
    ))
    .bind(organization_id)
    .bind(agent)
    .fetch_all(executor)
    .await
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
    const JAKE: &str = "jake@example.com";
    const ADA: &str = "ada@example.com";

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

    async fn reread(pool: &PgPool, id: Uuid) -> AgentLoginRow {
        get(pool, id)
            .await
            .expect("the login is readable")
            .expect("the login is still there")
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 9, day, 4, 0, 0).unwrap()
    }

    fn claude(organization_id: Uuid) -> Insert<'static> {
        Insert {
            organization_id,
            agent: AgentKind::Claude.as_str(),
            account: Some(JAKE),
            credential: Some("sealed-claude"),
            label: Some("Claude Max"),
            expires_at: Some(at(23)),
        }
    }

    fn codex(organization_id: Uuid) -> Insert<'static> {
        Insert {
            organization_id,
            agent: AgentKind::Codex.as_str(),
            account: None,
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

    fn snapshot(used_percent: f64, headroom: f64) -> Snapshot {
        Snapshot {
            windows: vec![
                Window {
                    name: "5h".to_string(),
                    used_percent: Some(used_percent),
                    used: None,
                    limit: None,
                    resets_at: Some(at(23)),
                },
                Window {
                    name: "7d".to_string(),
                    used_percent: Some(31.0),
                    used: Some(310),
                    limit: Some(1_000),
                    resets_at: Some(at(28)),
                },
            ],
            headroom: Some(headroom),
            fetched_at: at(22),
        }
    }

    #[tokio::test]
    async fn an_organization_keeps_several_logins_per_agent() {
        let pool = pool().await;
        let organization = organization(&pool).await;

        let jake = insert(&pool, &claude(organization)).await.unwrap();
        let ada = insert(
            &pool,
            &Insert {
                account: Some(ADA),
                credential: Some("sealed-ada"),
                ..claude(organization)
            },
        )
        .await
        .unwrap();
        let codex = insert(&pool, &codex(organization)).await.unwrap();

        assert_ne!(jake.id, ada.id);
        assert_eq!(jake.account.as_deref(), Some(JAKE));
        assert_eq!(ada.account.as_deref(), Some(ADA));
        assert_eq!(exposed(&reread(&pool, ada.id).await), Some("sealed-ada"));
        let claude: Vec<Uuid> = list_for(&pool, organization, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.id)
            .collect();
        assert_eq!(claude, [jake.id, ada.id], "oldest first");
        let every: Vec<Uuid> = list(&pool, organization)
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.id)
            .collect();
        assert_eq!(every, [jake.id, ada.id, codex.id], "by agent, oldest first");

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn signing_in_the_same_account_again_replaces_its_login_and_clears_its_exhaustion() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        observe(&pool, stored.id, &snapshot(100.0, 0.0))
            .await
            .unwrap();
        exhaust(&pool, stored.id, at(25)).await.unwrap();
        touch(&pool, stored.id, at(22)).await.unwrap();
        let written = backdate(&pool, stored.id).await;

        let replaced = insert(
            &pool,
            &Insert {
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
            "signing the same account in again replaces its login instead of adding a second"
        );
        assert_eq!(exposed(&replaced), Some("sealed-again"));
        assert_eq!(replaced.label.as_deref(), Some("Claude Pro"));
        assert_eq!(replaced.expires_at, Some(at(24)));
        assert_eq!(
            replaced.exhausted_until, None,
            "a fresh sign-in may be on a plan with headroom again"
        );
        assert_eq!(replaced.last_used_at, Some(at(22)));
        assert_eq!(replaced.snapshot(), Some(snapshot(100.0, 0.0)));
        assert_eq!(replaced.created_at, stored.created_at);
        assert!(
            replaced.updated_at > written,
            "a replacement must record when it was written"
        );
        assert_eq!(
            list_for(&pool, organization, AgentKind::Claude.as_str())
                .await
                .unwrap()
                .len(),
            1
        );

        let cleared = insert(
            &pool,
            &Insert {
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
    async fn a_login_with_no_account_is_never_merged_with_another() {
        let pool = pool().await;
        let organization = organization(&pool).await;

        let first = insert(&pool, &codex(organization)).await.unwrap();
        let second = insert(&pool, &codex(organization)).await.unwrap();
        let named = insert(
            &pool,
            &Insert {
                account: Some("account-1"),
                ..codex(organization)
            },
        )
        .await
        .unwrap();

        assert_ne!(first.id, second.id);
        assert_ne!(named.id, first.id);
        assert_ne!(named.id, second.id);
        assert_eq!(
            list_for(&pool, organization, AgentKind::Codex.as_str())
                .await
                .unwrap()
                .len(),
            3,
            "without an account Zone cannot tell two sign-ins are one, so it keeps both"
        );

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn an_account_is_one_login_per_agent_and_organization_only() {
        let pool = pool().await;
        let ours = organization(&pool).await;
        let theirs = organization(&pool).await;

        let claude = insert(&pool, &claude(ours)).await.unwrap();
        let codex = insert(
            &pool,
            &Insert {
                account: Some(JAKE),
                ..codex(ours)
            },
        )
        .await
        .unwrap();
        let elsewhere = insert(&pool, &self::claude(theirs)).await.unwrap();

        assert_ne!(
            claude.id, codex.id,
            "an account of another agent is another login"
        );
        assert_ne!(
            claude.id, elsewhere.id,
            "an account another organization signed in is that organization's login"
        );
        assert_eq!(
            exposed(&reread(&pool, claude.id).await),
            Some("sealed-claude")
        );

        discard(&pool, ours).await;
        discard(&pool, theirs).await;
    }

    #[tokio::test]
    async fn a_snapshot_is_written_over_the_login_and_read_back() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        assert_eq!(stored.snapshot(), None, "no usage has been read yet");
        let written = backdate(&pool, stored.id).await;

        observe(&pool, stored.id, &snapshot(62.0, 38.0))
            .await
            .unwrap();
        assert_eq!(
            reread(&pool, stored.id).await.snapshot(),
            Some(snapshot(62.0, 38.0))
        );

        let later = Snapshot {
            windows: vec![],
            headroom: None,
            fetched_at: at(24),
        };
        observe(&pool, stored.id, &later).await.unwrap();
        let observed = reread(&pool, stored.id).await;
        assert_eq!(
            observed.snapshot(),
            Some(later),
            "a reading replaces the one before it whole"
        );
        assert_eq!(
            observed.updated_at, written,
            "a usage reading is not a change to the sign-in"
        );
        assert_eq!(exposed(&observed), Some("sealed-claude"));

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn exhausting_a_login_never_shortens_an_earlier_exhaustion() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();

        exhaust(&pool, stored.id, at(25)).await.unwrap();
        assert_eq!(reread(&pool, stored.id).await.exhausted_until, Some(at(25)));

        exhaust(&pool, stored.id, at(24)).await.unwrap();
        assert_eq!(
            reread(&pool, stored.id).await.exhausted_until,
            Some(at(25)),
            "a limit that resets sooner never lifts one that resets later"
        );

        exhaust(&pool, stored.id, at(27)).await.unwrap();
        assert_eq!(reread(&pool, stored.id).await.exhausted_until, Some(at(27)));

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn touching_a_login_never_moves_its_last_use_back() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        assert_eq!(stored.last_used_at, None);

        touch(&pool, stored.id, at(24)).await.unwrap();
        touch(&pool, stored.id, at(23)).await.unwrap();

        assert_eq!(reread(&pool, stored.id).await.last_used_at, Some(at(24)));

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn deleting_one_login_leaves_its_siblings_and_returns_what_went() {
        let pool = pool().await;
        let ours = organization(&pool).await;
        let theirs = organization(&pool).await;
        let jake = insert(&pool, &claude(ours)).await.unwrap();
        let ada = insert(
            &pool,
            &Insert {
                account: Some(ADA),
                ..claude(ours)
            },
        )
        .await
        .unwrap();

        assert!(
            delete(&pool, theirs, jake.id).await.unwrap().is_none(),
            "an organization can never delete another's login"
        );
        let gone = delete(&pool, ours, jake.id)
            .await
            .unwrap()
            .expect("the login that went");
        assert_eq!(gone.id, jake.id);
        assert_eq!(gone.account.as_deref(), Some(JAKE));
        assert!(delete(&pool, ours, jake.id).await.unwrap().is_none());
        assert!(get(&pool, jake.id).await.unwrap().is_none());
        let left: Vec<Uuid> = list(&pool, ours)
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.id)
            .collect();
        assert_eq!(left, [ada.id]);

        discard(&pool, ours).await;
        discard(&pool, theirs).await;
    }

    #[tokio::test]
    async fn deleting_every_login_of_an_agent_leaves_the_other_agents() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let jake = insert(&pool, &claude(organization)).await.unwrap();
        let ada = insert(
            &pool,
            &Insert {
                account: Some(ADA),
                ..claude(organization)
            },
        )
        .await
        .unwrap();
        let codex = insert(&pool, &codex(organization)).await.unwrap();

        let mut gone: Vec<Uuid> = delete_all(&pool, organization, AgentKind::Claude.as_str())
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.id)
            .collect();
        gone.sort();
        let mut expected = vec![jake.id, ada.id];
        expected.sort();
        assert_eq!(gone, expected);
        assert!(
            delete_all(&pool, organization, AgentKind::Claude.as_str())
                .await
                .unwrap()
                .is_empty(),
            "a second sign-out finds nothing to remove"
        );
        let left: Vec<Uuid> = list(&pool, organization)
            .await
            .unwrap()
            .into_iter()
            .map(|login| login.id)
            .collect();
        assert_eq!(left, [codex.id]);

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn get_and_list_read_only_the_organizations_own_logins() {
        let pool = pool().await;
        let ours = organization(&pool).await;
        let theirs = organization(&pool).await;

        for agent in AgentKind::ALL {
            assert!(
                list_for(&pool, ours, agent.as_str())
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        insert(&pool, &codex(ours)).await.unwrap();
        let claude = insert(&pool, &claude(ours)).await.unwrap();
        insert(
            &pool,
            &Insert {
                credential: Some("sealed-theirs"),
                ..self::claude(theirs)
            },
        )
        .await
        .unwrap();

        let found = list_for(&pool, ours, AgentKind::Claude.as_str())
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].organization_id, ours);
        assert_eq!(
            exposed(&found[0]),
            Some("sealed-claude"),
            "reading one organization's login must never return another's"
        );
        assert_eq!(
            exposed(&reread(&pool, claude.id).await),
            Some("sealed-claude")
        );
        assert!(
            list_for(&pool, theirs, AgentKind::Codex.as_str())
                .await
                .unwrap()
                .is_empty()
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
        assert!(get(&pool, Uuid::new_v4()).await.unwrap().is_none());

        discard(&pool, ours).await;
        discard(&pool, theirs).await;
    }

    #[tokio::test]
    async fn lock_holds_one_login_and_not_its_siblings() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        let sibling = insert(
            &pool,
            &Insert {
                account: Some(ADA),
                ..claude(organization)
            },
        )
        .await
        .unwrap();
        let mut bystander = pool.acquire().await.unwrap();

        let mut transaction = pool.begin().await.unwrap();
        let locked = lock(&mut transaction, stored.id)
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
        sqlx::query(KEY_SHARE)
            .bind(sibling.id)
            .execute(&mut *bystander)
            .await
            .expect("renewing one login never waits on another of the same agent");

        renew(&mut *transaction, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        sqlx::query(KEY_SHARE)
            .bind(stored.id)
            .execute(&mut *bystander)
            .await
            .expect("the lock ends with its transaction");
        assert_eq!(
            exposed(&reread(&pool, stored.id).await),
            Some("sealed-renewed")
        );

        let mut transaction = pool.begin().await.unwrap();
        assert!(
            lock(&mut transaction, Uuid::new_v4())
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
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        let written = backdate(&pool, stored.id).await;

        renew(&pool, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();

        let renewed = reread(&pool, stored.id).await;
        assert_eq!(exposed(&renewed), Some("sealed-renewed"));
        assert_eq!(renewed.expires_at, Some(at(24)));
        assert_eq!(
            renewed.label.as_deref(),
            Some("Claude Max"),
            "a renewal writes only the credential and its expiry, so the plan label stays"
        );
        assert_eq!(renewed.account.as_deref(), Some(JAKE));
        assert_eq!(renewed.created_at, stored.created_at);
        assert!(renewed.updated_at > written);

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn renew_never_brings_back_a_deleted_login() {
        let pool = pool().await;
        let organization = organization(&pool).await;
        let stored = insert(&pool, &claude(organization)).await.unwrap();
        assert!(
            delete(&pool, organization, stored.id)
                .await
                .unwrap()
                .is_some()
        );

        renew(&pool, stored.id, "sealed-renewed", Some(at(24)))
            .await
            .unwrap();
        observe(&pool, stored.id, &snapshot(10.0, 90.0))
            .await
            .unwrap();
        exhaust(&pool, stored.id, at(25)).await.unwrap();
        touch(&pool, stored.id, at(25)).await.unwrap();

        assert!(
            list(&pool, organization).await.unwrap().is_empty(),
            "a write that finishes after a sign-out must not sign the organization back in"
        );

        discard(&pool, organization).await;
    }

    #[tokio::test]
    async fn only_an_agent_zone_drives_can_be_stored() {
        let pool = pool().await;
        let organization = organization(&pool).await;

        let refused = insert(
            &pool,
            &Insert {
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
            insert(
                &pool,
                &Insert {
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
        insert(&pool, &claude(organization)).await.unwrap();
        insert(&pool, &codex(organization)).await.unwrap();

        discard(&pool, organization).await;

        assert!(
            list(&pool, organization).await.unwrap().is_empty(),
            "an organization's logins must go with it"
        );
    }
}
