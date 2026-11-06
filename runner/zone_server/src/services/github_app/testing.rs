//! Fixtures shared by this module's tests.

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::InstallationIssuer;
use chrono::{Duration, SecondsFormat, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::db::github_apps::{self, NewApp, OwnerType};
use crate::db::github_installations::{self, AccountType, InstallationUpsert, RepositorySelection};

use super::Provider;

/// A throwaway RSA key, generated for these tests and used nowhere else.
pub(crate) const TEST_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
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

pub(crate) fn provider(server: &MockServer, encryption_key: [u8; 32]) -> Provider {
    Provider::with_issuer(
        InstallationIssuer::standing_in_for(&server.uri()).expect("stand-in issuer"),
        encryption_key,
    )
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

/// An App of `organization_id` whose private key is [`TEST_PRIVATE_KEY`],
/// sealed by `provider`, and an active installation of it numbered
/// `installation_id` on GitHub. Returns the installation's id.
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
