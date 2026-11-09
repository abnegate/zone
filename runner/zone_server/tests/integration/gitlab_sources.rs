//! A GitLab source's stored access token is only ever sent to the instance it
//! was entered for: moving the source to another instance needs the token
//! again.

use crate::common;
use crate::common::TestClient;
use crate::common::TestResponse;
use crate::common::test_email;
use crate::common::test_password;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use axum::http::StatusCode;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::sources;
use zone_server::db::sources::GITLAB;
use zone_server::db::sources::NewSource;
use zone_server::db::sources::SourceRow;

const STORED_TOKEN: &str = "glpat-throwaway-stored";
const SENT_TOKEN: &str = "glpat-throwaway-sent";
const PROJECT: &str = "group/project";
const STORED_INSTANCE: &str = "https://127.0.0.1:9";
const OTHER_INSTANCE: &str = "https://localhost:9";
const GITLAB_COM: &str = "https://gitlab.com";
const NEEDS_TOKEN: &str =
    "Re-enter the access token to read this source from another GitLab instance.";
const HOST_FIELD: &str = "host";
const BASE_URL_FIELD: &str = "base_url";
const INDEX_SETTLE: Duration = Duration::from_millis(300);

fn config(field: &str, instance: Option<&str>, branch: &str) -> Value {
    let mut config = json!({ "project_id": PROJECT, "branch": branch });
    if let Some(instance) = instance {
        config[field] = json!(instance);
    }
    config
}

struct Tenant {
    token: String,
    user: Uuid,
    workspace: Uuid,
}

struct Harness {
    client: TestClient,
}

impl Harness {
    async fn indexing() -> Self {
        Self {
            client: TestClient::with_db().await,
        }
    }

    /// Accepted updates spawn indexing, which would read the instance; these
    /// tests only assert what the route stores.
    async fn new() -> Self {
        let harness = Self::indexing().await;
        harness.client.state().index_semaphore().close();
        harness
    }

    fn pool(&self) -> &PgPool {
        self.client.state().db()
    }

    async fn tenant(&self) -> Tenant {
        let suffix = Uuid::new_v4().simple().to_string();
        let registered = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({ "email": test_email(), "password": test_password() }),
            )
            .await
            .json_value();
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("register returns an access token, got {registered}"))
            .to_string();
        let user = registered["user"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("register returns the user, got {registered}"));
        let organization: Uuid = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "GitLab Org", "slug": format!("gitlab-org-{suffix}") }),
                &token,
            )
            .await
            .json_value()["organization"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("an organization");
        let workspace = self
            .client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({ "name": "GitLab Workspace", "slug": format!("gitlab-ws-{suffix}") }),
                &token,
            )
            .await
            .json_value()["workspace"]["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .expect("a workspace");
        Tenant {
            token,
            user,
            workspace,
        }
    }

    async fn source(&self, tenant: &Tenant, config: Value, token: Option<&str>) -> SourceRow {
        let sealed = token.map(|token| {
            encrypt_value(&SecretValue::new(token), self.client.state().master_key())
                .expect("the token seals")
        });
        sources::create_source(
            self.pool(),
            NewSource {
                workspace_id: tenant.workspace,
                name: &format!("Project {}", Uuid::new_v4()),
                source_type: GITLAB,
                config,
                description: None,
                url: None,
                credentials_encrypted: sealed.as_deref(),
                github_installation_id: None,
            },
        )
        .await
        .expect("the source is stored")
    }

    async fn stored(&self, tenant: &Tenant, id: Uuid) -> SourceRow {
        sources::get_source(self.pool(), id, tenant.workspace)
            .await
            .expect("the source is readable")
            .expect("the source exists")
    }

    async fn update(&self, tenant: &Tenant, id: Uuid, body: Value) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/workspaces/{}/sources/{id}", tenant.workspace),
                &body,
                &tenant.token,
            )
            .await
    }

    async fn gatherings(&self, source: Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM context_gatherings WHERE $1 = ANY(source_ids)")
            .bind(source)
            .fetch_one(self.pool())
            .await
            .expect("the source's gatherings are countable")
    }

    async fn discard(&self, tenant: &Tenant) {
        common::discard(self.pool(), tenant.workspace, &[tenant.user]).await;
    }
}

