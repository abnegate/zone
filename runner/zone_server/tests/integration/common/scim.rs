//! An Enterprise server with one organization, created through the API by the server admin who
//! owns it, and helpers for issuing SCIM tokens and calling the SCIM endpoints as an identity
//! provider would, replaying the requests Okta and Entra ID send. Every fixture has a database of
//! its own, because the license is server-wide.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{self, Creation, Definition};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::scim::{NewToken, tokens};
use zone_server::services::scim::secret;
use zone_server::state::AppState;

use super::TestResponse;
use super::sso::{self, Account, Audit};

pub const BASE_PATH: &str = "/api/scim/v2/organizations";
pub const CONTENT_TYPE: &str = "application/scim+json";
pub const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
pub const GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
pub const PATCH_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
pub const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
pub const LIST_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../zone_enterprise/src/scim/tests/fixtures"
);
const ENTRA_LOOKUPS: &str = "entra/filters.txt";

const TOKEN_NAME: &str = "Identity provider";

pub struct IssuedToken {
    pub id: Uuid,
    pub secret: String,
}

/// The values a recorded request names, put in place of its `{{user}}`, `{{user_name}}`,
/// `{{external_id}}` and `{{group}}`.
#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    pub user: Option<Uuid>,
    pub user_name: Option<String>,
    pub external_id: Option<String>,
    pub group: Option<Uuid>,
}

impl Placeholders {
    /// Fills every placeholder in `text`, failing the test if one is left without a value.
    pub fn substitute(&self, text: &str) -> String {
        let values = [
            ("{{user}}", self.user.map(|user| user.to_string())),
            ("{{user_name}}", self.user_name.clone()),
            ("{{external_id}}", self.external_id.clone()),
            ("{{group}}", self.group.map(|group| group.to_string())),
        ];
        let mut filled = text.to_owned();
        for (placeholder, value) in values {
            if let Some(value) = value {
                filled = filled.replace(placeholder, &value);
            }
        }
        assert!(
            !filled.contains("{{"),
            "a placeholder has no value in {filled}"
        );
        filled
    }
}

/// A recorded file, such as `okta/create_user.json`, as it is on disk.
pub fn recording(fixture: &str) -> String {
    let path = format!("{FIXTURES}/{fixture}");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A recorded request body, such as `okta/create_user`, with its placeholders filled.
pub fn replay(fixture: &str, placeholders: &Placeholders) -> Value {
    let text = placeholders.substitute(&recording(&format!("{fixture}.json")));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{fixture}: {error}"))
}

/// Line `line`, counted from 1, of Entra's recorded lookups, filled and encoded as a path relative
/// to the organization's base.
pub fn entra_lookup(line: usize, placeholders: &Placeholders) -> String {
    let recorded = recording(ENTRA_LOOKUPS);
    let lookup = recorded
        .lines()
        .nth(line - 1)
        .unwrap_or_else(|| panic!("{ENTRA_LOOKUPS} has no line {line}"));
    let encoded = placeholders
        .substitute(lookup)
        .replace(' ', "%20")
        .replace('"', "%22");
    format!("/{encoded}")
}

pub fn assert_scim(response: &TestResponse, status: StatusCode) {
    response.assert_status(status);
    assert_eq!(
        response.header("content-type"),
        Some(CONTENT_TYPE),
        "{}",
        response.text()
    );
}

/// A refusal in the SCIM error format: the status as a string, and `scimType` only when given.
pub fn assert_refused(response: &TestResponse, status: StatusCode, scim_type: Option<&str>) {
    assert_scim(response, status);
    let body = response.json_value();
    assert_eq!(body["schemas"], json!([ERROR_SCHEMA]), "{body}");
    assert_eq!(body["status"], json!(status.as_u16().to_string()), "{body}");
    match scim_type {
        Some(scim_type) => assert_eq!(body["scimType"], json!(scim_type), "{body}"),
        None => assert!(body.get("scimType").is_none(), "{body}"),
    }
}

pub fn resource_id(response: &TestResponse) -> Uuid {
    response.json_value()["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {}", response.text()))
        .parse()
        .unwrap()
}

pub struct Fixture {
    pub inner: sso::Fixture,
    pub owner: Account,
    pub organization: Uuid,
}

