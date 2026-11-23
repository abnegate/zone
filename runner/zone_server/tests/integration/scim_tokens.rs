use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::services::scim::failure::UNAUTHENTICATED;
use zone_server::services::scim::secret;

use crate::common::TestResponse;
use crate::common::scim::{self, CONTENT_TYPE, Fixture, assert_refused, assert_scim};

const PREFIX: &str = "scim_tokens";
const NO_STORE: &str = "no-store";

struct Issued {
    id: Uuid,
    secret: String,
}

fn challenged(response: &TestResponse) {
    assert_refused(response, StatusCode::UNAUTHORIZED, None);
    assert_eq!(response.header("www-authenticate"), Some("Bearer"));
    assert_eq!(response.json_value()["detail"], json!(UNAUTHENTICATED));
}

fn issued(response: &TestResponse) -> Issued {
    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.header("cache-control"), Some(NO_STORE));
    let body = response.json_value();
    let secret = body["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with(secret::PREFIX), "{secret}");
    assert_eq!(body["token"]["hint"], json!(secret::hint(&secret)));
    Issued {
        id: body["token"]["id"].as_str().unwrap().parse().unwrap(),
        secret,
    }
}

async fn issue(fixture: &Fixture, name: &str) -> Issued {
    issued(
        &fixture
            .inner
            .client
            .post_json_auth(
                &format!("/api/organizations/{}/scim/tokens", fixture.organization),
                &json!({ "name": name }),
                &fixture.owner.token,
            )
            .await,
    )
}

async fn rotate(fixture: &Fixture, token: Uuid) -> Issued {
    issued(
        &fixture
            .inner
            .client
            .post_json_auth(
                &format!(
                    "/api/organizations/{}/scim/tokens/{token}/rotations",
                    fixture.organization
                ),
                &json!({}),
                &fixture.owner.token,
            )
            .await,
    )
}

async fn revoke(fixture: &Fixture, token: Uuid) {
    fixture
        .inner
        .client
        .delete_auth(
            &format!(
                "/api/organizations/{}/scim/tokens/{token}",
                fixture.organization
            ),
            &fixture.owner.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

async fn authorized(fixture: &Fixture, uri: &str, authorization: Option<&str>) -> TestResponse {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("Accept", CONTENT_TYPE);
    let request = match authorization {
        Some(authorization) => request.header("Authorization", authorization),
        None => request,
    };
    fixture
        .inner
        .client
        .send_request(request.body(Body::empty()).unwrap())
        .await
}

async fn errors(fixture: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM scim_errors")
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

async fn everything(fixture: &Fixture) -> String {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT quote_ident(table_name) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(fixture.pool())
    .await
    .unwrap();
    let mut dumped = String::new();
    for table in tables {
        let rows: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT string_agg(row_to_json(t)::text, E'\\n') FROM {table} t"
        )))
        .fetch_one(fixture.pool())
        .await
        .unwrap();
        dumped.push_str(&rows.unwrap_or_default());
        dumped.push('\n');
    }
    dumped
}

#[tokio::test]
async fn only_a_live_token_of_the_organization_in_the_url_gets_in() {
    let fixture = Fixture::new(PREFIX).await;
    let token = issue(&fixture, "Okta").await;
    let neighbour = fixture.stranger().await;
    let other = fixture.organization_of(&neighbour).await;
    let foreign = fixture.token_for(other).await;
    let users = format!("{}/Users", fixture.base());
    let elsewhere = format!("{}/Users", scim::base(other));

    assert_scim(
        &authorized(&fixture, &users, Some(&format!("Bearer {}", token.secret))).await,
        StatusCode::OK,
    );

    let refusals: [Option<String>; 9] = [
        None,
        Some(String::new()),
        Some("Bearer".to_owned()),
        Some(format!("Basic {}", token.secret)),
        Some(token.secret.clone()),
        Some(format!("Bearer {}", fixture.owner.token)),
        Some(format!("Bearer {}", secret::generate())),
        Some(format!("Bearer {}x", token.secret)),
        Some(format!("Bearer {}", foreign.secret)),
    ];
    for authorization in &refusals {
        challenged(&authorized(&fixture, &users, authorization.as_deref()).await);
    }
    challenged(
        &authorized(
            &fixture,
            &elsewhere,
            Some(&format!("Bearer {}", token.secret)),
        )
        .await,
    );
    assert_scim(
        &authorized(
            &fixture,
            &elsewhere,
            Some(&format!("Bearer {}", foreign.secret)),
        )
        .await,
        StatusCode::OK,
    );
    let unknown = format!("{}/{}/Users", scim::BASE_PATH, Uuid::new_v4());
    challenged(
        &authorized(
            &fixture,
            &unknown,
            Some(&format!("Bearer {}", token.secret)),
        )
        .await,
    );
    let malformed = format!("{}/not-an-organization/Users", scim::BASE_PATH);
    assert_refused(
        &authorized(
            &fixture,
            &malformed,
            Some(&format!("Bearer {}", token.secret)),
        )
        .await,
        StatusCode::NOT_FOUND,
        None,
    );

    let rotated = rotate(&fixture, token.id).await;
    assert_eq!(rotated.id, token.id);
    challenged(&fixture.scim_get("/Users", &token.secret).await);
    assert_scim(
        &fixture.scim_get("/Users", &rotated.secret).await,
        StatusCode::OK,
    );

    let backup = issue(&fixture, "Entra").await;
    revoke(&fixture, rotated.id).await;
    challenged(&fixture.scim_get("/Users", &rotated.secret).await);
    assert_scim(
        &fixture.scim_get("/Users", &backup.secret).await,
        StatusCode::OK,
    );

    sqlx::query("UPDATE organizations SET is_active = FALSE WHERE id = $1")
        .bind(fixture.organization)
        .execute(fixture.pool())
        .await
        .unwrap();
    challenged(&fixture.scim_get("/Users", &backup.secret).await);
    sqlx::query("UPDATE organizations SET is_active = TRUE WHERE id = $1")
        .bind(fixture.organization)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert_scim(
        &fixture.scim_get("/Users", &backup.secret).await,
        StatusCode::OK,
    );

    assert_eq!(errors(&fixture).await, 0, "a refused caller was recorded");

    let stored = everything(&fixture).await;
    for secret in [
        &token.secret,
        &rotated.secret,
        &backup.secret,
        &foreign.secret,
    ] {
        assert!(
            !stored.contains(secret.as_str()),
            "a secret is stored in plain text"
        );
        let body = secret.strip_prefix(secret::PREFIX).unwrap();
        assert!(!stored.contains(body), "a secret's body is stored");
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn discovery_needs_a_token_too() {
    let fixture = Fixture::new(PREFIX).await;
    let token = issue(&fixture, "Okta").await;

    for path in [
        "/ServiceProviderConfig",
        "/ResourceTypes",
        "/ResourceTypes/User",
        "/Schemas",
        "/Users",
        "/Groups",
    ] {
        let uri = format!("{}{path}", fixture.base());
        challenged(&authorized(&fixture, &uri, None).await);
        let answered = fixture.scim_get(path, &token.secret).await;
        assert_scim(&answered, StatusCode::OK);
        let body: Value = answered.json_value();
        assert!(body["schemas"].is_array(), "{path}: {body}");
    }

    fixture.cleanup().await;
}
