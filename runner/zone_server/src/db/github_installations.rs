//! Installations of an organization's GitHub Apps, and the credential each one mints from.

use std::fmt;

use abnegate_secret::REDACTED;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use super::DbResult;

#[derive(Debug, Error)]
#[error("unknown stored value")]
pub struct UnknownValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    User,
    Organization,
    Enterprise,
}

impl AccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Organization => "Organization",
            Self::Enterprise => "Enterprise",
        }
    }
}

impl TryFrom<String> for AccountType {
    type Error = UnknownValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "User" => Ok(Self::User),
            "Organization" => Ok(Self::Organization),
            "Enterprise" => Ok(Self::Enterprise),
            _ => Err(UnknownValue),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositorySelection {
    All,
    Selected,
}

impl RepositorySelection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Selected => "selected",
        }
    }
}

impl TryFrom<String> for RepositorySelection {
    type Error = UnknownValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "all" => Ok(Self::All),
            "selected" => Ok(Self::Selected),
            _ => Err(UnknownValue),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallationStatus {
    Active,
    Revoked,
    Suspended,
}

impl InstallationStatus {
    fn of(suspended_at: Option<DateTime<Utc>>, revoked_at: Option<DateTime<Utc>>) -> Self {
        match (revoked_at, suspended_at) {
            (Some(_), _) => Self::Revoked,
            (None, Some(_)) => Self::Suspended,
            (None, None) => Self::Active,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstallationRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub github_app_id: Uuid,
    pub installation_id: i64,
    pub account_login: String,
    #[sqlx(try_from = "String")]
    pub account_type: AccountType,
    #[sqlx(try_from = "String")]
    pub repository_selection: RepositorySelection,
    pub html_url: String,
    pub suspended_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InstallationRow {
    pub fn status(&self) -> InstallationStatus {
        InstallationStatus::of(self.suspended_at, self.revoked_at)
    }
}

pub struct InstallationUpsert<'a> {
    pub organization_id: Uuid,
    pub github_app_id: Uuid,
    pub installation_id: i64,
    pub account_login: &'a str,
    pub account_type: AccountType,
    pub repository_selection: RepositorySelection,
    pub html_url: &'a str,
    pub suspended_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
}

#[derive(Clone, sqlx::FromRow)]
pub struct InstallationCredential {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub app_id: i64,
    pub installation_id: i64,
    pub private_key: String,
    pub suspended_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl InstallationCredential {
    pub fn status(&self) -> InstallationStatus {
        InstallationStatus::of(self.suspended_at, self.revoked_at)
    }
}

impl fmt::Debug for InstallationCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstallationCredential")
            .field("id", &self.id)
            .field("organization_id", &self.organization_id)
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .field("private_key", &REDACTED)
            .field("suspended_at", &self.suspended_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstallationSummary {
    pub id: Uuid,
    pub account_login: String,
    pub suspended_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl InstallationSummary {
    pub fn status(&self) -> InstallationStatus {
        InstallationStatus::of(self.suspended_at, self.revoked_at)
    }
}

const COLUMNS: &str = "id, organization_id, github_app_id, installation_id, account_login, \
     account_type, repository_selection, html_url, suspended_at, revoked_at, created_by, \
     created_at, updated_at";

/// Fails with `RowNotFound` when `github_app_id` is not an app of
/// `organization_id`, or when GitHub's installation is already filed under
/// another app, so an installation can never cross organizations.
pub async fn upsert(
    pool: &PgPool,
    installation: &InstallationUpsert<'_>,
) -> DbResult<InstallationRow> {
    sqlx::query_as::<_, InstallationRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO github_installations \
           (organization_id, github_app_id, installation_id, account_login, account_type, \
            repository_selection, html_url, suspended_at, created_by) \
         SELECT organization_id, id, $3, $4, $5, $6, $7, $8, $9 \
           FROM github_apps WHERE organization_id = $1 AND id = $2 \
         ON CONFLICT (installation_id) DO UPDATE SET \
           account_login = EXCLUDED.account_login, \
           account_type = EXCLUDED.account_type, \
           repository_selection = EXCLUDED.repository_selection, \
           html_url = EXCLUDED.html_url, \
           suspended_at = EXCLUDED.suspended_at, \
           revoked_at = NULL, \
           updated_at = NOW() \
         WHERE github_installations.github_app_id = EXCLUDED.github_app_id \
         RETURNING {COLUMNS}"
    )))
    .bind(installation.organization_id)
    .bind(installation.github_app_id)
    .bind(installation.installation_id)
    .bind(installation.account_login)
    .bind(installation.account_type.as_str())
    .bind(installation.repository_selection.as_str())
    .bind(installation.html_url)
    .bind(installation.suspended_at)
    .bind(installation.created_by)
    .fetch_one(pool)
    .await
}

pub async fn list(pool: &PgPool, organization_id: Uuid) -> DbResult<Vec<InstallationRow>> {
    sqlx::query_as::<_, InstallationRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_installations WHERE organization_id = $1 \
         ORDER BY lower(account_login), installation_id"
    )))
    .bind(organization_id)
    .fetch_all(pool)
    .await
}

