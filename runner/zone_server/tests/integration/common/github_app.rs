//! A GitHub App and its installation, seeded straight into the database, for
//! tests of the routes that attach sources and projects to an installation.

use abnegate_secret::{MasterKey, SecretValue, encrypt_value};
use abnegate_vcs::pull_request::InstallationIssuer;
use uuid::Uuid;
use wiremock::MockServer;
use zone_server::config::Config;
use zone_server::db::github_apps::{self, NewApp, OwnerType};
use zone_server::db::github_installations::{
    self, AccountType, InstallationUpsert, RepositorySelection,
};
use zone_server::services::github_app::Provider;
use zone_server::state::AppState;

use super::test_config;

/// A throwaway RSA key, generated for these tests and used nowhere else.
/// The same key as `services::github_app::testing::TEST_PRIVATE_KEY`.
pub const TEST_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAu5dqhjvoHZtbdSz9bGJAMp3r+1bqzy/9lhjVz5crI2yWhz7I
z3M0H74MQPCHhlOn8nyeOs1cclBLRt7iX7/wgkaP9mGGpvCzLXYdLH4JrrMZj7r5
Pjt0KHrrouTvYXjuCTu2F6QnBcIe+Esz5Vs/+V2fa73I+M1C0sHdX22YNIazcAcB
I9gnaEeS+gINe8LUZDf72lJQc4z+gmVb7dd2R9YEAc90NPpkoeGl7pFwrVHRFlsY
AbJAz6pKc1dVkfTU/yFuw/0d0117InbS1RxTVFEunMWqDJLFwl01THIuVzLDtX93
8cmeMZMcys+n74ZNoA9W6Pv9P2GVQsbnnZV43QIDAQABAoIBAAj9cOq1cMKIeTnU
meGJlNuII3DEYdTjjyLUFl0QOM5GDDG3kc6VTgxuYm5zSH9ov234yGl3gYRt8imX
kWA21dsccBZF5rrV4rRdSnkhIiwn46P2eS7hEQhGmcfQ8mLotXmmTav03zTgsHTE
P9ywQpDcCoGSkxDPX3IzvbzuxuJPdBIIZLraG07haKpemdvNR4UoeMl8blAic3df
NjcFPVD8t4my7EZTgHJtWDqYTW/g/EPwIeMZ/DOEqHDNey9RrKTi4tAr9Uzsszg0
YK2mEFr2B2NG/G5/1X5fh/QV7tPvbKXxFMCiL7RBv9LW3qD8LTs/ovQUxEgOBspV
qyAuCwECgYEA+5Y5Xr0/uedzoGpMLhg86aHdt80cX+Ssi5HkKXwJa5taYCz0gbgU
r7s/9hlGVLiXEjwnXmhWXETq/56b4ebO0u8fcM4ptLrnr+KKWs3AXdjtSvTskOkH
S4QCx6VuZpetyVD6yLZucVes07S6bfIIcRNQto3DPcBCYm+YsNHSdd0CgYEAvuHQ
gazZaxRSTap8IAiRZ2mHUxK3FkVqWrR1aXG/V7OsZprDRVzHz662f+5a27ZCTc4q
2tC8B04tUA0AFWYpk7wh4vnb2VffziarFnCRXhOeOK9/82vxsMwOEX8qgRHs0GK0
1zHTFqUA/lsA6bcqQZYmOn4r0dSPJMahp5i+XwECgYB/7/nGqrhwYjnTdpq8ygiX
yn+Ei2KFhTUVWKBNVE06EmtYAyRnnuOuJau2C05PoPr6A+sFQEvCai2SxeaBbyz3
6S/03nIo/O7661nuKTlMwBaTio+OdWIHTd9YBVFqDHIMsQiG7vak3q/9jKdNZ8pR
LkBaRSbnDRD1G8jrChhbZQKBgFkaD7p4dQUG92RJsKdDWJxtJj4g/lXnET5F/oi6
EBdgR5mdpIk8RgksBQSyvrbQ3SJ0moyJ4zuFwqEbcG6Mwdu0dhz9hSJvYolYg4R2
B2Viwviy84ctXCSrG+YO9khJlcGwUboiB+cKHuycjlCKr67t5+pl+w53qloAXnVd
V4ABAoGAJQaRxv9kpn6TMTCDtTPtWpVyxymF0A58t8/Yy3lcC3HS3V5hDRZlBGyL
snLljJnOKrZGf+JfNkPul+la0f9cJo9Ka/oCy6X243y8VmzRG6/BX6eEfnWEJ+NB
+ehfHUDVhe3oJRr15Rw1gt7CrSORoe46YkbnxO3H80of1gn9Vyw=
-----END RSA PRIVATE KEY-----";

pub const APP_ID: i64 = 424_242;

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

/// A provider that mints installation tokens from `server` instead of GitHub.
pub fn issuer_provider(server: &MockServer, state: &AppState) -> Provider {
    Provider::with_issuer(
        InstallationIssuer::standing_in_for(&server.uri()).expect("a stand-in issuer"),
        *state.encryption_key(),
    )
}

/// The test config with every GitHub API call sent to `server`.
pub fn config_for(server: &MockServer) -> Config {
    let mut config = test_config();
    config.github_api_url = server.uri();
    config.github_api_allows_http = true;
    config
}
