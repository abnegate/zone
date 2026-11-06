//! Files an installation GitHub described under the organization's App.

use abnegate_vcs::pull_request::{self, Installation};
use chrono::{DateTime, Utc};
use reqwest::Url;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::github_apps::AppRow;
use crate::db::github_installations::{
    self, AccountType, InstallationRow, InstallationUpsert, RepositorySelection,
};

use super::unrecorded::Unrecorded;

const SCHEMES: [&str; 2] = ["http", "https"];

/// Records `installation` under `app`, reviving it if it was revoked.
///
/// Refuses an installation of another App, one GitHub gave no http(s) address
/// for, and an account type or repository selection Zone does not know,
/// writing nothing.
pub async fn record(
    db: &PgPool,
    app: &AppRow,
    installation: &Installation,
    created_by: Option<Uuid>,
) -> Result<InstallationRow, Unrecorded> {
    let upsert = InstallationUpsert {
        organization_id: app.organization_id,
        github_app_id: app.id,
        installation_id: number(app, installation)?,
        account_login: &installation.account.login,
        account_type: account_type(installation.account.kind)?,
        repository_selection: repository_selection(installation.repository_selection)?,
        html_url: addressable(&installation.html_url)?,
        suspended_at: installation.suspended_at.map(DateTime::<Utc>::from),
        created_by,
    };

    github_installations::upsert(db, &upsert)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => Unrecorded::Foreign,
            error => Unrecorded::Database(error),
        })
}

fn number(app: &AppRow, installation: &Installation) -> Result<i64, Unrecorded> {
    let application = i64::try_from(installation.application.get())
        .map_err(|_| Unrecorded::ForeignApplication)?;
    if application != app.app_id {
        return Err(Unrecorded::ForeignApplication);
    }

    i64::try_from(installation.id.get()).map_err(|_| Unrecorded::OutOfRange)
}

fn account_type(kind: pull_request::AccountType) -> Result<AccountType, Unrecorded> {
    match kind {
        pull_request::AccountType::User => Ok(AccountType::User),
        pull_request::AccountType::Organization => Ok(AccountType::Organization),
        pull_request::AccountType::Enterprise => Ok(AccountType::Enterprise),
        _ => Err(Unrecorded::UnknownAccountType),
    }
}

fn repository_selection(
    selection: pull_request::RepositorySelection,
) -> Result<RepositorySelection, Unrecorded> {
    match selection {
        pull_request::RepositorySelection::All => Ok(RepositorySelection::All),
        pull_request::RepositorySelection::Selected => Ok(RepositorySelection::Selected),
        _ => Err(Unrecorded::UnknownRepositorySelection),
    }
}