impl Fixture {
    pub async fn new(prefix: &'static str) -> Self {
        Self::licensed(sso::Fixture::new(prefix).await).await
    }

    /// As [`Fixture::new`], with the Community member cap in force.
    pub async fn capped(prefix: &'static str) -> Self {
        Self::licensed(sso::Fixture::capped(prefix).await).await
    }

    async fn licensed(inner: sso::Fixture) -> Self {
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

    /// An organization that `owner` creates, so that this fixture's owner isn't a member of it.
    pub async fn organization_of(&self, owner: &Account) -> Uuid {
        create_organization(&self.inner, owner).await
    }

    /// An organization-scope custom role holding the scope's base and `permissions`, created by an
    /// owner of `organization`.
    pub async fn custom_role(
        &self,
        organization: Uuid,
        name: &str,
        permissions: &[Permission],
    ) -> Uuid {
        let creator: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM organization_members \
             WHERE organization_id = $1 AND role = $2 AND is_active \
             ORDER BY created_at LIMIT 1",
        )
        .bind(organization)
        .bind(OrgRole::Owner.as_str())
        .fetch_one(self.pool())
        .await
        .unwrap();
        let names: Vec<String> = permissions
            .iter()
            .map(|permission| permission.name().to_owned())
            .collect();
        let grant = Grant::compose(Scope::Organization, &names).unwrap();
        let definition = Definition {
            name,
            description: None,
            scope: Scope::Organization,
            grant: &grant,
        };
        match custom_roles::create(self.pool(), organization, &definition, creator)
            .await
            .unwrap()
        {
            Creation::Created(role) => role.id,
            Creation::Forbidden => panic!("the creator does not hold roles:update"),
            Creation::NameTaken => panic!("{name} is already taken"),
        }
    }

    /// Maps a pushed group in the console, as the organization's owner.
    pub async fn map(&self, group: Uuid, mapping: &Value) -> TestResponse {
        self.inner
            .client
            .put_json_auth(
                &format!(
                    "/api/organizations/{}/scim/groups/{group}/mapping",
                    self.organization
                ),
                mapping,
                &self.owner.token,
            )
            .await
    }

    /// Whether `token` still opens a session.
    pub async fn signed_in(&self, token: &str) -> bool {
        let response = self
            .inner
            .client
            .get_auth("/api/auth/sessions", token)
            .await;
        match response.status {
            StatusCode::OK => true,
            StatusCode::UNAUTHORIZED => false,
            status => panic!("the session answered {status}: {}", response.text()),
        }
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

    pub async fn provision(&self, token: &str, user_name: &str) -> Uuid {
        let response = self
            .scim_post(
                "/Users",
                token,
                &json!({
                    "schemas": [USER_SCHEMA],
                    "userName": user_name,
                    "name": { "givenName": "Ada", "familyName": "Lovelace" },
                    "emails": [{ "value": user_name, "type": "work", "primary": true }],
                    "active": true,
                }),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    pub async fn holding(&self, user: Uuid) -> Option<(String, Option<Uuid>, bool)> {
        sqlx::query_as(
            "SELECT role, custom_role_id, is_active FROM organization_members \
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(self.organization)
        .bind(user)
        .fetch_optional(self.pool())
        .await
        .unwrap()
    }

    pub async fn seat(&self, workspace: Uuid, user: Uuid) -> Option<(String, bool)> {
        sqlx::query_as(
            "SELECT role, is_active FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace)
        .bind(user)
        .fetch_optional(self.pool())
        .await
        .unwrap()
    }

    pub async fn audits(&self, action: &str) -> Vec<Audit> {
        self.inner.audits(action).await
    }

    /// The actions of every audit row a SCIM request wrote, sorted, failing the test if any of
    /// them names an actor.
    pub async fn scim_actions(&self) -> Vec<String> {
        let rows: Vec<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT action, actor_id FROM audit_logs WHERE new_values ? 'scim_token_id'",
        )
        .fetch_all(self.pool())
        .await
        .unwrap();
        for (action, actor) in &rows {
            assert_eq!(*actor, None, "{action} names an actor");
        }
        let mut actions: Vec<String> = rows.into_iter().map(|(action, _)| action).collect();
        actions.sort();
        actions
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
