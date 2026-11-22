//! Single sign-on administration, OIDC and SAML alike, belongs to the server
//! admin. Providers are server-wide, so no organization role reaches them:
//! not a built-in owner or admin, and not a custom role holding every
//! composable permission, since none of those permissions is about single
//! sign-on.

use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::sso::saml::{service_metadata, testing};
use zone_server::auth::Scope;
use zone_server::auth::admin_guard::REQUIRED;
use zone_server::db::organization_members::OrgRole;

use super::{Call, composable, describe, licensed, organization_holder, workspace_holder};
use crate::common::identity_provider::IdentityProvider;
use crate::common::policy::Fixture;
use crate::common::sso::{Account, ENFORCEMENT, PROVIDERS, SETTINGS};

const METADATA_PATH: &str = "/metadata";
const DOMAIN: &str = "acme.com";
const SAML_NAME: &str = "Okta SAML";
const RENAMED: &str = "Renamed";
/// The server admin's create and refresh; a refused caller never fetches.
const METADATA_FETCHES: u64 = 2;

struct Providers {
    oidc: Uuid,
    saml: Uuid,
    metadata_url: String,
}

fn provider_path(id: Uuid) -> String {
    format!("{PROVIDERS}/{id}")
}

fn saml_provider(metadata_url: &str) -> Value {
    json!({
        "protocol": "saml",
        "display_name": SAML_NAME,
        "metadata_url": metadata_url,
        "allowed_domains": [DOMAIN],
    })
}

fn pasted_saml_provider() -> Value {
    json!({
        "protocol": "saml",
        "display_name": SAML_NAME,
        "metadata": testing::metadata(testing::ENTITY_ID, testing::SSO_URL, &[testing::CERTIFICATE]),
        "allowed_domains": [DOMAIN],
    })
}

async fn metadata_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(METADATA_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    header::CONTENT_TYPE.as_str(),
                    service_metadata::CONTENT_TYPE,
                )
                .set_body_string(testing::metadata(
                    testing::ENTITY_ID,
                    testing::SSO_URL,
                    &[testing::CERTIFICATE],
                )),
        )
        .expect(METADATA_FETCHES)
        .mount(&server)
        .await;
    server
}

/// Every single sign-on administration route, each with a body it accepts
/// from the server admin.
fn administration(identity_provider: &IdentityProvider, providers: &Providers) -> Vec<Call> {
    vec![
        Call::get(SETTINGS),
        Call::post(PROVIDERS, identity_provider.provider()),
        Call::post(PROVIDERS, pasted_saml_provider()),
        Call::patch(
            provider_path(providers.oidc),
            json!({ "display_name": RENAMED }),
        ),
        Call::patch(
            provider_path(providers.saml),
            json!({ "display_name": RENAMED }),
        ),
        Call::patch(
            provider_path(providers.saml),
            json!({ "metadata_url": providers.metadata_url }),
        ),
        Call::delete(provider_path(providers.oidc)),
        Call::delete(provider_path(providers.saml)),
        Call::put(ENFORCEMENT, json!({ "required": true })),
        Call::put(ENFORCEMENT, json!({ "required": false })),
    ]
}

async fn callers(fixture: &Fixture) -> Vec<(&'static str, Account)> {
    vec![
        (
            "an organization owner who is not the server admin",
            fixture.member(OrgRole::Owner, None).await,
        ),
        (
            "an organization admin",
            fixture.member(OrgRole::Admin, None).await,
        ),
        (
            "an organization member",
            fixture.member(OrgRole::Member, None).await,
        ),
        (
            "a holder of every composable organization permission",
            organization_holder(fixture, &composable(Scope::Organization)).await,
        ),
        (
            "a holder of every composable workspace permission",
            workspace_holder(fixture, &composable(Scope::Workspace)).await,
        ),
    ]
}

#[tokio::test]
async fn no_organization_role_administers_single_sign_on() {
    let fixture = licensed().await;
    let identity_provider = IdentityProvider::start().await;
    let metadata = metadata_server().await;
    let metadata_url = format!("{}{METADATA_PATH}", metadata.uri());
    let admin = &fixture.owner.token;
    let providers = Providers {
        oidc: fixture
            .sso
            .created(admin, &identity_provider.provider())
            .await,
        saml: fixture
            .sso
            .created(admin, &saml_provider(&metadata_url))
            .await,
        metadata_url,
    };
    let refusal = json!({ "error": REQUIRED });
    let mut departures = Vec::new();

    for (caller, account) in callers(&fixture).await {
        for call in administration(&identity_provider, &providers) {
            let response = call.send(&fixture, &account.token).await;
            if response.status != StatusCode::FORBIDDEN || response.json_value() != refusal {
                departures.push(format!(
                    "{caller} is not refused: {}",
                    describe(&call, &response)
                ));
            }
        }
    }

    let settings = fixture.sso.settings(admin).await;
    let names: Vec<Value> = settings.json_value()["providers"]
        .as_array()
        .expect("the settings list providers")
        .iter()
        .map(|provider| provider["display_name"].clone())
        .collect();
    if names.len() != 2 || names.contains(&json!(RENAMED)) {
        departures.push(format!("a refused caller changed the providers: {names:?}"));
    }

    let admitted = [
        (Call::get(SETTINGS), StatusCode::OK),
        (
            Call::patch(
                provider_path(providers.oidc),
                json!({ "display_name": RENAMED }),
            ),
            StatusCode::OK,
        ),
        (
            Call::patch(
                provider_path(providers.saml),
                json!({ "metadata_url": providers.metadata_url }),
            ),
            StatusCode::OK,
        ),
        (
            Call::put(ENFORCEMENT, json!({ "required": false })),
            StatusCode::OK,
        ),
        (
            Call::delete(provider_path(providers.oidc)),
            StatusCode::NO_CONTENT,
        ),
        (
            Call::delete(provider_path(providers.saml)),
            StatusCode::NO_CONTENT,
        ),
    ];
    for (call, expected) in admitted {
        let response = call.send(&fixture, admin).await;
        if response.status != expected {
            departures.push(format!(
                "the server admin is answered {} instead of {expected}: {}",
                response.status,
                describe(&call, &response)
            ));
        }
    }

    metadata.verify().await;
    fixture.cleanup().await;
    assert!(departures.is_empty(), "{}", departures.join("\n"));
}