pub async fn get(
    pool: &PgPool,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<InstallationRow>> {
    sqlx::query_as::<_, InstallationRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_installations WHERE organization_id = $1 AND id = $2"
    )))
    .bind(organization_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Why a source or project was refused an installation [`attachable`] did not return.
pub const UNATTACHABLE: &str = "This GitHub account isn't connected to this organization, or it was \
     disconnected. Reconnect GitHub.";

/// The installation, if a source or project in `workspace_id` may use it:
/// it belongs to the workspace's organization and is neither revoked nor suspended.
pub async fn attachable(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> DbResult<Option<InstallationRow>> {
    sqlx::query_as::<_, InstallationRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_installations \
         WHERE id = $2 AND revoked_at IS NULL AND suspended_at IS NULL \
           AND organization_id = (SELECT organization_id FROM workspaces WHERE id = $1)"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The installation and its app's key, if a source or project in
/// `workspace_id` may mint from it: it belongs to the workspace's organization.
pub async fn credential(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> DbResult<Option<InstallationCredential>> {
    sqlx::query_as::<_, InstallationCredential>(
        "SELECT installation.id, installation.organization_id, app.app_id, \
                installation.installation_id, app.private_key, \
                installation.suspended_at, installation.revoked_at \
         FROM github_installations installation \
         JOIN github_apps app ON app.id = installation.github_app_id \
           AND app.organization_id = installation.organization_id \
         JOIN workspaces workspace ON workspace.organization_id = installation.organization_id \
         WHERE workspace.id = $1 AND installation.id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The installation and its app's key, if it belongs to `organization_id`.
pub async fn organization_credential(
    pool: &PgPool,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<InstallationCredential>> {
    sqlx::query_as::<_, InstallationCredential>(
        "SELECT installation.id, installation.organization_id, app.app_id, \
                installation.installation_id, app.private_key, \
                installation.suspended_at, installation.revoked_at \
         FROM github_installations installation \
         JOIN github_apps app ON app.id = installation.github_app_id \
           AND app.organization_id = installation.organization_id \
         WHERE installation.organization_id = $1 AND installation.id = $2",
    )
    .bind(organization_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn mark_revoked(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE github_installations \
         SET revoked_at = COALESCE(revoked_at, NOW()), updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn mark_suspended(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE github_installations \
         SET suspended_at = COALESCE(suspended_at, NOW()), updated_at = NOW() \
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Revokes every live installation of the app whose GitHub id is not in `present`.
pub async fn revoke_missing(pool: &PgPool, github_app_id: Uuid, present: &[i64]) -> DbResult<u64> {
    let result = sqlx::query(
        "UPDATE github_installations SET revoked_at = NOW(), updated_at = NOW() \
         WHERE github_app_id = $1 AND revoked_at IS NULL \
           AND NOT (installation_id = ANY($2))",
    )
    .bind(github_app_id)
    .bind(present)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete(pool: &PgPool, organization_id: Uuid, id: Uuid) -> DbResult<bool> {
    let result =
        sqlx::query("DELETE FROM github_installations WHERE organization_id = $1 AND id = $2")
            .bind(organization_id)
            .bind(id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn summaries(pool: &PgPool, ids: &[Uuid]) -> DbResult<Vec<InstallationSummary>> {
    sqlx::query_as::<_, InstallationSummary>(
        "SELECT id, account_login, suspended_at, revoked_at \
         FROM github_installations WHERE id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::db::github_apps::testing::{
        installation_id, pool, seed_app, seed_organization, seed_user, seed_workspace,
    };
    use crate::db::github_apps::{self, AppRow};
    use crate::db::github_connection_states::{self, Purpose};

    fn installation<'a>(app: &AppRow, installation_id: i64) -> InstallationUpsert<'a> {
        InstallationUpsert {
            organization_id: app.organization_id,
            github_app_id: app.id,
            installation_id,
            account_login: "acme",
            account_type: AccountType::Organization,
            repository_selection: RepositorySelection::All,
            html_url: "https://github.com/organizations/acme/settings/installations/1",
            suspended_at: None,
            created_by: None,
        }
    }

    async fn seed_installation(pool: &PgPool, app: &AppRow, number: i64) -> InstallationRow {
        upsert(pool, &installation(app, number))
            .await
            .expect("seed installation")
    }

    async fn seeded(pool: &PgPool, app: &AppRow) -> InstallationRow {
        seed_installation(pool, app, installation_id()).await
    }

    async fn status(pool: &PgPool, row: &InstallationRow) -> InstallationStatus {
        get(pool, row.organization_id, row.id)
            .await
            .unwrap()
            .expect("installation still exists")
            .status()
    }

    #[test]
    fn enums_round_trip_and_refuse_unknown_values() {
        for account_type in [
            AccountType::User,
            AccountType::Organization,
            AccountType::Enterprise,
        ] {
            assert_eq!(
                AccountType::try_from(account_type.as_str().to_owned()).unwrap(),
                account_type
            );
        }
        for selection in [RepositorySelection::All, RepositorySelection::Selected] {
            assert_eq!(
                RepositorySelection::try_from(selection.as_str().to_owned()).unwrap(),
                selection
            );
        }
        assert!(AccountType::try_from("Bot".to_owned()).is_err());
        assert!(RepositorySelection::try_from("All".to_owned()).is_err());
    }

    #[test]
    fn revocation_outranks_suspension() {
        let now = Utc::now();
        assert_eq!(
            InstallationStatus::of(None, None),
            InstallationStatus::Active
        );
        assert_eq!(
            InstallationStatus::of(Some(now), None),
            InstallationStatus::Suspended
        );
        assert_eq!(
            InstallationStatus::of(None, Some(now)),
            InstallationStatus::Revoked
        );
        assert_eq!(
            InstallationStatus::of(Some(now), Some(now)),
            InstallationStatus::Revoked
        );
        assert_eq!(
            serde_json::to_value(InstallationStatus::Suspended).unwrap(),
            serde_json::json!("suspended")
        );
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_keeps_the_creator() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let creator = seed_user(&pool).await;
        let later = seed_user(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let number = installation_id();

        let first = upsert(
            &pool,
            &InstallationUpsert {
                created_by: Some(creator),
                ..installation(&app, number)
            },
        )
        .await
        .unwrap();
        let second = upsert(
            &pool,
            &InstallationUpsert {
                account_login: "acme-renamed",
                account_type: AccountType::Enterprise,
                repository_selection: RepositorySelection::Selected,
                html_url: "https://github.com/enterprises/acme",
                created_by: Some(later),
                ..installation(&app, number)
            },
        )
        .await
        .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.account_login, "acme-renamed");
        assert_eq!(second.account_type, AccountType::Enterprise);
        assert_eq!(second.repository_selection, RepositorySelection::Selected);
        assert_eq!(second.html_url, "https://github.com/enterprises/acme");
        assert_eq!(second.created_by, Some(creator));
        assert_eq!(second.created_at, first.created_at);
        assert!(second.updated_at >= first.updated_at);
        assert_eq!(list(&pool, organization).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn upsert_restores_a_revoked_installation() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;

        assert!(mark_revoked(&pool, row.id).await.unwrap());
        assert_eq!(status(&pool, &row).await, InstallationStatus::Revoked);

        let restored = seed_installation(&pool, &app, row.installation_id).await;
        assert_eq!(restored.id, row.id);
        assert_eq!(restored.revoked_at, None);
        assert_eq!(restored.status(), InstallationStatus::Active);
    }

    #[tokio::test]
    async fn upsert_without_a_suspension_lifts_one() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;

        let suspended = upsert(
            &pool,
            &InstallationUpsert {
                suspended_at: Some(Utc::now()),
                ..installation(&app, installation_id())
            },
        )
        .await
        .unwrap();
        assert_eq!(suspended.status(), InstallationStatus::Suspended);

        let lifted = seed_installation(&pool, &app, suspended.installation_id).await;
        assert_eq!(lifted.id, suspended.id);
        assert_eq!(lifted.suspended_at, None);
        assert_eq!(lifted.status(), InstallationStatus::Active);
    }

    #[tokio::test]
    async fn upsert_refuses_another_organizations_app() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let other = seed_organization(&pool).await;
        let app = seed_app(&pool, other, "acme").await;

        let refused = upsert(
            &pool,
            &InstallationUpsert {
                organization_id: organization,
                ..installation(&app, installation_id())
            },
        )
        .await
        .expect_err("an installation must not be filed under another organization's app");
        assert!(matches!(refused, sqlx::Error::RowNotFound), "{refused:?}");
        assert!(list(&pool, organization).await.unwrap().is_empty());
        assert!(list(&pool, other).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn status_writers_set_the_status_once() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;

        assert!(mark_suspended(&pool, row.id).await.unwrap());
        assert_eq!(status(&pool, &row).await, InstallationStatus::Suspended);
        let suspended_at = get(&pool, organization, row.id)
            .await
            .unwrap()
            .unwrap()
            .suspended_at;
        assert!(mark_suspended(&pool, row.id).await.unwrap());
        let unchanged = get(&pool, organization, row.id).await.unwrap().unwrap();
        assert_eq!(unchanged.suspended_at, suspended_at);

        assert!(mark_revoked(&pool, row.id).await.unwrap());
        assert_eq!(status(&pool, &row).await, InstallationStatus::Revoked);

        assert!(!mark_revoked(&pool, Uuid::new_v4()).await.unwrap());
        assert!(!mark_suspended(&pool, Uuid::new_v4()).await.unwrap());
    }

    #[tokio::test]
    async fn revoke_missing_revokes_only_the_absent_installations() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let neighbour = seed_app(&pool, organization, "globex").await;
        let kept = seeded(&pool, &app).await;
        let gone = seeded(&pool, &app).await;
        let also_kept = seeded(&pool, &app).await;
        let already_revoked = seeded(&pool, &app).await;
        mark_revoked(&pool, already_revoked.id).await.unwrap();
        let unrelated = seeded(&pool, &neighbour).await;
        let present = [kept.installation_id, also_kept.installation_id];

        assert_eq!(revoke_missing(&pool, app.id, &present).await.unwrap(), 1);
        assert_eq!(status(&pool, &kept).await, InstallationStatus::Active);
        assert_eq!(status(&pool, &gone).await, InstallationStatus::Revoked);
        assert_eq!(status(&pool, &also_kept).await, InstallationStatus::Active);
        assert_eq!(status(&pool, &unrelated).await, InstallationStatus::Active);

        assert_eq!(revoke_missing(&pool, app.id, &[]).await.unwrap(), 2);
        assert_eq!(status(&pool, &kept).await, InstallationStatus::Revoked);
    }

    #[tokio::test]
    async fn attachable_refuses_foreign_revoked_and_suspended_installations() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;
        let other = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let foreign_app = seed_app(&pool, other, "acme").await;

        let active = seeded(&pool, &app).await;
        let revoked = seeded(&pool, &app).await;
        mark_revoked(&pool, revoked.id).await.unwrap();
        let suspended = seeded(&pool, &app).await;
        mark_suspended(&pool, suspended.id).await.unwrap();
        let foreign = seeded(&pool, &foreign_app).await;

        let attached = attachable(&pool, workspace, active.id).await.unwrap();
        assert_eq!(attached.map(|row| row.id), Some(active.id));
        for refused in [revoked.id, suspended.id, foreign.id, Uuid::new_v4()] {
            assert!(
                attachable(&pool, workspace, refused)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let unknown_workspace = attachable(&pool, Uuid::new_v4(), active.id).await.unwrap();
        assert!(unknown_workspace.is_none());
    }

    #[tokio::test]
    async fn summaries_report_login_and_status() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let active = seeded(&pool, &app).await;
        let revoked = upsert(
            &pool,
            &InstallationUpsert {
                account_login: "globex",
                ..installation(&app, installation_id())
            },
        )
        .await
        .unwrap();
        mark_revoked(&pool, revoked.id).await.unwrap();
        let unrequested = seeded(&pool, &app).await;

        let mut found = summaries(&pool, &[active.id, revoked.id, Uuid::new_v4()])
            .await
            .unwrap();
        found.sort_by(|left, right| left.account_login.cmp(&right.account_login));
        let reported: Vec<(Uuid, &str, InstallationStatus)> = found
            .iter()
            .map(|summary| (summary.id, summary.account_login.as_str(), summary.status()))
            .collect();
        assert_eq!(
            reported,
            vec![
                (active.id, "acme", InstallationStatus::Active),
                (revoked.id, "globex", InstallationStatus::Revoked),
            ]
        );
        assert!(found.iter().all(|summary| summary.id != unrequested.id));
        assert!(summaries(&pool, &[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn credential_carries_the_apps_key_and_never_prints_it() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;
        mark_suspended(&pool, row.id).await.unwrap();

        let credential = credential(&pool, workspace, row.id).await.unwrap().unwrap();
        assert_eq!(credential.id, row.id);
        assert_eq!(credential.organization_id, organization);
        assert_eq!(credential.app_id, app.app_id);
        assert_eq!(credential.installation_id, row.installation_id);
        assert_eq!(credential.private_key, app.private_key);
        assert_eq!(credential.status(), InstallationStatus::Suspended);

        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("private-key"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");

        assert!(
            super::credential(&pool, workspace, Uuid::new_v4())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn credential_is_refused_to_a_workspace_of_another_organization() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let stranger = seed_organization(&pool).await;
        let theirs = seed_workspace(&pool, stranger).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;

        for workspace in [theirs, Uuid::new_v4()] {
            assert!(
                credential(&pool, workspace, row.id)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn organization_credential_carries_the_apps_key() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;
        mark_revoked(&pool, row.id).await.unwrap();

        let credential = organization_credential(&pool, organization, row.id)
            .await
            .unwrap()
            .expect("the organization's own installation");

        assert_eq!(credential.id, row.id);
        assert_eq!(credential.organization_id, organization);
        assert_eq!(credential.app_id, app.app_id);
        assert_eq!(credential.installation_id, row.installation_id);
        assert_eq!(credential.private_key, app.private_key);
        assert_eq!(credential.status(), InstallationStatus::Revoked);
    }

    #[tokio::test]
    async fn organization_credential_is_refused_to_another_organization() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let stranger = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;

        for (asking, id) in [
            (stranger, row.id),
            (Uuid::new_v4(), row.id),
            (organization, Uuid::new_v4()),
        ] {
            assert!(
                organization_credential(&pool, asking, id)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn a_github_installation_is_filed_under_one_app_only() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let stranger = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let theirs = seed_app(&pool, stranger, "acme").await;
        let row = seeded(&pool, &app).await;

        let refused = upsert(
            &pool,
            &InstallationUpsert {
                account_login: "hijacked",
                ..installation(&theirs, row.installation_id)
            },
        )
        .await
        .expect_err("another app must not claim the installation");
        assert!(matches!(refused, sqlx::Error::RowNotFound), "{refused:?}");
        let kept = get(&pool, organization, row.id).await.unwrap().unwrap();
        assert_eq!(kept.account_login, "acme");
        assert!(list(&pool, stranger).await.unwrap().is_empty());

        let duplicate = sqlx::query(
            "INSERT INTO github_installations (organization_id, github_app_id, installation_id, \
               account_login, account_type, repository_selection, html_url) \
             VALUES ($1, $2, $3, 'acme', 'Organization', 'all', 'https://github.com')",
        )
        .bind(stranger)
        .bind(theirs.id)
        .bind(row.installation_id)
        .execute(&pool)
        .await
        .expect_err("the database holds each GitHub installation once");
        assert_eq!(
            constraint(&duplicate),
            Some("github_installations_installation_id_key")
        );
    }

    #[tokio::test]
    async fn an_installation_must_belong_to_its_apps_organization() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let stranger = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;

        let refused = sqlx::query(
            "INSERT INTO github_installations (organization_id, github_app_id, installation_id, \
               account_login, account_type, repository_selection, html_url) \
             VALUES ($1, $2, $3, 'acme', 'Organization', 'all', 'https://github.com')",
        )
        .bind(stranger)
        .bind(app.id)
        .bind(installation_id())
        .execute(&pool)
        .await
        .expect_err("an installation cannot be filed under another organization's app");
        assert_eq!(
            constraint(&refused),
            Some("github_installations_github_app_organization_fkey")
        );
        assert!(list(&pool, stranger).await.unwrap().is_empty());
    }

    fn constraint(error: &sqlx::Error) -> Option<&str> {
        error
            .as_database_error()
            .and_then(|database| database.constraint())
    }

    #[tokio::test]
    async fn deleting_an_installation_detaches_sources_and_projects() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;
        let other = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;

        let source = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO sources (id, workspace_id, name, source_type, github_installation_id) \
             VALUES ($1, $2, $1::text, 'github', $3)",
        )
        .bind(source)
        .bind(workspace)
        .bind(row.id)
        .execute(&pool)
        .await
        .unwrap();
        let project = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, github_installation_id) \
             VALUES ($1, $2, 'project', $3)",
        )
        .bind(project)
        .bind(workspace)
        .bind(row.id)
        .execute(&pool)
        .await
        .unwrap();

        assert!(!delete(&pool, other, row.id).await.unwrap());
        assert!(delete(&pool, organization, row.id).await.unwrap());
        assert!(get(&pool, organization, row.id).await.unwrap().is_none());

        let source_installation: Option<Uuid> =
            sqlx::query_scalar("SELECT github_installation_id FROM sources WHERE id = $1")
                .bind(source)
                .fetch_one(&pool)
                .await
                .unwrap();
        let project_installation: Option<Uuid> =
            sqlx::query_scalar("SELECT github_installation_id FROM projects WHERE id = $1")
                .bind(project)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source_installation, None);
        assert_eq!(project_installation, None);
    }

    #[tokio::test]
    async fn deleting_an_app_removes_its_installations_and_states() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let neighbour = seed_app(&pool, organization, "globex").await;
        let row = seeded(&pool, &app).await;
        let surviving = seeded(&pool, &neighbour).await;
        let issued = github_connection_states::issue(
            &pool,
            organization,
            user,
            Purpose::Installation,
            Some(app.id),
        )
        .await
        .unwrap();

        assert!(
            github_apps::delete(&pool, organization, app.id)
                .await
                .unwrap()
        );

        assert!(get(&pool, organization, row.id).await.unwrap().is_none());
        assert!(
            get(&pool, organization, surviving.id)
                .await
                .unwrap()
                .is_some()
        );
        let consumed =
            github_connection_states::consume(&pool, issued.state.expose(), Purpose::Installation)
                .await
                .unwrap();
        assert!(consumed.is_none());
    }

    #[tokio::test]
    async fn deleting_an_organization_removes_everything_it_owns() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;
        let user = seed_user(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;
        let row = seeded(&pool, &app).await;
        let issued =
            github_connection_states::issue(&pool, organization, user, Purpose::Registration, None)
                .await
                .unwrap();
        assert!(issued.expires_at > Utc::now() + TimeDelta::minutes(59));

        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            github_apps::list(&pool, organization)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            credential(&pool, workspace, row.id)
                .await
                .unwrap()
                .is_none()
        );
        let consumed =
            github_connection_states::consume(&pool, issued.state.expose(), Purpose::Registration)
                .await
                .unwrap();
        assert!(consumed.is_none());
    }

    #[test]
    fn the_unattachable_message_reads_as_one_sentence_pair() {
        assert_eq!(
            UNATTACHABLE,
            "This GitHub account isn't connected to this organization, or it was disconnected. \
             Reconnect GitHub."
        );
    }
}
