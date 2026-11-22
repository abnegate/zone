//! An Enterprise server with one organization, created through the API by the server admin who
//! owns it, and helpers for issuing SCIM tokens and calling the SCIM endpoints as an identity
//! provider would. Every fixture has a database of its own, because the license is server-wide.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::scim::{NewToken, tokens};
use zone_server::services::scim::secret;
use zone_server::state::AppState;

use super::TestResponse;
use super::sso::{self, Account, Audit};

pub const BASE_PATH: &str = "/api/scim/v2/organizations";
pub const CONTENT_TYPE: &str = "application/scim+json";

const TOKEN_NAME: &str = "Identity provider";

pub struct IssuedToken {
    pub id: Uuid,
    pub secret: String,
}

pub struct Fixture {
    pub inner: sso::Fixture,
    pub owner: Account,
    pub organization: Uuid,
}

impl Fixture {
    pub async fn new(prefix: &'static str) -> Self {
        let inner = sso::Fixture::new(prefix).await;
        let owner = inner.register().await;
        inner.license(&owner).await;
        let organization = create_organization(&inner, &owner).await;
        Self {
            inner,
            owner,
            organization,
        }
    }

    pub fn pool(&self) -> &PgPool {
        self.inner.pool()
    }

    pub fn state(&self) -> &AppState {
        self.inner.state()
    }

    pub async fn token(&self) -> IssuedToken {
        self.token_for(self.organization).await
    }

    pub async fn token_for(&self, organization: Uuid) -> IssuedToken {
        let secret = secret::generate();
        let record = tokens::create(
            self.pool(),
            &NewToken {
                organization_id: organization,
                name: TOKEN_NAME,
                token_hash: &secret::hash(&secret),
                hint: &secret::hint(&secret),
                created_by: Some(self.owner.id),
            },
        )
        .await
        .unwrap();
        IssuedToken {
            id: record.id,
            secret,
        }
    }

    pub async fn member(&self, role: OrgRole) -> Account {
        self.member_of(self.organization, role).await
    }

    pub async fn member_of(&self, organization: Uuid, role: OrgRole) -> Account {
        let account = self.inner.register().await;
        organization_members::add_member(self.pool(), organization, account.id, role, None)
            .await
            .unwrap();
        account
    }

    pub async fn stranger(&self) -> Account {
        self.inner.register().await
    }

    pub async fn workspace(&self) -> Uuid {
        let response = self
            .inner
            .client
            .post_json_auth(
                &format!("/api/organizations/{}/workspaces", self.organization),
                &json!({
                    "name": "Provisioned",
                    "slug": format!("provisioned-{}", Uuid::new_v4()),
                }),
                &self.owner.token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["workspace"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    pub async fn another_organization(&self) -> Uuid {
        create_organization(&self.inner, &self.owner).await
    }

    pub fn base(&self) -> String {
        base(self.organization)
    }

    pub async fn scim_get(&self, path: &str, token: &str) -> TestResponse {
        self.scim(Method::GET, &self.uri(path), token, None).await
    }

    pub async fn scim_post(&self, path: &str, token: &str, body: &Value) -> TestResponse {
        self.scim(Method::POST, &self.uri(path), token, Some(body))
            .await
    }

    pub async fn scim_put(&self, path: &str, token: &str, body: &Value) -> TestResponse {
        self.scim(Method::PUT, &self.uri(path), token, Some(body))
            .await
    }

    pub async fn scim_patch(&self, path: &str, token: &str, body: &Value) -> TestResponse {
        self.scim(Method::PATCH, &self.uri(path), token, Some(body))
            .await
    }

    pub async fn scim_delete(&self, path: &str, token: &str) -> TestResponse {
        self.scim(Method::DELETE, &self.uri(path), token, None)
            .await
    }

    /// Sends a SCIM request to `uri` as it is, for a request aimed at another organization's
    /// base or carrying no body where one is expected.
    pub async fn scim(
        &self,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<&Value>,
    ) -> TestResponse {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("Accept", CONTENT_TYPE)
            .header("Authorization", format!("Bearer {token}"));
        let request = match body {
            Some(body) => request
                .header("Content-Type", CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(body).unwrap())),
            None => request.body(Body::empty()),
        };
        self.inner.client.send_request(request.unwrap()).await
    }

    pub async fn audits(&self, action: &str) -> Vec<Audit> {
        self.inner.audits(action).await
    }

    pub async fn cleanup(self) {
        self.inner.cleanup().await;
    }

    fn uri(&self, path: &str) -> String {
        format!("{}{path}", self.base())
    }
}

pub fn base(organization: Uuid) -> String {
    format!("{BASE_PATH}/{organization}")
}

async fn create_organization(inner: &sso::Fixture, owner: &Account) -> Uuid {
    let response = inner
        .client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": "Provisioned",
                "slug": format!("provisioned-{}", Uuid::new_v4()),
            }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["organization"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
