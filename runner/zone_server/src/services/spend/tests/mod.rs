use std::time::{Duration, Instant};

use abnegate_agent_cli::AgentKind;
use abnegate_secret::{MasterKey, SecretValue};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, TimeDelta, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_context::embeddings::providers::{PROVIDER_OPENAI, PROVIDER_SELF_HOSTED};
use zone_enterprise::spend::{Microdollars, Uncounted};
use zone_enterprise::{
    Edition, Error as LicenseError, Keyring, License, Payload, SigningKey, Status, TrustedKey, sign,
};

use super::client::{Client, DATE_FORMAT, PAGE_SIZE};
use super::coverage::{merged, reasons};
use super::error::Error;
use super::ingest::{self, OVERLAP};
use super::key::{Key, PREFIX};
use super::log::Log;
use super::page::Page;
use super::registry::{self, COOLDOWN, Entry};
use super::standing::{Standing, standing};
use super::subject::Subject;
use super::unlicensed::{MESSAGE, Unlicensed};
use crate::config::{Config, ModelBackend};
use crate::db::ai_settings::{EffectiveAiSettings, PROVIDER_ANTHROPIC, PROVIDER_CLAUDE_CODE};
use crate::services::endpoint::testing::settings;
use crate::services::license::Stored;
use crate::state::{AppState, test_config, unreachable_pool};

const MASTER: &str = "sk-master-0123456789abcdef";
const WORKSPACE: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001);
const USER: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0002);
const ORGANIZATION: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0003);
const LOGS_PATH: &str = "/spend/logs/v2";
const GENERATE_PATH: &str = "/key/generate";
const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];

fn master() -> MasterKey {
    MasterKey::new([0x42; 32])
}

fn subject() -> Subject {
    Subject {
        organization: ORGANIZATION,
        workspace: WORKSPACE,
        user: Some(USER),
    }
}

