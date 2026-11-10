//! A GitLab source's stored access token is only ever sent to the instance it
//! was entered for: moving the source to another instance needs the token
//! again. Its link points at the web page of that instance, never another.

use crate::common;
use crate::common::TestClient;
use crate::common::TestResponse;
use crate::common::test_email;
use crate::common::test_password;
use abnegate_secret::SecretValue;
use abnegate_secret::decrypt_value;
use abnegate_secret::encrypt_value;
use axum::http::StatusCode;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::Transaction;
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::sources;
use zone_server::db::sources::GITHUB;
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
const INVALID_INSTANCE: &str = "Invalid source configuration: Invalid GitLab config: Invalid GitLab origin: expected an HTTPS (or opted-in HTTP) URL with no query, fragment or credentials, outside link-local and metadata addresses";
const BOTH_NAMED: &str = "Invalid source configuration: Invalid GitLab config: name the instance as host or base_url, not both";
const NON_STRING_NAMED: &str =
    "Invalid source configuration: Invalid GitLab config: name the instance as a URL string";
const INSTANCE_MOVED: &str =
    "The source reads from another GitLab instance now. Reload it and re-enter the access token.";
const UNREADABLE_NEEDS_TOKEN: &str = "The stored configuration names no readable GitLab instance. Re-enter the access token to change it.";
const UNNAMED_TOKEN_INSTANCE: &str =
    "Send `instance`, the GitLab instance this access token is for.";
const UNREADABLE_TOKEN_INSTANCE: &str =
    "Name the GitLab instance this access token is for as a URL a source can read from.";
