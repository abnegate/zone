//! The chat list pages and sorts on the server: a page is a `limit`/`offset`
//! slice of one ordering, `total` counts every chat the filter matches, and
//! ties fall back to the chat id so neighbouring pages never repeat or skip.

use axum::http::StatusCode;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::{TestClient, test_email, test_password};

struct Seed {
    title: &'static str,
    created: i64,
    updated: i64,
    archived: bool,
}

impl Seed {
    const fn new(title: &'static str, created: i64, updated: i64) -> Self {
        Self {
            title,
            created,
            updated,
            archived: false,
        }
    }

    const fn archived(title: &'static str, created: i64, updated: i64) -> Self {
        Self {
            title,
            created,
            updated,
            archived: true,
        }
    }
}

async fn register(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    response.json_value()["access_token"]
        .as_str()
        .expect("registration must return an access token")
        .to_string()
}

fn slug() -> String {
    format!("chat-list-{}", Uuid::new_v4())
}

async fn workspace(client: &TestClient, token: &str) -> Uuid {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Chat List Org", "slug": slug() }),
            token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization create must return a wrapped `organization` object")
        .to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Chat List Workspace", "slug": slug() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace create must return a wrapped `workspace` object")
        .parse()
        .expect("workspace id must be a uuid")
}

fn minutes(offset: i64) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .expect("a valid instant")
        + Duration::minutes(offset)
}

/// Inserts the seeds so that each one's id ranks by its position, letting a
/// test predict the id tie-break. They go in last-first so the table's
/// physical order disagrees with the ids.
async fn seed(pool: &PgPool, workspace: Uuid, seeds: &[Seed]) {
    let mut ids: Vec<Uuid> = seeds.iter().map(|_| Uuid::new_v4()).collect();
    ids.sort_unstable();
    for (id, seed) in ids.iter().zip(seeds).rev() {
        sqlx::query(
            "INSERT INTO chats (id, workspace_id, title, model_name, created_at, updated_at, archived)
             VALUES ($1, $2, $3, 'gpt-4', $4, $5, $6)",
        )
        .bind(id)
        .bind(workspace)
        .bind(seed.title)
        .bind(minutes(seed.created))
        .bind(minutes(seed.updated))
        .bind(seed.archived)
        .execute(pool)
        .await
        .expect("seed chat");
    }
}

async fn listed(client: &TestClient, token: &str, workspace: Uuid, parameters: &str) -> Value {
    let response = client
        .get_auth(
            &format!("/api/chats?workspace_id={workspace}{parameters}"),
            token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()
}

fn titles(body: &Value) -> Vec<String> {
    body["chats"]
        .as_array()
        .expect("a chats list")
        .iter()
        .map(|chat| chat["title"].as_str().expect("a chat title").to_string())
        .collect()
}

#[tokio::test]
async fn pages_slice_one_ordering_and_total_counts_every_match() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace = workspace(&client, &token).await;
    seed(
        client.state().db(),
        workspace,
        &[
            Seed::new("one", 0, 50),
            Seed::new("two", 0, 40),
            Seed::new("three", 0, 30),
            Seed::new("four", 0, 20),
            Seed::new("five", 0, 10),
            Seed::archived("hidden", 0, 60),
        ],
    )
    .await;

    let first = listed(&client, &token, workspace, "&archived=false&limit=2").await;
    assert_eq!(titles(&first), ["one", "two"]);
    assert_eq!(first["total"], 5, "{first}");
    assert_eq!(first["limit"], 2, "{first}");
    assert_eq!(first["offset"], 0, "{first}");

    let second = listed(
        &client,
        &token,
        workspace,
        "&archived=false&limit=2&offset=2",
    )
    .await;
    assert_eq!(titles(&second), ["three", "four"]);
    assert_eq!(second["total"], 5, "{second}");
    assert_eq!(second["offset"], 2, "{second}");

    let last = listed(
        &client,
        &token,
        workspace,
        "&archived=false&limit=2&offset=4",
    )
    .await;
    assert_eq!(titles(&last), ["five"]);
    assert_eq!(last["total"], 5, "{last}");

    let beyond = listed(
        &client,
        &token,
        workspace,
        "&archived=false&limit=2&offset=6",
    )
    .await;
    assert!(titles(&beyond).is_empty(), "{beyond}");
    assert_eq!(
        beyond["total"], 5,
        "a page past the end still counts the matches"
    );

    let archived = listed(&client, &token, workspace, "&archived=true&limit=2").await;
    assert_eq!(titles(&archived), ["hidden"]);
    assert_eq!(archived["total"], 1, "{archived}");

    let unfiltered = listed(&client, &token, workspace, "&limit=1").await;
    assert_eq!(titles(&unfiltered), ["hidden"]);
    assert_eq!(unfiltered["total"], 6, "{unfiltered}");
}