fn assert_unchanged(before: &SourceRow, after: &SourceRow) {
    assert_eq!(
        after.config, before.config,
        "the stored config is untouched"
    );
    assert_eq!(
        after.credentials_encrypted, before.credentials_encrypted,
        "the stored token is untouched"
    );
    assert_eq!(
        after.updated_at, before.updated_at,
        "the row is not written"
    );
}

#[tokio::test]
async fn moving_a_source_to_another_instance_without_its_token_is_refused() {
    let harness = Harness::indexing().await;
    let tenant = harness.tenant().await;

    for field in [HOST_FIELD, BASE_URL_FIELD] {
        for (stored, moved) in [
            (Some(STORED_INSTANCE), Some(OTHER_INSTANCE)),
            (None, Some(OTHER_INSTANCE)),
            (Some(GITLAB_COM), Some(OTHER_INSTANCE)),
            (Some(STORED_INSTANCE), None),
            (Some(STORED_INSTANCE), Some(GITLAB_COM)),
            (Some(STORED_INSTANCE), Some("https://127.0.0.1:9/gitlab")),
            (Some(STORED_INSTANCE), Some("http://127.0.0.1:9")),
        ] {
            let source = harness
                .source(&tenant, config(field, stored, "main"), Some(STORED_TOKEN))
                .await;

            let refused = harness
                .update(
                    &tenant,
                    source.id,
                    json!({ "config": config(field, moved, "main") }),
                )
                .await;

            refused.assert_status(StatusCode::BAD_REQUEST);
            assert_eq!(
                refused.json_value()["error"].as_str(),
                Some(NEEDS_TOKEN),
                "{field} {stored:?} -> {moved:?}"
            );
            assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
            tokio::time::sleep(INDEX_SETTLE).await;
            assert_eq!(
                harness.gatherings(source.id).await,
                0,
                "a refused update indexes nothing: {field} {stored:?} -> {moved:?}"
            );
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn moving_a_source_to_another_instance_with_a_new_token_is_accepted() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            Some(STORED_TOKEN),
        )
        .await;
    let moved = config(HOST_FIELD, Some(OTHER_INSTANCE), "main");

    let accepted = harness
        .update(
            &tenant,
            source.id,
            json!({ "config": moved, "credentials": SENT_TOKEN }),
        )
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert!(stored.credentials_encrypted.is_some());
    assert_ne!(
        stored.credentials_encrypted, source.credentials_encrypted,
        "the new token replaces the stored one"
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn respelling_the_same_instance_without_a_token_is_accepted() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for (field, stored, respelled) in [
        (
            HOST_FIELD,
            Some(STORED_INSTANCE),
            "https://127.0.0.1:9/api/v4",
        ),
        (HOST_FIELD, Some(STORED_INSTANCE), "https://127.0.0.1:9/"),
        (
            BASE_URL_FIELD,
            Some(STORED_INSTANCE),
            "  https://127.0.0.1:9/api/v4/  ",
        ),
        (
            HOST_FIELD,
            Some("https://GitLab.example.com"),
            "https://gitlab.example.com",
        ),
        (HOST_FIELD, None, GITLAB_COM),
        (HOST_FIELD, Some(GITLAB_COM), "https://gitlab.com/api/v4"),
    ] {
        let source = harness
            .source(&tenant, config(field, stored, "main"), Some(STORED_TOKEN))
            .await;
        let respelled = config(field, Some(respelled), "main");

        let accepted = harness
            .update(&tenant, source.id, json!({ "config": respelled }))
            .await;

        accepted.assert_status(StatusCode::OK);
        let after = harness.stored(&tenant, source.id).await;
        assert_eq!(after.config, respelled, "{field} {stored:?}");
        assert_eq!(after.credentials_encrypted, source.credentials_encrypted);
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_config_change_on_the_same_instance_keeps_the_stored_token() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            Some(STORED_TOKEN),
        )
        .await;
    let branched = config(HOST_FIELD, Some(STORED_INSTANCE), "develop");

    let accepted = harness
        .update(&tenant, source.id, json!({ "config": branched }))
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, branched);
    assert_eq!(stored.credentials_encrypted, source.credentials_encrypted);

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_source_with_no_stored_token_may_move_to_another_instance() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            None,
        )
        .await;
    let moved = config(HOST_FIELD, Some(OTHER_INSTANCE), "main");

    let accepted = harness
        .update(&tenant, source.id, json!({ "config": moved }))
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(stored.credentials_encrypted, None);

    harness.discard(&tenant).await;
}
