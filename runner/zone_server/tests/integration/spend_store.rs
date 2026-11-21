//! The spend data layer: derived keys, ingesting LiteLLM's spend logs, and keeping LiteLLM's
//! keys in step.

use abnegate_secret::MasterKey;
use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, ResponseTemplate};
use zone_server::db::audit::lock::Lock;
use zone_server::db::spend::{cursor, keys};
use zone_server::services::spend::attribution::attribute;
use zone_server::services::spend::client::DATE_FORMAT;
use zone_server::services::spend::ingest::{self, Ingested, OVERLAP, SPEND_LOCK};
use zone_server::services::spend::{Key, registry, upkeep};

use crate::common::spend::{DELETE_PATH, Fixture, GENERATE_PATH, LOGS_PATH, MASTER, log};

fn overlap() -> TimeDelta {
    TimeDelta::from_std(OVERLAP).unwrap()
}

fn query(request: &wiremock::Request, name: &str) -> String {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn seconds(moment: DateTime<Utc>) -> String {
    moment.format(DATE_FORMAT).to_string()
}

/// Registers the owner's key through attribution, as a member's first turn would.
async fn attributed(fixture: &Fixture) -> Key {
    fixture.mount_register().await;
    let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;
    let key = fixture.key(&fixture.subject());
    assert_eq!(endpoint.key().expose(), key.secret.expose());
    key
}

async fn ingested(fixture: &Fixture) -> Ingested {
    ingest::run(fixture.state(), &fixture.client())
        .await
        .unwrap()
}

#[tokio::test]
async fn keys_are_recorded_once_and_revoked_keys_stay() {
    let fixture = Fixture::new("spend_keys").await;
    let pool = fixture.pool();
    let subject = fixture.subject();
    assert_eq!(keys::earliest(pool).await.unwrap(), None);

    for _ in 0..2 {
        keys::insert(
            pool,
            "member",
            subject.organization,
            subject.workspace,
            subject.user,
        )
        .await
        .unwrap();
    }
    keys::insert(
        pool,
        "automation",
        subject.organization,
        subject.workspace,
        None,
    )
    .await
    .unwrap();
    let active = keys::active(pool).await.unwrap();
    assert_eq!(active.len(), 2);
    let earliest = keys::earliest(pool).await.unwrap().unwrap();
    assert_eq!(
        earliest,
        active.iter().map(|row| row.created_at).min().unwrap()
    );

    keys::revoked(pool, "member").await.unwrap();
    let active = keys::active(pool).await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|row| row.token.as_str())
            .collect::<Vec<_>>(),
        ["automation"]
    );
    assert_eq!(active[0].user_id, None);
    assert_eq!(keys::earliest(pool).await.unwrap(), Some(earliest));

    keys::insert(
        pool,
        "member",
        subject.organization,
        subject.workspace,
        subject.user,
    )
    .await
    .unwrap();
    assert_eq!(
        keys::active(pool).await.unwrap().len(),
        2,
        "a key derived again after its revocation is active once more"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_cursor_is_read_back_where_it_was_advanced() {
    let fixture = Fixture::new("spend_cursor").await;
    let pool = fixture.pool();
    assert_eq!(cursor::read(pool).await.unwrap(), None);

    let first = Utc::now().trunc_subsecs(6);
    cursor::advance(pool, first).await.unwrap();
    assert_eq!(cursor::read(pool).await.unwrap(), Some(first));
    let second = first + TimeDelta::minutes(1);
    cursor::advance(pool, second).await.unwrap();
    assert_eq!(cursor::read(pool).await.unwrap(), Some(second));

    fixture.cleanup().await;
}

#[tokio::test]
async fn attributed_spend_lands_with_its_organization_workspace_and_user() {
    let fixture = Fixture::new("spend_attributed").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let automation = fixture.key(&fixture.automation());
    keys::insert(
        fixture.pool(),
        &automation.token,
        fixture.policy.organization,
        fixture.policy.workspace,
        None,
    )
    .await
    .unwrap();
    let start = Utc::now().trunc_subsecs(3) - TimeDelta::minutes(1);
    fixture
        .mount_logs(vec![
            log("member-request", &member.token, start, 0.012346),
            log("automation-request", &automation.token, start, 0.5),
        ])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 2, stored: 2 }
    );

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 2);
    let automated = &stored[0];
    assert_eq!(automated.request_id, "automation-request");
    assert_eq!(automated.user_id, None);
    assert_eq!(automated.spend_microdollars, 500_000);
    let spent = &stored[1];
    assert_eq!(spent.request_id, "member-request");
    assert_eq!(spent.organization_id, fixture.policy.organization);
    assert_eq!(spent.workspace_id, Some(fixture.policy.workspace));
    assert_eq!(spent.user_id, Some(fixture.policy.owner.id));
    assert_eq!(spent.token, member.token);
    assert_eq!(spent.spend_microdollars, 12_346);
    assert_eq!((spent.prompt_tokens, spent.completion_tokens), (100, 20));
    assert_eq!(
        (spent.model.as_str(), spent.provider.as_str()),
        ("gpt-4o", "openai")
    );
    assert!(!spent.failed);
    assert_eq!(spent.occurred_at, start);

    fixture.cleanup().await;
}