fn addressable(html_url: &str) -> Result<&str, Unrecorded> {
    Url::parse(html_url)
        .ok()
        .filter(|url| SCHEMES.contains(&url.scheme()))
        .map(|_| html_url)
        .ok_or(Unrecorded::Unaddressable)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use abnegate_vcs::pull_request::{Account, ApplicationId, InstallationId};

    use super::*;
    use crate::db::github_apps::testing::{installation_id, pool, seed_app, seed_organization};
    use crate::db::github_installations::InstallationStatus;

    const HTML_URL: &str = "https://github.com/organizations/acme/settings/installations/1";

    fn installation_of(app: &AppRow, number: i64) -> Installation {
        Installation::new(
            InstallationId::new(number.unsigned_abs()),
            ApplicationId::new(app.app_id.unsigned_abs()),
            Account::new("acme", pull_request::AccountType::Organization),
            pull_request::RepositorySelection::Selected,
            HTML_URL,
        )
    }

    async fn rows(db: &PgPool, organization: Uuid) -> usize {
        github_installations::list(db, organization)
            .await
            .expect("list installations")
            .len()
    }

    #[tokio::test]
    async fn an_installation_of_the_app_is_recorded() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;
        let number = installation_id();
        let suspended_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_768_478_400);

        let row = record(
            &db,
            &app,
            &installation_of(&app, number).with_suspended_at(suspended_at),
            None,
        )
        .await
        .expect("record");

        assert_eq!(row.organization_id, organization);
        assert_eq!(row.github_app_id, app.id);
        assert_eq!(row.installation_id, number);
        assert_eq!(row.account_login, "acme");
        assert_eq!(row.account_type, AccountType::Organization);
        assert_eq!(row.repository_selection, RepositorySelection::Selected);
        assert_eq!(row.html_url, HTML_URL);
        assert_eq!(row.suspended_at, Some(DateTime::<Utc>::from(suspended_at)));
        assert_eq!(row.status(), InstallationStatus::Suspended);
    }

    #[tokio::test]
    async fn recording_again_is_idempotent_and_revives_a_revoked_installation() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;
        let installation = installation_of(&app, installation_id());

        let first = record(&db, &app, &installation, None)
            .await
            .expect("record");
        github_installations::mark_revoked(&db, first.id)
            .await
            .expect("revoke");
        let second = record(&db, &app, &installation, None)
            .await
            .expect("record again");

        assert_eq!(second.id, first.id);
        assert_eq!(second.revoked_at, None);
        assert_eq!(second.status(), InstallationStatus::Active);
        assert_eq!(rows(&db, organization).await, 1);
    }

    #[tokio::test]
    async fn an_installation_of_another_application_is_refused() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;
        let mut installation = installation_of(&app, installation_id());
        installation.application = ApplicationId::new(app.app_id.unsigned_abs() + 1);

        let refused = record(&db, &app, &installation, None)
            .await
            .expect_err("another App's installation must not be filed");

        assert!(
            matches!(refused, Unrecorded::ForeignApplication),
            "{refused:?}"
        );
        assert_eq!(rows(&db, organization).await, 0);
    }

    #[tokio::test]
    async fn an_unknown_account_type_or_selection_is_refused() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;
        let mut unknown_account = installation_of(&app, installation_id());
        unknown_account.account.kind = pull_request::AccountType::Unknown;
        let mut unknown_selection = installation_of(&app, installation_id());
        unknown_selection.repository_selection = pull_request::RepositorySelection::Unknown;

        let account = record(&db, &app, &unknown_account, None)
            .await
            .expect_err("an unknown account type must not be filed");
        let selection = record(&db, &app, &unknown_selection, None)
            .await
            .expect_err("an unknown selection must not be filed");

        assert!(
            matches!(account, Unrecorded::UnknownAccountType),
            "{account:?}"
        );
        assert!(
            matches!(selection, Unrecorded::UnknownRepositorySelection),
            "{selection:?}"
        );
        assert_eq!(rows(&db, organization).await, 0);
    }

    #[tokio::test]
    async fn an_address_that_is_not_absolute_http_is_refused() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;

        for html_url in [
            "/organizations/acme/settings/installations/1",
            "javascript:alert(1)",
            "ftp://github.com/acme",
            "",
        ] {
            let mut installation = installation_of(&app, installation_id());
            installation.html_url = html_url.to_string();

            let refused = record(&db, &app, &installation, None)
                .await
                .expect_err("an address a browser cannot safely follow must not be filed");

            assert!(matches!(refused, Unrecorded::Unaddressable), "{refused:?}");
        }
        assert_eq!(rows(&db, organization).await, 0);
    }

    #[tokio::test]
    async fn a_private_host_address_is_recorded() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let app = seed_app(&db, organization, "acme").await;
        let mut installation = installation_of(&app, installation_id());
        installation.html_url = "http://127.0.0.1:8080/settings/installations/1".to_string();

        let row = record(&db, &app, &installation, None)
            .await
            .expect("record");

        assert_eq!(row.html_url, installation.html_url);
    }

    #[tokio::test]
    async fn an_installation_already_filed_under_another_organizations_app_is_foreign() {
        let db = pool().await;
        let organization = seed_organization(&db).await;
        let stranger = seed_organization(&db).await;
        let theirs = seed_app(&db, stranger, "acme").await;
        let ours = AppRow {
            app_id: theirs.app_id,
            ..seed_app(&db, organization, "acme").await
        };
        let installation = installation_of(&theirs, installation_id());
        record(&db, &theirs, &installation, None)
            .await
            .expect("record under their App");

        let refused = record(&db, &ours, &installation, None)
            .await
            .expect_err("an installation must never cross organizations");

        assert!(matches!(refused, Unrecorded::Foreign), "{refused:?}");
        assert_eq!(rows(&db, organization).await, 0);
        assert_eq!(rows(&db, stranger).await, 1);
    }
}