const STORED_LINK: &str = "https://127.0.0.1:9/group/project";
const OTHER_LINK: &str = "https://localhost:9/group/project";
const EXPLICIT_LINK: &str = "https://docs.example.test/project";
const GITLAB_COM_LINK: &str = "https://gitlab.com/group/project";
const NUMERIC_PROJECT: &str = "42";
const HOST_FIELD: &str = "host";
const BASE_URL_FIELD: &str = "base_url";
const LOCK_WAIT: Duration = Duration::from_secs(10);
const LOCK_POLL: Duration = Duration::from_millis(10);

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
    /// Accepted updates spawn indexing, which would read the instance; these
    /// tests only assert what the route stores.
    async fn new() -> Self {
        let harness = Self {
            client: TestClient::with_db().await,
        };
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

    fn seal(&self, token: &str) -> String {
        encrypt_value(&SecretValue::new(token), self.client.state().master_key())
            .expect("the token seals")
    }

    fn open(&self, sealed: &str) -> String {
        decrypt_value(sealed, self.client.state().master_key())
            .expect("the token opens")
            .expose()
            .to_string()
    }

    async fn source(&self, tenant: &Tenant, config: Value, token: Option<&str>) -> SourceRow {
        self.typed_source(tenant, GITLAB, config, token).await
    }

    async fn typed_source(
        &self,
        tenant: &Tenant,
        source_type: &str,
        config: Value,
        token: Option<&str>,
    ) -> SourceRow {
        let sealed = token.map(|token| self.seal(token));
        sources::create_source(
            self.pool(),
            NewSource {
                workspace_id: tenant.workspace,
                name: &format!("Project {}", Uuid::new_v4()),
                source_type,
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

    /// A source stored with `link`, as released code stored the link it
    /// derived at creation
    async fn linked(&self, tenant: &Tenant, config: Value, link: &str) -> SourceRow {
        sources::create_source(
            self.pool(),
            NewSource {
                workspace_id: tenant.workspace,
                name: &format!("Project {}", Uuid::new_v4()),
                source_type: GITLAB,
                config,
                description: None,
                url: Some(link),
                credentials_encrypted: None,
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

    async fn create(&self, tenant: &Tenant, body: Value) -> TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/workspaces/{}/sources", tenant.workspace),
                &body,
                &tenant.token,
            )
            .await
    }

    async fn count(&self, tenant: &Tenant) -> usize {
        sources::list_sources(self.pool(), tenant.workspace, None, None, 100, 0)
            .await
            .expect("the sources are listable")
            .len()
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

    /// Writes `config` and `sealed` to the source in a transaction left open,
    /// so the row stays locked until the caller commits it.
    async fn concurrent(
        &self,
        id: Uuid,
        config: Option<&Value>,
        sealed: Option<&str>,
    ) -> (Transaction<'static, Postgres>, i32) {
        let mut concurrent = self.pool().begin().await.expect("a transaction");
        let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *concurrent)
            .await
            .expect("the backend is named");
        sqlx::query(
            "UPDATE sources SET config = COALESCE($2, config), credentials_encrypted = COALESCE($3, credentials_encrypted) WHERE id = $1",
        )
        .bind(id)
        .bind(config)
        .bind(sealed)
        .execute(&mut *concurrent)
        .await
        .expect("the concurrent write lands");
        (concurrent, backend)
    }

    async fn blocked_by(&self, backend: i32) {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE NOT granted AND $1 = ANY(pg_blocking_pids(pid)))",
            )
            .bind(backend)
            .fetch_one(self.pool())
            .await
            .expect("the lock table is readable");
            if waiting {
                return;
            }
            tokio::time::sleep(LOCK_POLL).await;
        }
    }

    async fn wait_until_blocked_by(&self, backend: i32) {
        tokio::time::timeout(LOCK_WAIT, self.blocked_by(backend))
            .await
            .expect("the update waits on the row the concurrent write holds");
    }

    async fn created(&self, tenant: &Tenant, body: Value) -> (Uuid, Value) {
        let response = self.create(tenant, body).await;
        response.assert_status(StatusCode::CREATED);
        let source = response.json_value()["source"].clone();
        let id = source["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("create returns the source, got {source}"));
        (id, source)
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
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for field in [HOST_FIELD, BASE_URL_FIELD] {
        for (stored, moved) in [
            (Some(STORED_INSTANCE), Some(OTHER_INSTANCE)),
            (None, Some(OTHER_INSTANCE)),
            (Some(GITLAB_COM), Some(OTHER_INSTANCE)),
            (Some(STORED_INSTANCE), None),
            (Some(STORED_INSTANCE), Some(GITLAB_COM)),
            (Some(STORED_INSTANCE), Some("https://127.0.0.1:9/gitlab")),
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
    assert_eq!(
        harness.open(stored.credentials_encrypted.as_deref().expect("a token")),
        SENT_TOKEN,
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

#[tokio::test]
async fn a_token_stored_while_the_move_waits_is_never_paired_with_the_new_instance() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            None,
        )
        .await;
    let sealed = harness.seal(STORED_TOKEN);
    let (concurrent, backend) = harness.concurrent(source.id, None, Some(&sealed)).await;

    let (moved, ()) = tokio::join!(
        harness.update(
            &tenant,
            source.id,
            json!({ "config": config(HOST_FIELD, Some(OTHER_INSTANCE), "main") }),
        ),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent token commits");
        },
    );

    moved.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(moved.json_value()["error"].as_str(), Some(NEEDS_TOKEN));
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, source.config, "the instance did not move");
    assert_eq!(
        stored.credentials_encrypted.as_deref(),
        Some(sealed.as_str())
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn an_edit_waiting_on_a_move_is_judged_against_the_moved_instance() {
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
    let sealed = harness.seal(SENT_TOKEN);
    let (concurrent, backend) = harness
        .concurrent(source.id, Some(&moved), Some(&sealed))
        .await;

    let (edited, ()) = tokio::join!(
        harness.update(
            &tenant,
            source.id,
            json!({ "config": config(HOST_FIELD, Some(STORED_INSTANCE), "develop") }),
        ),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent move commits");
        },
    );

    edited.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(edited.json_value()["error"].as_str(), Some(NEEDS_TOKEN));
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(
        stored.config, moved,
        "the edit did not move the source back"
    );
    assert_eq!(
        harness.open(stored.credentials_encrypted.as_deref().expect("a token")),
        SENT_TOKEN,
        "the moved instance's token is never paired with the old instance"
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_token_entered_for_an_instance_the_source_left_is_refused_unstored() {
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
    let (concurrent, backend) = harness.concurrent(source.id, Some(&moved), None).await;

    let (entered, ()) = tokio::join!(
        harness.update(
            &tenant,
            source.id,
            json!({ "credentials": SENT_TOKEN, "instance": STORED_INSTANCE }),
        ),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent move commits");
        },
    );

    entered.assert_status(StatusCode::CONFLICT);
    assert_eq!(entered.json_value()["error"].as_str(), Some(INSTANCE_MOVED));
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(
        stored.credentials_encrypted, None,
        "the token entered for the old instance is never paired with the new one"
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_token_is_stored_only_while_the_source_reads_from_the_instance_it_names() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for (stored, named, accepted) in [
        (Some(STORED_INSTANCE), STORED_INSTANCE, true),
        (Some(STORED_INSTANCE), "https://127.0.0.1:9/api/v4/", true),
        (None, GITLAB_COM, true),
        (Some(GITLAB_COM), "", true),
        (Some(STORED_INSTANCE), OTHER_INSTANCE, false),
        (Some(STORED_INSTANCE), GITLAB_COM, false),
        (None, OTHER_INSTANCE, false),
    ] {
        let source = harness
            .source(
                &tenant,
                config(HOST_FIELD, stored, "main"),
                Some(STORED_TOKEN),
            )
            .await;

        let response = harness
            .update(
                &tenant,
                source.id,
                json!({ "credentials": SENT_TOKEN, "instance": named }),
            )
            .await;

        let after = harness.stored(&tenant, source.id).await;
        if accepted {
            response.assert_status(StatusCode::OK);
            assert_eq!(
                harness.open(after.credentials_encrypted.as_deref().expect("a token")),
                SENT_TOKEN,
                "{stored:?} {named}"
            );
        } else {
            response.assert_status(StatusCode::CONFLICT);
            assert_eq!(
                response.json_value()["error"].as_str(),
                Some(INSTANCE_MOVED),
                "{stored:?} {named}"
            );
            assert_unchanged(&source, &after);
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn an_instance_the_adapter_refuses_is_refused_unwritten() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for field in [HOST_FIELD, BASE_URL_FIELD] {
        for refused in [
            "http://127.0.0.1:9",
            "https://user:password@127.0.0.1:9",
            "https://127.0.0.1:9/?private_token=glpat-throwaway-query",
        ] {
            for credentials in [None, Some(SENT_TOKEN)] {
                let source = harness
                    .source(
                        &tenant,
                        config(field, Some(STORED_INSTANCE), "main"),
                        Some(STORED_TOKEN),
                    )
                    .await;
                let mut body = json!({ "config": config(field, Some(refused), "main") });
                if let Some(credentials) = credentials {
                    body["credentials"] = json!(credentials);
                }

                let response = harness.update(&tenant, source.id, body).await;

                response.assert_status(StatusCode::BAD_REQUEST);
                let error = response.json_value()["error"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                assert_eq!(error, INVALID_INSTANCE, "{field} {credentials:?}");
                assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
            }
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_config_naming_both_host_and_base_url_is_refused_unwritten() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for (host, base_url) in [
        (STORED_INSTANCE, STORED_INSTANCE),
        (STORED_INSTANCE, OTHER_INSTANCE),
        (OTHER_INSTANCE, STORED_INSTANCE),
    ] {
        for credentials in [None, Some(SENT_TOKEN)] {
            let source = harness
                .source(
                    &tenant,
                    config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
                    Some(STORED_TOKEN),
                )
                .await;
            let mut named = config(HOST_FIELD, Some(host), "main");
            named[BASE_URL_FIELD] = json!(base_url);
            let mut body = json!({ "config": named });
            if let Some(credentials) = credentials {
                body["credentials"] = json!(credentials);
            }

            let response = harness.update(&tenant, source.id, body).await;

            response.assert_status(StatusCode::BAD_REQUEST);
            assert_eq!(
                response.json_value()["error"].as_str(),
                Some(BOTH_NAMED),
                "{host} {base_url} {credentials:?}"
            );
            assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn an_instance_named_by_anything_but_a_string_is_refused_unwritten() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for field in [HOST_FIELD, BASE_URL_FIELD] {
        for named in [json!(1), json!(true), json!({ "url": OTHER_INSTANCE })] {
            for credentials in [None, Some(SENT_TOKEN)] {
                let source = harness
                    .source(
                        &tenant,
                        config(field, Some(STORED_INSTANCE), "main"),
                        Some(STORED_TOKEN),
                    )
                    .await;
                let mut changed = config(field, None, "main");
                changed[field] = named.clone();
                let mut body = json!({ "config": changed });
                if let Some(credentials) = credentials {
                    body["credentials"] = json!(credentials);
                }

                let response = harness.update(&tenant, source.id, body).await;

                response.assert_status(StatusCode::BAD_REQUEST);
                assert_eq!(
                    response.json_value()["error"].as_str(),
                    Some(NON_STRING_NAMED),
                    "{field} {named} {credentials:?}"
                );
                assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
            }
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_stored_config_naming_no_readable_instance_changes_only_with_a_new_token() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let mut unreadable = config(HOST_FIELD, Some(STORED_INSTANCE), "main");
    unreadable[BASE_URL_FIELD] = json!(OTHER_INSTANCE);
    let source = harness
        .source(&tenant, unreadable, Some(STORED_TOKEN))
        .await;

    for changed in [
        config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
        config(BASE_URL_FIELD, Some(OTHER_INSTANCE), "main"),
        config(HOST_FIELD, None, "main"),
    ] {
        let refused = harness
            .update(&tenant, source.id, json!({ "config": changed }))
            .await;

        refused.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            refused.json_value()["error"].as_str(),
            Some(UNREADABLE_NEEDS_TOKEN),
            "{changed}"
        );
        assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
    }

    let repaired = config(HOST_FIELD, Some(STORED_INSTANCE), "main");
    let accepted = harness
        .update(
            &tenant,
            source.id,
            json!({ "config": repaired, "credentials": SENT_TOKEN }),
        )
        .await;

    accepted.assert_status(StatusCode::OK);
    assert_eq!(harness.stored(&tenant, source.id).await.config, repaired);

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn content_stored_for_a_source_never_blocks_a_config_update() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            Some(STORED_TOKEN),
        )
        .await;
    let mut indexing = harness.pool().begin().await.expect("a transaction");
    let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *indexing)
        .await
        .expect("the backend is named");
    sqlx::query(
        "INSERT INTO content_items (source_id, category, uri, title, content_hash) VALUES ($1, 'file', 'README.md', 'README.md', 'held')",
    )
    .bind(source.id)
    .execute(&mut *indexing)
    .await
    .expect("the content is stored");
    let branched = config(HOST_FIELD, Some(STORED_INSTANCE), "develop");

    let updated = tokio::time::timeout(LOCK_WAIT, async {
        tokio::select! {
            updated = harness.update(&tenant, source.id, json!({ "config": branched })) => updated,
            () = harness.blocked_by(backend) => {
                panic!("the update waits on the lock the content's foreign key holds")
            }
        }
    })
    .await
    .expect("the update finishes");
    indexing.rollback().await.expect("the content is discarded");

    updated.assert_status(StatusCode::OK);
    assert_eq!(harness.stored(&tenant, source.id).await.config, branched);

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_created_sources_link_is_at_its_instances_web_origin() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for (field, instance) in [
        (BASE_URL_FIELD, "https://127.0.0.1:9/api/v4"),
        (BASE_URL_FIELD, STORED_INSTANCE),
        (HOST_FIELD, "https://127.0.0.1:9/api/v4/"),
    ] {
        let (id, created) = harness
            .created(
                &tenant,
                json!({
                    "name": format!("Project {}", Uuid::new_v4()),
                    "source_type": GITLAB,
                    "config": config(field, Some(instance), "main"),
                    "credentials": SENT_TOKEN,
                }),
            )
            .await;

        assert_eq!(
            created["url"].as_str(),
            Some(STORED_LINK),
            "{field} {instance}"
        );
        assert_eq!(
            harness.stored(&tenant, id).await.url.as_deref(),
            Some(STORED_LINK),
            "{field} {instance}"
        );
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn moving_a_source_moves_its_link_unless_one_was_named() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let (id, _) = harness
        .created(
            &tenant,
            json!({
                "name": format!("Project {}", Uuid::new_v4()),
                "source_type": GITLAB,
                "config": config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            }),
        )
        .await;
    assert_eq!(
        harness.stored(&tenant, id).await.url.as_deref(),
        Some(STORED_LINK)
    );

    for (body, linked) in [
        (
            json!({ "config": config(BASE_URL_FIELD, Some("https://localhost:9/api/v4"), "main") }),
            OTHER_LINK,
        ),
        (
            json!({ "name": format!("Renamed {}", Uuid::new_v4()) }),
            OTHER_LINK,
        ),
        (
            json!({
                "config": config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
                "url": EXPLICIT_LINK,
            }),
            EXPLICIT_LINK,
        ),
        (
            json!({ "config": config(HOST_FIELD, Some(STORED_INSTANCE), "develop") }),
            EXPLICIT_LINK,
        ),
        (
            json!({ "config": config(HOST_FIELD, Some(OTHER_INSTANCE), "develop") }),
            EXPLICIT_LINK,
        ),
    ] {
        let updated = harness.update(&tenant, id, body.clone()).await;

        updated.assert_status(StatusCode::OK);
        assert_eq!(
            updated.json_value()["source"]["url"].as_str(),
            Some(linked),
            "{body}"
        );
        assert_eq!(
            harness.stored(&tenant, id).await.url.as_deref(),
            Some(linked),
            "{body}"
        );
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_source_stored_without_a_link_is_linked_by_its_next_config_edit() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            None,
        )
        .await;
    assert_eq!(source.url, None);

    let updated = harness
        .update(
            &tenant,
            source.id,
            json!({ "config": config(HOST_FIELD, Some(OTHER_INSTANCE), "main") }),
        )
        .await;

    updated.assert_status(StatusCode::OK);
    assert_eq!(
        harness.stored(&tenant, source.id).await.url.as_deref(),
        Some(OTHER_LINK)
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_link_released_code_derived_moves_with_the_next_config_edit() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let api_host = "https://127.0.0.1:9/api/v4/";
    let uppercase_host = "HTTPS://LOCALHOST:9";
    let numeric = |branch: &str| json!({ "project_id": NUMERIC_PROJECT, "host": STORED_INSTANCE, "branch": branch });

    for (case, config_before, link_before, config_after, link_after) in [
        (
            "a self-hosted base_url, which released code linked at gitlab.com",
            config(BASE_URL_FIELD, Some(STORED_INSTANCE), "main"),
            GITLAB_COM_LINK.to_string(),
            config(BASE_URL_FIELD, Some(STORED_INSTANCE), "develop"),
            STORED_LINK.to_string(),
        ),
        (
            "an /api/v4 host, which released code kept in the link",
            config(HOST_FIELD, Some(api_host), "main"),
            format!("https://127.0.0.1:9/api/v4/{PROJECT}"),
            config(HOST_FIELD, Some(api_host), "develop"),
            STORED_LINK.to_string(),
        ),
        (
            "an /api/v4 host moved to another instance",
            config(HOST_FIELD, Some(api_host), "main"),
            format!("https://127.0.0.1:9/api/v4/{PROJECT}"),
            config(HOST_FIELD, Some(OTHER_INSTANCE), "main"),
            OTHER_LINK.to_string(),
        ),
        (
            "an uppercase host, which released code kept as spelled",
            config(HOST_FIELD, Some(uppercase_host), "main"),
            format!("{uppercase_host}/{PROJECT}"),
            config(HOST_FIELD, Some(uppercase_host), "develop"),
            OTHER_LINK.to_string(),
        ),
        (
            "a numeric project id, which released code linked without /projects",
            numeric("main"),
            format!("{STORED_INSTANCE}/{NUMERIC_PROJECT}"),
            numeric("develop"),
            format!("{STORED_INSTANCE}/projects/{NUMERIC_PROJECT}"),
        ),
        (
            "a link its creator named",
            config(BASE_URL_FIELD, Some(STORED_INSTANCE), "main"),
            EXPLICIT_LINK.to_string(),
            config(BASE_URL_FIELD, Some(OTHER_INSTANCE), "develop"),
            EXPLICIT_LINK.to_string(),
        ),
    ] {
        let source = harness.linked(&tenant, config_before, &link_before).await;

        let updated = harness
            .update(&tenant, source.id, json!({ "config": config_after }))
            .await;

        updated.assert_status(StatusCode::OK);
        assert_eq!(
            updated.json_value()["source"]["url"].as_str(),
            Some(link_after.as_str()),
            "{case}"
        );
        assert_eq!(
            harness.stored(&tenant, source.id).await.url,
            Some(link_after),
            "{case}"
        );
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn creating_a_source_at_an_instance_the_adapter_refuses_stores_nothing() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for (instance, error) in [
        (
            json!({ HOST_FIELD: "http://127.0.0.1:9" }),
            INVALID_INSTANCE,
        ),
        (
            json!({ BASE_URL_FIELD: "https://127.0.0.1:9/?private_token=glpat-throwaway-query" }),
            INVALID_INSTANCE,
        ),
        (
            json!({ HOST_FIELD: STORED_INSTANCE, BASE_URL_FIELD: STORED_INSTANCE }),
            BOTH_NAMED,
        ),
        (json!({ HOST_FIELD: 1 }), NON_STRING_NAMED),
        (json!({ BASE_URL_FIELD: true }), NON_STRING_NAMED),
    ] {
        let mut named = config(HOST_FIELD, None, "main");
        for (field, value) in instance.as_object().expect("an instance") {
            named[field] = value.clone();
        }

        let response = harness
            .create(
                &tenant,
                json!({
                    "name": "project",
                    "source_type": GITLAB,
                    "config": named,
                    "credentials": SENT_TOKEN,
                }),
            )
            .await;

        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json_value()["error"].as_str(),
            Some(error),
            "{instance}"
        );
    }
    assert_eq!(harness.count(&tenant).await, 0);

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_new_token_is_stored_only_naming_the_instance_it_is_for() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            Some(STORED_TOKEN),
        )
        .await;

    for unnamed in [
        json!({ "credentials": SENT_TOKEN }),
        json!({ "credentials": SENT_TOKEN, "instance": null }),
        json!({ "credentials": SENT_TOKEN, "name": format!("Renamed {}", Uuid::new_v4()) }),
    ] {
        let refused = harness.update(&tenant, source.id, unnamed.clone()).await;

        refused.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(
            refused.json_value()["error"].as_str(),
            Some(UNNAMED_TOKEN_INSTANCE),
            "{unnamed}"
        );
        let after = harness.stored(&tenant, source.id).await;
        assert_unchanged(&source, &after);
        assert_eq!(after.name, source.name, "{unnamed}");
    }

    let accepted = harness
        .update(
            &tenant,
            source.id,
            json!({ "credentials": SENT_TOKEN, "instance": STORED_INSTANCE }),
        )
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, source.config);
    assert_eq!(
        harness.open(stored.credentials_encrypted.as_deref().expect("a token")),
        SENT_TOKEN
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_token_naming_an_instance_the_adapter_refuses_is_refused_unstored() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;

    for named in [
        "not a url",
        "http://127.0.0.1:9",
        "https://user:password@127.0.0.1:9",
        "https://127.0.0.1:9/?private_token=glpat-throwaway-query",
        "https://127.0.0.1:9/#fragment",
    ] {
        for changed in [
            None,
            Some(config(HOST_FIELD, Some(STORED_INSTANCE), "develop")),
        ] {
            let source = harness
                .source(
                    &tenant,
                    config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
                    Some(STORED_TOKEN),
                )
                .await;
            let mut body = json!({ "credentials": SENT_TOKEN, "instance": named });
            if let Some(changed) = &changed {
                body["config"] = changed.clone();
            }

            let response = harness.update(&tenant, source.id, body).await;

            response.assert_status(StatusCode::BAD_REQUEST);
            assert_eq!(
                response.json_value()["error"].as_str(),
                Some(UNREADABLE_TOKEN_INSTANCE),
                "{named} {changed:?}"
            );
            assert_unchanged(&source, &harness.stored(&tenant, source.id).await);
        }
    }

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_token_naming_no_instance_waits_out_a_move_and_is_refused_unstored() {
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
    let (concurrent, backend) = harness.concurrent(source.id, Some(&moved), None).await;

    let (entered, ()) = tokio::join!(
        harness.update(&tenant, source.id, json!({ "credentials": SENT_TOKEN })),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent move commits");
        },
    );

    entered.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        entered.json_value()["error"].as_str(),
        Some(UNNAMED_TOKEN_INSTANCE)
    );
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(
        stored.credentials_encrypted, None,
        "a token naming no instance is never paired with the one the source moved to"
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn clearing_a_token_names_the_instance_it_was_for() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let source = harness
        .source(
            &tenant,
            config(HOST_FIELD, Some(STORED_INSTANCE), "main"),
            Some(STORED_TOKEN),
        )
        .await;

    let refused = harness
        .update(&tenant, source.id, json!({ "credentials": "" }))
        .await;

    refused.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        refused.json_value()["error"].as_str(),
        Some(UNNAMED_TOKEN_INSTANCE)
    );
    assert_unchanged(&source, &harness.stored(&tenant, source.id).await);

    let cleared = harness
        .update(
            &tenant,
            source.id,
            json!({ "credentials": "", "instance": STORED_INSTANCE }),
        )
        .await;

    cleared.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(
        harness.open(
            stored
                .credentials_encrypted
                .as_deref()
                .expect("a credential")
        ),
        ""
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_stale_clear_keeps_the_token_entered_for_the_instance_the_source_moved_to() {
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
    let sealed = harness.seal(SENT_TOKEN);
    let (concurrent, backend) = harness
        .concurrent(source.id, Some(&moved), Some(&sealed))
        .await;

    let (cleared, ()) = tokio::join!(
        harness.update(
            &tenant,
            source.id,
            json!({ "credentials": "", "instance": STORED_INSTANCE }),
        ),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent move commits");
        },
    );

    cleared.assert_status(StatusCode::CONFLICT);
    assert_eq!(cleared.json_value()["error"].as_str(), Some(INSTANCE_MOVED));
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(
        stored.credentials_encrypted.as_deref(),
        Some(sealed.as_str()),
        "the moved instance's token survives a clear meant for the old one"
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_token_sent_with_a_config_is_bound_to_that_config_even_after_a_move() {
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
    let (concurrent, backend) = harness.concurrent(source.id, Some(&moved), None).await;
    let named = config(HOST_FIELD, Some(STORED_INSTANCE), "develop");

    let (entered, ()) = tokio::join!(
        harness.update(
            &tenant,
            source.id,
            json!({ "config": named, "credentials": SENT_TOKEN }),
        ),
        async {
            harness.wait_until_blocked_by(backend).await;
            concurrent
                .commit()
                .await
                .expect("the concurrent move commits");
        },
    );

    entered.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(
        stored.config, named,
        "the token reads from the instance its own config names"
    );
    assert_eq!(
        harness.open(stored.credentials_encrypted.as_deref().expect("a token")),
        SENT_TOKEN
    );

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn a_github_source_naming_a_host_is_not_held_to_the_gitlab_rule() {
    let harness = Harness::new().await;
    let tenant = harness.tenant().await;
    let repository = |host: &str| json!({ "owner": "acme", "repo": "widgets", HOST_FIELD: host });
    let source = harness
        .typed_source(
            &tenant,
            GITHUB,
            repository(STORED_INSTANCE),
            Some(STORED_TOKEN),
        )
        .await;
    let moved = repository(OTHER_INSTANCE);

    let accepted = harness
        .update(&tenant, source.id, json!({ "config": moved }))
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(stored.credentials_encrypted, source.credentials_encrypted);

    harness.discard(&tenant).await;
}

#[tokio::test]
async fn an_empty_token_replaces_the_stored_one_so_the_source_may_move() {
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
            json!({ "config": moved, "credentials": "" }),
        )
        .await;

    accepted.assert_status(StatusCode::OK);
    let stored = harness.stored(&tenant, source.id).await;
    assert_eq!(stored.config, moved);
    assert_eq!(
        harness.open(
            stored
                .credentials_encrypted
                .as_deref()
                .expect("a credential")
        ),
        "",
        "the stored token does not follow the source to the new instance"
    );

    harness.discard(&tenant).await;
}