#[tokio::test]
async fn every_sort_orders_by_its_column_then_by_id() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace = workspace(&client, &token).await;
    seed(
        client.state().db(),
        workspace,
        &[
            Seed::new("Delta", 10, 30),
            Seed::new("alpha", 20, 20),
            Seed::new("Charlie", 10, 20),
            Seed::new("bravo", 30, 10),
        ],
    )
    .await;

    let cases: [(&str, [&str; 4]); 7] = [
        ("", ["Delta", "alpha", "Charlie", "bravo"]),
        ("updated_desc", ["Delta", "alpha", "Charlie", "bravo"]),
        ("updated_asc", ["bravo", "alpha", "Charlie", "Delta"]),
        ("created_desc", ["bravo", "alpha", "Delta", "Charlie"]),
        ("created_asc", ["Delta", "Charlie", "alpha", "bravo"]),
        ("title_asc", ["alpha", "bravo", "Charlie", "Delta"]),
        ("title_desc", ["Delta", "Charlie", "bravo", "alpha"]),
    ];
    for (sort, expected) in cases {
        let parameters = if sort.is_empty() {
            String::new()
        } else {
            format!("&sort={sort}")
        };
        let body = listed(&client, &token, workspace, &parameters).await;
        assert_eq!(titles(&body), expected, "sort `{sort}`");
        assert_eq!(body["total"], 4, "sort `{sort}`: {body}");
    }

    let paged: Vec<String> = [0, 1, 2, 3]
        .into_iter()
        .map(|offset| format!("&sort=updated_asc&limit=1&offset={offset}"))
        .collect();
    let mut walked = Vec::new();
    for parameters in &paged {
        walked.extend(titles(
            &listed(&client, &token, workspace, parameters).await,
        ));
    }
    assert_eq!(
        walked,
        ["bravo", "alpha", "Charlie", "Delta"],
        "pages of one must walk a tie in id order"
    );
}

#[tokio::test]
async fn title_sorts_ignore_case_and_break_equal_titles_by_id() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace = workspace(&client, &token).await;
    seed(
        client.state().db(),
        workspace,
        &[
            Seed::new("Zed", 0, 0),
            Seed::new("zed", 0, 0),
            Seed::new("apple", 0, 0),
            Seed::new("Banana", 0, 0),
        ],
    )
    .await;

    let ascending = listed(&client, &token, workspace, "&sort=title_asc").await;
    assert_eq!(titles(&ascending), ["apple", "Banana", "Zed", "zed"]);

    let descending = listed(&client, &token, workspace, "&sort=title_desc").await;
    assert_eq!(titles(&descending), ["Zed", "zed", "Banana", "apple"]);
}

#[tokio::test]
async fn without_a_limit_every_chat_is_listed() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace = workspace(&client, &token).await;
    sqlx::query(
        "INSERT INTO chats (workspace_id, title, model_name)
         SELECT $1, 'Chat ' || number, 'gpt-4' FROM generate_series(1, 120) AS number",
    )
    .bind(workspace)
    .execute(client.state().db())
    .await
    .expect("seed chats");

    let everything = listed(&client, &token, workspace, "").await;
    assert_eq!(titles(&everything).len(), 120);
    assert_eq!(everything["total"], 120, "{everything}");
    assert_eq!(everything["limit"], Value::Null, "{everything}");
    assert_eq!(everything["offset"], 0, "{everything}");

    let rest = listed(&client, &token, workspace, "&offset=100").await;
    assert_eq!(titles(&rest).len(), 20);
    assert_eq!(rest["total"], 120, "{rest}");
    assert_eq!(rest["limit"], Value::Null, "{rest}");
    assert_eq!(rest["offset"], 100, "{rest}");
}

#[tokio::test]
async fn limits_outside_one_to_a_hundred_and_negative_offsets_are_refused() {
    let client = TestClient::with_db().await;
    let token = register(&client).await;
    let workspace = workspace(&client, &token).await;

    for parameters in ["&limit=0", "&limit=101", "&limit=-1", "&offset=-1"] {
        let response = client
            .get_auth(
                &format!("/api/chats?workspace_id={workspace}{parameters}"),
                &token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "`{parameters}` must be refused: {}",
            response.text()
        );
        assert!(
            response.json_value()["error"].is_string(),
            "`{parameters}` must explain the refusal: {}",
            response.text()
        );
    }

    let response = client
        .get_auth(
            &format!("/api/chats?workspace_id={workspace}&sort=sideways"),
            &token,
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "an unknown sort must be refused: {}",
        response.text()
    );

    for parameters in ["&limit=1", "&limit=100&offset=0"] {
        listed(&client, &token, workspace, parameters).await;
    }
}