#[tokio::test]
async fn spend_under_keys_zone_did_not_derive_is_skipped() {
    let fixture = Fixture::new("spend_foreign").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let now = Utc::now();
    fixture
        .mount_logs(vec![
            log("foreign", &Key::hash("sk-someone-elses-key-0000"), now, 1.0),
            log("master", &Key::hash(MASTER), now, 1.0),
            log("ours", &member.token, now, 1.0),
        ])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 3, stored: 1 }
    );
    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].request_id, "ours");

    fixture.cleanup().await;
}

#[tokio::test]
async fn ingesting_again_keeps_one_record_with_the_newest_spend() {
    let fixture = Fixture::new("spend_idempotent").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let start = Utc::now() - TimeDelta::minutes(2);
    fixture
        .mount_logs(vec![log("request", &member.token, start, 0.1)])
        .await;
    ingested(&fixture).await;
    ingested(&fixture).await;
    assert_eq!(fixture.stored().await.len(), 1);

    fixture.litellm.reset().await;
    let mut settled = log("request", &member.token, start, 0.2);
    settled["status"] = "failure".into();
    settled["completion_tokens"] = 40.into();
    fixture.mount_logs(vec![settled]).await;
    ingested(&fixture).await;

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].spend_microdollars, 200_000);
    assert_eq!(stored[0].completion_tokens, 40);
    assert!(stored[0].failed);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_run_advances_the_cursor_to_the_end_it_read_through() {
    let fixture = Fixture::new("spend_cursor_end").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let earliest = keys::earliest(fixture.pool()).await.unwrap().unwrap();
    fixture
        .mount_logs(vec![log("request", &member.token, Utc::now(), 0.1)])
        .await;

    let before = Utc::now().trunc_subsecs(6);
    ingested(&fixture).await;
    let after = Utc::now();

    let through = cursor::read(fixture.pool()).await.unwrap().unwrap();
    assert!(
        before <= through && through <= after,
        "{before} {through} {after}"
    );
    let requests = fixture.received(LOGS_PATH).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(query(&requests[0], "end_date"), seconds(through));
    assert_eq!(
        query(&requests[0], "start_date"),
        seconds(earliest - overlap()),
        "the first run reads from the overlap before the first key"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_request_logged_late_inside_the_overlap_is_still_ingested() {
    let fixture = Fixture::new("spend_overlap").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    let through = Utc::now().trunc_subsecs(3);
    cursor::advance(fixture.pool(), through).await.unwrap();
    let late = through - TimeDelta::minutes(30);
    fixture
        .mount_logs(vec![log("late", &member.token, late, 0.3)])
        .await;

    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 1, stored: 1 }
    );

    assert_eq!(fixture.stored().await[0].occurred_at, late);
    let requests = fixture.received(LOGS_PATH).await;
    assert_eq!(
        query(&requests[0], "start_date"),
        seconds(through - overlap())
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_run_while_another_instance_ingests_is_skipped() {
    let fixture = Fixture::new("spend_locked").await;
    fixture.licensed().await;
    attributed(&fixture).await;
    let held = Lock::attempt(fixture.pool(), SPEND_LOCK)
        .await
        .unwrap()
        .expect("nothing else holds the lock");

    assert_eq!(ingested(&fixture).await, Ingested::Skipped);
    assert!(fixture.received(LOGS_PATH).await.is_empty());
    assert_eq!(cursor::read(fixture.pool()).await.unwrap(), None);

    held.release().await.unwrap();
    fixture.mount_logs(Vec::new()).await;
    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 0, stored: 0 }
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn with_no_key_there_is_nothing_to_ingest() {
    let fixture = Fixture::new("spend_idle").await;
    fixture.licensed().await;

    assert_eq!(ingested(&fixture).await, Ingested::Idle);
    assert!(fixture.received(LOGS_PATH).await.is_empty());

    fixture.cleanup().await;
}

#[tokio::test]
async fn community_sends_the_master_key_and_registers_nothing() {
    let fixture = Fixture::new("spend_community").await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;

    let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;

    assert_eq!(endpoint.key().expose(), MASTER);
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());
    assert_eq!(ingested(&fixture).await, Ingested::Idle);
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_attributes() {
    let fixture = Fixture::new("spend_lapsed").await;
    fixture.lapsed().await;
    fixture.mount_register().await;

    let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;

    assert!(endpoint.key().expose().starts_with("sk-zone-"));

    fixture.cleanup().await;
}

#[tokio::test]
async fn attribution_registers_each_key_once_per_process() {
    let fixture = Fixture::new("spend_register_once").await;
    fixture.licensed().await;
    Fixture::registration()
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    let key = fixture.key(&fixture.subject());

    for _ in 0..3 {
        let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;
        assert!(endpoint.key().expose().starts_with("sk-zone-"));
        assert_eq!(endpoint.key().expose(), key.secret.expose());
        assert_eq!(endpoint.credential().expose(), MASTER);
    }

    fixture.litellm.verify().await;
    let active = keys::active(fixture.pool()).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].token, key.token);
    assert_eq!(active[0].user_id, Some(fixture.policy.owner.id));
    let registered = fixture.received(GENERATE_PATH).await;
    let body: serde_json::Value = serde_json::from_slice(&registered[0].body).unwrap();
    assert_eq!(
        body["metadata"]["zone_workspace"],
        fixture.policy.workspace.to_string()
    );
    assert!(body.get("key_alias").is_none() && body.get("user_id").is_none());

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_endpoint_that_is_not_attributable_is_sent_as_it_is() {
    let fixture = Fixture::new("spend_unattributable").await;
    fixture.licensed().await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;
    let saved = fixture
        .instance()
        .with_key(abnegate_secret::SecretValue::new(
            "sk-workspace-saved-key-000",
        ));

    let endpoint = attribute(fixture.state(), saved, &fixture.subject()).await;

    assert_eq!(endpoint.key().expose(), "sk-workspace-saved-key-000");
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_registration_sends_the_master_key_and_cools_before_retrying() {
    let fixture = Fixture::new("spend_register_failed").await;
    fixture.licensed().await;
    Mock::given(method("POST"))
        .and(path(GENERATE_PATH))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    let key = fixture.key(&fixture.subject());

    for _ in 0..2 {
        let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;
        assert_eq!(endpoint.key().expose(), MASTER);
    }

    let host = fixture.client().host().to_string();
    assert!(registry::cooling(&host, &key.token));
    assert!(!registry::registered(&host, &key.token));
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn verify_registers_again_a_key_litellm_lost() {
    let fixture = Fixture::new("spend_verify").await;
    fixture.licensed().await;
    let key = attributed(&fixture).await;
    fixture.litellm.reset().await;
    fixture.mount_missing_key().await;
    Fixture::registration()
        .expect(1)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::verify(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        1
    );

    let endpoint = attribute(fixture.state(), fixture.instance(), &fixture.subject()).await;
    assert_eq!(endpoint.key().expose(), key.secret.expose());
    fixture.litellm.verify().await;
    assert_eq!(keys::active(fixture.pool()).await.unwrap().len(), 1);

    fixture.cleanup().await;
}

#[tokio::test]
async fn verify_leaves_keys_litellm_still_has() {
    let fixture = Fixture::new("spend_verify_present").await;
    fixture.licensed().await;
    attributed(&fixture).await;
    fixture.litellm.reset().await;
    Mock::given(method("GET"))
        .and(path("/key/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "info": {} })))
        .expect(1)
        .mount(&fixture.litellm)
        .await;
    Fixture::registration()
        .expect(0)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::verify(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        0
    );
    fixture.litellm.verify().await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_an_earlier_encryption_key_derived_is_revoked_and_its_spend_still_attributes() {
    let fixture = Fixture::new("spend_revoke").await;
    fixture.licensed().await;
    let current = attributed(&fixture).await;
    let subject = fixture.subject();
    let former = Key::derive(&MasterKey::new([0x07; 32]), subject.workspace, subject.user);
    keys::insert(
        fixture.pool(),
        &former.token,
        subject.organization,
        subject.workspace,
        subject.user,
    )
    .await
    .unwrap();
    Mock::given(method("POST"))
        .and(path(DELETE_PATH))
        .and(body_json(serde_json::json!({ "keys": [former.token] })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "deleted_keys": [former.token] })),
        )
        .expect(1)
        .mount(&fixture.litellm)
        .await;

    assert_eq!(
        upkeep::revoke(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        upkeep::revoke(fixture.state(), &fixture.client())
            .await
            .unwrap(),
        0,
        "a revoked key is not revoked again"
    );
    fixture.litellm.verify().await;

    let active = keys::active(fixture.pool()).await.unwrap();
    assert_eq!(
        active
            .iter()
            .map(|row| row.token.as_str())
            .collect::<Vec<_>>(),
        [current.token.as_str()]
    );
    let revoked_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM spend_keys WHERE token = $1")
            .bind(&former.token)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(revoked_at.is_some());

    fixture
        .mount_logs(vec![log("late", &former.token, Utc::now(), 0.4)])
        .await;
    assert_eq!(
        ingested(&fixture).await,
        Ingested::Done { rows: 1, stored: 1 }
    );
    let stored = fixture.stored().await;
    assert_eq!(stored[0].user_id, Some(fixture.policy.owner.id));
    assert_eq!(stored[0].workspace_id, Some(fixture.policy.workspace));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deleted_workspace_keeps_its_spend_in_the_organization() {
    let fixture = Fixture::new("spend_workspace_deleted").await;
    fixture.licensed().await;
    let member = attributed(&fixture).await;
    fixture
        .mount_logs(vec![log("request", &member.token, Utc::now(), 0.1)])
        .await;
    ingested(&fixture).await;

    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(fixture.policy.workspace)
        .execute(fixture.pool())
        .await
        .unwrap();

    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].workspace_id, None);
    assert_eq!(stored[0].organization_id, fixture.policy.organization);
    assert!(keys::active(fixture.pool()).await.unwrap().is_empty());

    fixture.cleanup().await;
}