fn moment(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

fn row(request: usize) -> Value {
    json!({
        "request_id": format!("request-{request}"),
        "api_key": "token",
        "spend": 0.001,
        "startTime": "2026-11-21T10:00:00Z",
    })
}

fn page(rows: usize) -> Value {
    json!({ "data": (0..rows).map(row).collect::<Vec<_>>(), "total": rows })
}

async fn litellm() -> (MockServer, Client) {
    let server = MockServer::start().await;
    let client = Client::new(server.uri(), SecretValue::new(MASTER));
    (server, client)
}

async fn mount_page(server: &MockServer, number: u32, rows: usize) {
    Mock::given(method("GET"))
        .and(path(LOGS_PATH))
        .and(query_param("page", number.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(rows)))
        .expect(1)
        .mount(server)
        .await;
}

async fn read(
    client: &Client,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(u64, u64, Vec<usize>), Error> {
    let mut batches = Vec::new();
    let (rows, stored) = ingest::page_through(client, start, end, |records| {
        batches.push(records.len());
        let stored = records.len() as u64;
        async move { Ok(stored) }
    })
    .await?;
    Ok((rows, stored, batches))
}

fn query(request: &Request, name: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[tokio::test]
async fn paging_stops_at_the_first_short_page() {
    let (server, client) = litellm().await;
    mount_page(&server, 1, PAGE_SIZE).await;
    mount_page(&server, 2, PAGE_SIZE).await;
    mount_page(&server, 3, 3).await;

    let (rows, stored, batches) = read(
        &client,
        moment("2026-11-21T09:00:00Z"),
        moment("2026-11-21T10:00:00Z"),
    )
    .await
    .unwrap();

    assert_eq!(rows, 2 * PAGE_SIZE as u64 + 3);
    assert_eq!(stored, rows);
    assert_eq!(batches, [PAGE_SIZE, PAGE_SIZE, 3]);
    server.verify().await;
}

#[tokio::test]
async fn an_empty_first_page_ends_the_run() {
    let (server, client) = litellm().await;
    mount_page(&server, 1, 0).await;

    let (rows, stored, batches) = read(
        &client,
        moment("2026-11-21T09:00:00Z"),
        moment("2026-11-21T10:00:00Z"),
    )
    .await
    .unwrap();

    assert_eq!((rows, stored), (0, 0));
    assert_eq!(batches, [0]);
    server.verify().await;
}

#[tokio::test]
async fn a_failed_page_fails_the_run() {
    let (server, client) = litellm().await;
    mount_page(&server, 1, PAGE_SIZE).await;
    Mock::given(path(LOGS_PATH))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = read(
        &client,
        moment("2026-11-21T09:00:00Z"),
        moment("2026-11-21T10:00:00Z"),
    )
    .await;

    assert!(
        matches!(
            result,
            Err(Error::Status(StatusCode::INTERNAL_SERVER_ERROR))
        ),
        "{result:?}"
    );
}

#[test]
fn the_overlap_outlasts_the_longest_request_by_five_minutes() {
    assert_eq!(OVERLAP, Duration::from_secs(35 * 60));
    assert_eq!(
        OVERLAP,
        zone_core::llm::TIMEOUT + Duration::from_secs(5 * 60)
    );
}

#[test]
fn a_run_reads_from_the_overlap_before_the_cursor_or_the_first_key() {
    let cursor = moment("2026-11-21T10:00:00Z");
    let earliest = moment("2026-11-01T00:00:00Z");

    assert_eq!(
        ingest::start(Some(cursor), earliest),
        moment("2026-11-21T09:25:00Z")
    );
    assert_eq!(
        ingest::start(None, earliest),
        moment("2026-10-31T23:25:00Z")
    );
}

#[tokio::test]
async fn every_page_of_a_run_asks_for_the_same_window_oldest_first() {
    let (server, client) = litellm().await;
    mount_page(&server, 1, PAGE_SIZE).await;
    mount_page(&server, 2, 1).await;
    let start = moment("2026-11-21T09:25:00.750Z");
    let end = moment("2026-11-21T10:00:59.999Z");

    read(&client, start, end).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(
            query(request, "start_date").as_deref(),
            Some("2026-11-21 09:25:00")
        );
        assert_eq!(
            query(request, "end_date").as_deref(),
            Some("2026-11-21 10:00:59")
        );
        assert_eq!(query(request, "page_size"), Some(PAGE_SIZE.to_string()));
        assert_eq!(query(request, "sort_by").as_deref(), Some("startTime"));
        assert_eq!(query(request, "sort_order").as_deref(), Some("asc"));
    }
    assert_eq!(start.format(DATE_FORMAT).to_string(), "2026-11-21 09:25:00");
}

#[tokio::test]
async fn every_call_carries_the_master_key() {
    let (server, client) = litellm().await;
    let bearer = format!("Bearer {MASTER}");
    Mock::given(header("authorization", bearer.as_str()))
        .and(path(LOGS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(0)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(header("authorization", bearer.as_str()))
        .and(path("/key/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(header("authorization", bearer.as_str()))
        .and(path("/key/delete"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(header("authorization", bearer.as_str()))
        .and(path(GENERATE_PATH))
        .respond_with(Registers)
        .expect(1)
        .mount(&server)
        .await;
    let key = Key::derive(&master(), WORKSPACE, Some(USER));

    client
        .logs(Utc::now(), Utc::now(), 1)
        .await
        .expect("logs under the master key");
    assert!(client.exists(&key.token).await.unwrap());
    client
        .revoke(std::slice::from_ref(&key.token))
        .await
        .unwrap();
    client.register(&key, &subject()).await.unwrap();

    server.verify().await;
}

/// LiteLLM's answer to `/key/generate`: the key it was sent, and its hash as the token.
struct Registers;

impl Respond for Registers {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let key = body["key"].as_str().unwrap();
        ResponseTemplate::new(200).set_body_json(json!({ "key": key, "token": Key::hash(key) }))
    }
}

#[tokio::test]
async fn registering_sends_the_key_with_whose_spend_it_is_and_nothing_else() {
    let (server, client) = litellm().await;
    let key = Key::derive(&master(), WORKSPACE, Some(USER));
    Mock::given(method("POST"))
        .and(path(GENERATE_PATH))
        .and(body_json(json!({
            "key": key.secret.expose(),
            "metadata": {
                "zone_organization": ORGANIZATION,
                "zone_workspace": WORKSPACE,
                "zone_user": USER,
            },
        })))
        .respond_with(Registers)
        .expect(1)
        .mount(&server)
        .await;

    client.register(&key, &subject()).await.unwrap();

    server.verify().await;
}

#[tokio::test]
async fn registration_fails_unless_litellm_answers_with_the_keys_own_token() {
    let key = Key::derive(&master(), WORKSPACE, None);
    for (answer, status) in [
        (
            json!({ "key": key.secret.expose(), "token": "another" }),
            200,
        ),
        (json!({ "key": key.secret.expose() }), 200),
        (json!({ "key": key.secret.expose(), "token": null }), 200),
        (json!("not an object"), 200),
    ] {
        let (server, client) = litellm().await;
        Mock::given(path(GENERATE_PATH))
            .respond_with(ResponseTemplate::new(status).set_body_json(answer.clone()))
            .mount(&server)
            .await;

        let result = client.register(&key, &subject()).await;

        assert!(matches!(result, Err(Error::Token)), "{answer}: {result:?}");
    }

    let (server, client) = litellm().await;
    Mock::given(path(GENERATE_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "key": key.secret.expose(), "token": key.token })),
        )
        .mount(&server)
        .await;
    client.register(&key, &subject()).await.unwrap();

    let (server, client) = litellm().await;
    Mock::given(path(GENERATE_PATH))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;
    assert!(matches!(
        client.register(&key, &subject()).await,
        Err(Error::Status(StatusCode::BAD_REQUEST))
    ));
}

#[tokio::test]
async fn a_key_litellm_does_not_have_does_not_exist() {
    let (server, client) = litellm().await;
    Mock::given(path("/key/info"))
        .and(query_param("key", "present"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "info": {} })))
        .mount(&server)
        .await;
    Mock::given(path("/key/info"))
        .and(query_param("key", "missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(path("/key/info"))
        .and(query_param("key", "unreadable"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(client.exists("present").await.unwrap());
    assert!(!client.exists("missing").await.unwrap());
    assert!(
        client.exists("unreadable").await.is_err(),
        "a key LiteLLM can't answer for is not taken as missing"
    );
}

#[tokio::test]
async fn revoking_deletes_the_tokens_and_a_key_already_gone_is_revoked() {
    let (server, client) = litellm().await;
    Mock::given(method("POST"))
        .and(path("/key/delete"))
        .and(body_json(json!({ "keys": ["one", "two"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "deleted_keys": ["one"] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/key/delete"))
        .and(body_json(json!({ "keys": ["gone"] })))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    client
        .revoke(&["one".to_string(), "two".to_string()])
        .await
        .unwrap();
    client.revoke(&["gone".to_string()]).await.unwrap();
    client.revoke(&[]).await.unwrap();

    server.verify().await;
}

#[test]
fn a_failed_registration_cools_for_thirty_seconds() {
    let host = format!("http://cooling-{}", Uuid::new_v4());
    let token = "cooling-token";
    assert_eq!(COOLDOWN, Duration::from_secs(30));
    assert!(!registry::registered(&host, token));
    assert!(!registry::cooling(&host, token));

    registry::mark(&host, token, Entry::Failed(Instant::now()));
    assert!(registry::cooling(&host, token));
    assert!(!registry::registered(&host, token));
    assert!(registry::tokens(&host).is_empty());

    let cooled = Instant::now()
        .checked_sub(COOLDOWN + Duration::from_secs(1))
        .unwrap();
    registry::mark(&host, token, Entry::Failed(cooled));
    assert!(!registry::cooling(&host, token), "the key is tried again");
}

#[test]
fn the_registry_is_kept_per_litellm_host() {
    let host = format!("http://registered-{}", Uuid::new_v4());
    let other = format!("http://other-{}", Uuid::new_v4());

    registry::mark(&host, "first", Entry::Registered);
    registry::mark(&host, "second", Entry::Registered);
    registry::mark(&host, "failed", Entry::Failed(Instant::now()));

    assert!(registry::registered(&host, "first"));
    assert!(!registry::registered(&other, "first"));
    let mut tokens = registry::tokens(&host);
    tokens.sort();
    assert_eq!(tokens, ["first", "second"]);
    assert!(registry::tokens(&other).is_empty());

    registry::forget(&host, "first");
    assert!(!registry::registered(&host, "first"));
    assert_eq!(registry::tokens(&host), ["second"]);
}

#[test]
fn a_subject_always_derives_the_same_key() {
    let member = Key::derive(&master(), WORKSPACE, Some(USER));
    let automation = Key::derive(&master(), WORKSPACE, None);

    assert_eq!(
        member.secret.expose(),
        "sk-zone-bb18cb8610bd3f48e217e1f3186f4204951109f778249df624dee4467bcb9acf"
    );
    assert_eq!(
        member.token,
        "1787bd117b185488ecb9d7e8558ef776a902ab9f00ead7af8f8f69bd4507f36f"
    );
    assert_eq!(
        automation.secret.expose(),
        "sk-zone-10992e9a38a1aa92aaddf2bf05a7e8a7662dfaf40a3573dd7633ee904c1e061c"
    );
    assert_eq!(Key::derive(&master(), WORKSPACE, Some(USER)), member);
    assert_eq!(Key::hash(member.secret.expose()), member.token);
    assert!(member.secret.expose().starts_with(PREFIX));
    assert!(
        member.secret.expose().len() >= 16,
        "LiteLLM's custom key minimum"
    );
}

#[test]
fn every_subject_and_encryption_key_derives_its_own_key() {
    let member = Key::derive(&master(), WORKSPACE, Some(USER));

    assert_ne!(Key::derive(&master(), WORKSPACE, None), member);
    assert_ne!(Key::derive(&master(), ORGANIZATION, Some(USER)), member);
    assert_ne!(
        Key::derive(&master(), WORKSPACE, Some(ORGANIZATION)),
        member
    );
    assert_ne!(
        Key::derive(&MasterKey::new([0x43; 32]), WORKSPACE, Some(USER)),
        member
    );
}

#[tokio::test]
async fn the_key_is_derived_from_the_encryption_key_never_from_the_litellm_key() {
    let state = |litellm_key: &str| {
        AppState::new(
            crate::config::Config {
                litellm_key: litellm_key.to_string(),
                ..test_config()
            },
            unreachable_pool(),
            None,
        )
    };
    let first = state("sk-first-master-key-0123");
    let second = state("sk-second-master-key-4567");
    let rotated = AppState::new(
        crate::config::Config {
            encryption_key: "another-encryption-key-of-at-least-32-bytes".to_string(),
            ..test_config()
        },
        unreachable_pool(),
        None,
    );

    let key = Key::derive(first.master_key(), WORKSPACE, Some(USER));

    assert_eq!(Key::derive(second.master_key(), WORKSPACE, Some(USER)), key);
    assert_ne!(
        Key::derive(rotated.master_key(), WORKSPACE, Some(USER)),
        key
    );
    assert!(!key.secret.expose().contains("sk-first-master-key-0123"));
}

fn license() -> License {
    let signing = SigningKey::from_bytes(&SEED);
    let keyring =
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap();
    let text = sign(
        &Payload {
            id: Uuid::new_v4(),
            customer: "Acme Ltd".into(),
            seats: 3,
            issued_at: moment("2026-01-01T00:00:00Z"),
            expires_at: moment("2027-01-01T00:00:00Z"),
            key_id: KEY_ID.into(),
            subscription: None,
        },
        &signing,
    );
    keyring.verify(&text).unwrap()
}

#[test]
fn the_standing_of_each_license_state() {
    let license = license();
    let expires = license.expires_at();
    let cases = [
        (
            "community",
            Stored::Empty,
            expires,
            (false, false, Edition::Community, Status::Community),
        ),
        (
            "active",
            Stored::Verified(license.clone()),
            expires - TimeDelta::days(1),
            (true, true, Edition::Enterprise, Status::Active),
        ),
        (
            "grace",
            Stored::Verified(license.clone()),
            expires + TimeDelta::days(1),
            (true, true, Edition::Enterprise, Status::Grace),
        ),
        (
            "lapsed",
            Stored::Verified(license.clone()),
            license.grace_ends_at() + TimeDelta::days(1),
            (false, true, Edition::Enterprise, Status::Lapsed),
        ),
        (
            "unverified",
            Stored::Unverified(LicenseError::Signature),
            expires - TimeDelta::days(1),
            (false, true, Edition::Community, Status::Community),
        ),
    ];
    for (name, stored, now, (entitled, enforced, edition, status)) in cases {
        assert_eq!(
            standing(&stored, now),
            Standing {
                entitled,
                enforced,
                edition,
                status,
            },
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_license_that_cannot_be_read_neither_entitles_nor_enforces() {
    let state = AppState::new(test_config(), unreachable_pool(), None);

    assert_eq!(
        Standing::read(&state).await,
        Standing {
            entitled: false,
            enforced: false,
            edition: Edition::Community,
            status: Status::Community,
        }
    );
}

#[test]
fn only_an_entitled_standing_edits_or_passes() {
    let entitled = Standing {
        entitled: true,
        enforced: true,
        edition: Edition::Enterprise,
        status: Status::Grace,
    };
    let lapsed = Standing {
        entitled: false,
        status: Status::Lapsed,
        ..entitled
    };

    assert!(entitled.editable(true));
    assert!(!entitled.editable(false));
    assert!(!lapsed.editable(true));
    assert_eq!(entitled.require(), Ok(()));
    assert_eq!(lapsed.require(), Err(Unlicensed));
}

#[tokio::test]
async fn an_unlicensed_change_is_forbidden_with_the_license_code() {
    let response = Unlicensed.into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        json!({
            "error": "This needs a Zone Team license that is active or in its grace period.",
            "code": "license_required",
        })
    );
    assert!(!MESSAGE.contains("Enterprise"));
}

fn log(fields: Value) -> Log {
    let mut row = json!({ "request_id": "request", "startTime": "2026-11-21T10:00:00Z" });
    row.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(row).unwrap()
}

#[test]
fn spend_is_rounded_half_away_from_zero_and_never_negative() {
    for (spend, microdollars) in [
        (json!(0.0000005), 1),
        (json!(1.2345675), 1_234_568),
        (json!(0.0000004), 0),
        (json!(-1.0), 0),
        (json!(0), 0),
        (json!(null), 0),
        (json!(12.5), 12_500_000),
    ] {
        assert_eq!(
            log(json!({ "spend": spend })).microdollars(),
            Microdollars(microdollars),
            "{spend}"
        );
    }
}

#[test]
fn a_log_names_its_model_group_then_model_and_its_provider() {
    let full = log(json!({
        "model": "gpt-4o-2024-08-06",
        "model_group": "gpt-4o",
        "custom_llm_provider": "openai",
    }));
    let bare = log(json!({ "model": "llama3.2", "model_group": "" }));
    let nothing = log(json!({}));

    assert_eq!((full.model(), full.provider()), ("gpt-4o", "openai"));
    assert_eq!((bare.model(), bare.provider()), ("llama3.2", "unknown"));
    assert_eq!(
        (nothing.model(), nothing.provider()),
        ("unknown", "unknown")
    );
}

#[test]
fn a_log_becomes_the_record_of_its_request() {
    let record = log(json!({
        "api_key": "token",
        "spend": 0.25,
        "prompt_tokens": 120,
        "completion_tokens": -3,
        "model": "gpt-4o",
        "status": "failure",
        "startTime": "2026-11-21 10:04:05.123000",
    }))
    .record()
    .unwrap();

    assert_eq!(record.request_id, "request");
    assert_eq!(record.token, "token");
    assert_eq!(record.spend_microdollars, Microdollars(250_000));
    assert_eq!((record.prompt_tokens, record.completion_tokens), (120, 0));
    assert!(record.failed);
    assert_eq!(record.occurred_at, moment("2026-11-21T10:04:05.123Z"));
    assert!(
        !log(json!({ "status": "success", "api_key": "t" }))
            .record()
            .unwrap()
            .failed
    );
    assert_eq!(log(json!({})).record(), None, "a request under no key");
    assert_eq!(log(json!({ "api_key": " " })).record(), None);
}

#[test]
fn a_log_reads_each_time_litellm_writes() {
    for (written, read) in [
        ("2026-11-21T10:04:05Z", "2026-11-21T10:04:05Z"),
        ("2026-11-21T10:04:05.5+00:00", "2026-11-21T10:04:05.5Z"),
        ("2026-11-21T12:04:05+02:00", "2026-11-21T10:04:05Z"),
        ("2026-11-21T10:04:05.250000", "2026-11-21T10:04:05.25Z"),
        ("2026-11-21 10:04:05", "2026-11-21T10:04:05Z"),
    ] {
        assert_eq!(
            log(json!({ "startTime": written })).start_time,
            moment(read),
            "{written}"
        );
    }
}

#[test]
fn an_unreadable_row_is_skipped_but_still_counted() {
    let page: Page = serde_json::from_value(json!({
        "data": [
            { "request_id": "kept", "startTime": "2026-11-21T10:00:00Z" },
            { "request_id": "no time" },
            { "startTime": "2026-11-21T10:00:00Z" },
            { "request_id": "bad time", "startTime": "yesterday" },
        ],
        "total": 4,
        "total_is_capped": false,
    }))
    .unwrap();

    assert_eq!(page.rows(), 4);
    let logs = page.logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].request_id, "kept");
    let empty: Page = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty.rows(), 0);
}

const INSTANCE: &str = "http://litellm.internal:4000";
const SAVED_KEY: &str = "sk-saved-0123456789abcdef";

fn instance_config() -> Config {
    Config {
        litellm_host: INSTANCE.to_string(),
        litellm_key: MASTER.to_string(),
        ..test_config()
    }
}

fn provider_keyed(provider: &str) -> EffectiveAiSettings {
    let key = Some(SecretValue::new(SAVED_KEY));
    EffectiveAiSettings {
        openai_api_key: key.clone(),
        anthropic_api_key: key,
        ..settings(provider)
    }
}

fn litellm_at(host: &str, key: Option<&str>) -> EffectiveAiSettings {
    EffectiveAiSettings {
        litellm_host: Some(host.to_string()),
        litellm_key: key.map(SecretValue::new),
        ..settings(PROVIDER_SELF_HOSTED)
    }
}

#[test]
fn traffic_through_the_instance_litellm_under_its_own_key_is_all_counted() {
    let config = instance_config();

    assert_eq!(reasons(&config, &settings(PROVIDER_SELF_HOSTED)), []);
    assert_eq!(reasons(&config, &litellm_at(INSTANCE, None)), []);
    assert_eq!(reasons(&config, &litellm_at(INSTANCE, Some(MASTER))), []);
    assert_eq!(reasons(&config, &settings(PROVIDER_OPENAI)), []);
}

#[test]
fn a_coding_agent_is_uncounted_whether_settings_or_the_instance_pick_it() {
    let config = instance_config();
    let cli = Config {
        model_backend: ModelBackend::Cli {
            agent: AgentKind::Claude,
            executable: None,
        },
        ..instance_config()
    };

    assert_eq!(
        reasons(&config, &settings(PROVIDER_CLAUDE_CODE)),
        [Uncounted::CodingAgent]
    );
    assert_eq!(
        reasons(&cli, &settings(PROVIDER_SELF_HOSTED)),
        [Uncounted::CodingAgent]
    );
}

#[test]
fn a_provider_key_sends_straight_to_the_provider() {
    let config = instance_config();
    for provider in [PROVIDER_OPENAI, PROVIDER_ANTHROPIC] {
        assert_eq!(
            reasons(&config, &provider_keyed(provider)),
            [Uncounted::DirectProvider],
            "{provider}"
        );
    }
}

#[test]
fn a_litellm_saved_for_another_host_is_another_endpoint() {
    assert_eq!(
        reasons(
            &instance_config(),
            &litellm_at("http://192.168.1.10:4000", Some(SAVED_KEY))
        ),
        [Uncounted::ExternalEndpoint]
    );
}

#[test]
fn a_key_saved_for_the_instance_host_is_not_attributed() {
    assert_eq!(
        reasons(&instance_config(), &litellm_at(INSTANCE, Some(SAVED_KEY))),
        [Uncounted::SavedKey]
    );
}

#[test]
fn an_instance_without_a_master_key_names_no_saved_key() {
    let keyless = Config {
        litellm_key: String::new(),
        ..instance_config()
    };

    assert_eq!(reasons(&keyless, &settings(PROVIDER_SELF_HOSTED)), []);
    assert_eq!(
        reasons(&keyless, &litellm_at(INSTANCE, Some(SAVED_KEY))),
        []
    );
}

#[test]
fn settings_whose_url_is_refused_send_nothing_to_count() {
    assert_eq!(
        reasons(
            &instance_config(),
            &litellm_at("http://169.254.169.254", Some(SAVED_KEY))
        ),
        []
    );
}

#[test]
fn every_workspace_reason_is_named_once_in_order() {
    let config = instance_config();
    let workspaces = [
        litellm_at(INSTANCE, Some(SAVED_KEY)),
        settings(PROVIDER_CLAUDE_CODE),
        provider_keyed(PROVIDER_OPENAI),
        settings(PROVIDER_CLAUDE_CODE),
        settings(PROVIDER_SELF_HOSTED),
    ];

    assert_eq!(
        merged(&config, &workspaces),
        [
            Uncounted::CodingAgent,
            Uncounted::DirectProvider,
            Uncounted::SavedKey
        ]
    );
    assert_eq!(merged(&config, &[]), []);
}
