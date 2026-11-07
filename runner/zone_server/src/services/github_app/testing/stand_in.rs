use std::sync::Arc;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use chrono::{Duration, SecondsFormat, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::Config;
use crate::db::github_apps::testing::installation_id;
use crate::db::github_apps::{self, NewApp, OwnerType};
use crate::db::github_installations::{self, AccountType, InstallationUpsert, RepositorySelection};
use crate::services::github_app::Provider;
use crate::state::test_config;

use super::TEST_PRIVATE_KEY;

pub(crate) fn provider(server: &MockServer, master_key: Arc<MasterKey>) -> Provider {
    Provider::new(
        &Config {
            github_api_url: server.uri(),
            github_api_allows_http: true,
            ..test_config()
        },
        master_key,
    )
}

pub(crate) fn installation_number() -> u64 {
    installation_id().unsigned_abs()
}

pub(crate) async fn issuing(server: &MockServer, installation_id: u64, token: &str, calls: u64) {
    let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);

    Mock::given(method("POST"))
        .and(path(format!(
            "/app/installations/{installation_id}/access_tokens"
        )))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": token,
            "expires_at": expires_at,
        })))
        .expect(calls)
        .mount(server)
        .await;
}

/// GitHub confirms, once, that installation `installation_id` no longer exists
/// while the App itself is still answered for.
pub(crate) async fn uninstalled(server: &MockServer, installation_id: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/app/installations/{installation_id}")))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/app/installations"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(server)
        .await;
}

/// An App of `organization_id` sealing [`TEST_PRIVATE_KEY`], and an active
/// installation of it numbered `installation_id` on GitHub.
pub(crate) async fn installed(
    pool: &PgPool,
    provider: &Provider,
    organization_id: Uuid,
    installation_id: u64,
) -> Uuid {
    let owner_login = format!("owner-{}", Uuid::new_v4().simple());
    let private_key = provider
        .seal(&SecretValue::new(TEST_PRIVATE_KEY))
        .expect("seal the test private key");
    let app = github_apps::insert(
        pool,
        &NewApp {
            organization_id,
            app_id: rand::random_range(1..i64::MAX),
            slug: "zone-app",
            name: "Zone",
            owner_login: &owner_login,
            owner_type: OwnerType::Organization,
            html_url: "https://github.com/apps/zone-app",
            client_id: "Iv1.client",
            client_secret: "ENC[v1:client-secret]",
            private_key: &private_key,
            webhook_secret: None,
            created_by: None,
        },
    )
    .await
    .expect("insert the test App");
    let installation = github_installations::upsert(
        pool,
        &InstallationUpsert {
            organization_id,
            github_app_id: app.id,
            installation_id: i64::try_from(installation_id).expect("a GitHub installation id"),
            account_login: &owner_login,
            account_type: AccountType::Organization,
            repository_selection: RepositorySelection::All,
            html_url: "https://github.com/organizations/acme/settings/installations/1",
            suspended_at: None,
            created_by: None,
        },
    )
    .await
    .expect("insert the test installation");

    installation.id
}
