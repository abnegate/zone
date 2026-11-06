//! A GitHub App and its installation, seeded straight into the database, for
//! tests of the routes that attach sources and projects to an installation.

pub use zone_server::services::github_app::testing::TEST_PRIVATE_KEY;

use abnegate_secret::{MasterKey, SecretValue, encrypt_value};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_server::config::Config;
use zone_server::db::github_apps::{self, NewApp, OwnerType};
use zone_server::db::github_installations::{
    self, AccountType, InstallationUpsert, RepositorySelection,
};
use zone_server::services::github_app::Provider;
use zone_server::state::AppState;

use super::test_config;

pub const APP_ID: i64 = 424_242;

/// A GitHub installation number no other test uses, since the database holds
/// each of GitHub's installations once.
pub fn installation_id() -> i64 {
    rand::random_range(1..i64::MAX)
}

pub struct Seeded {
    pub app: Uuid,
    pub installation: Uuid,
    pub installation_id: i64,
    pub account_login: String,
}

/// An App registered by `organization_id` and one active installation of it,
/// on an account with a fresh login, its secrets sealed under the state's key
/// the way `Provider::seal` seals them.
pub async fn seed_installation(
    state: &AppState,
    organization_id: Uuid,
    installation_id: i64,
) -> Seeded {
    let master_key = MasterKey::new(*state.encryption_key());
    let seal = |secret: &str| {
        encrypt_value(&SecretValue::new(secret), &master_key).expect("a test secret seals")
    };
    let login = format!("zone-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let private_key = seal(TEST_PRIVATE_KEY);
    let client_secret = seal("throwaway-client-secret");
    let html_url = format!("https://github.com/apps/{login}");

    let app = github_apps::insert(
        state.db(),
        &NewApp {
            organization_id,
            app_id: APP_ID,
            slug: &login,
            name: &login,
            owner_login: &login,
            owner_type: OwnerType::Organization,
            html_url: &html_url,
            client_id: "Iv1.throwaway",
            client_secret: &client_secret,
            private_key: &private_key,
            webhook_secret: None,
            created_by: None,
        },
    )
    .await
    .expect("the organization registers its App");

    let installation = github_installations::upsert(
        state.db(),
        &InstallationUpsert {
            organization_id,
            github_app_id: app.id,
            installation_id,
            account_login: &login,
            account_type: AccountType::Organization,
            repository_selection: RepositorySelection::All,
            html_url: &format!(
                "https://github.com/organizations/{login}/settings/installations/{installation_id}"
            ),
            suspended_at: None,
            created_by: None,
        },
    )
    .await
    .expect("the App is installed on the account");

    Seeded {
        app: app.id,
        installation: installation.id,
        installation_id,
        account_login: login,
    }
}

/// `server` confirms, once, that installation `installation_id` no longer
/// exists while the App itself is still answered for.
pub async fn uninstalled(server: &MockServer, installation_id: i64) {
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

/// A provider that mints installation tokens from `server` instead of GitHub.
pub fn issuer_provider(server: &MockServer, state: &AppState) -> Provider {
    Provider::new(&config_for(server), *state.encryption_key())
}

/// The test config with every GitHub API call sent to `server`.
pub fn config_for(server: &MockServer) -> Config {
    let mut config = test_config();
    config.github_api_url = server.uri();
    config.github_api_allows_http = true;
    config
}
