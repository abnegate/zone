use axum::Json;
use axum::body::Body;
use axum::extract::{FromRequest, Path};
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{TimeDelta, Utc};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::fleet::{
    BODY_INVALID, Error, LABEL_INVALID, Labels, NO_RUNNER_LEFT, RUNNER_CORDONED, RUNNER_ONLINE,
    RUNNER_OUTDATED, RUNNER_REVOKED, STATE_INVALID, VERSION_INVALID, Version,
};
use zone_enterprise::{Feature, Keyring, LICENSE_REQUIRED, Payload, Plan, SigningKey, TrustedKey};

use super::refusal::Refusal;
use super::requests::{UpdateFleetPolicyRequest, UpdateRunnerLabelsRequest, UpdateRunnerRequest};
use super::{policy, runners};
use crate::auth::Permission;
use crate::auth::entitlement::Unentitled;
use crate::db::audit::{actions, resources};
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::runners::{self as stored_runners, Change};
use crate::db::fleet::state::State;
use crate::db::testing::Database;
use crate::db::users::{self, UserRow};
use crate::services::fleet::{Identity, registration};
use crate::state::{AppState, test_config};

const KEY_ID: &str = "test-1";

fn parsed<T: DeserializeOwned>(body: Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(body)
}

async fn rejected<T: DeserializeOwned + Send>(text: &'static str) -> Refusal {
    let request = Request::builder()
        .method("PATCH")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(text))
        .unwrap();
    match Json::<T>::from_request(request, &()).await {
        Ok(_) => panic!("{text} parsed"),
        Err(rejection) => Refusal::Body(rejection),
    }
}

async fn answered(refusal: Refusal) -> (StatusCode, Value) {
    let response: Response = refusal.into_response();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn minimum(text: &str) -> Minimum {
    Minimum::parse(text).unwrap()
}

#[test]
fn a_policy_body_sets_or_clears_the_minimum_and_confirms_only_when_asked() {
    let set: UpdateFleetPolicyRequest =
        parsed(json!({ "minimum_version": "1.4.0", "confirm": true })).unwrap();
    assert_eq!(set.minimum().unwrap(), Some(minimum("1.4.0")));
    assert!(set.confirm);
    for clearing in [json!({}), json!({ "minimum_version": null })] {
        let cleared: UpdateFleetPolicyRequest = parsed(clearing).unwrap();
        assert_eq!(cleared.minimum().unwrap(), None);
        assert!(!cleared.confirm);
    }
    assert!(parsed::<UpdateFleetPolicyRequest>(json!({ "minimumVersion": "1.4.0" })).is_err());
}

#[test]
fn a_minimum_must_be_a_plain_release() {
    for text in ["1.4", "v1.4.0", "1.4.0-rc.1", "", "garbage"] {
        let request: UpdateFleetPolicyRequest = parsed(json!({ "minimum_version": text })).unwrap();
        assert_eq!(
            request.minimum(),
            Err(Error::VersionInvalid(text.to_string()))
        );
    }
}

#[test]
fn a_runner_body_names_a_stored_state() {
    for state in State::ALL {
        let request: UpdateRunnerRequest = parsed(json!({ "state": state.as_str() })).unwrap();
        assert!(matches!(request.requested(), Ok(requested) if requested == state));
        assert!(!request.confirm);
    }
    let draining: UpdateRunnerRequest = parsed(json!({ "state": "draining" })).unwrap();
    assert!(matches!(draining.requested(), Err(Refusal::State(state)) if state == "draining"));
    assert!(parsed::<UpdateRunnerRequest>(json!({ "confirm": true })).is_err());
    assert!(parsed::<UpdateRunnerRequest>(json!({ "state": "active", "name": "x" })).is_err());
}

#[test]
fn a_labels_body_replaces_the_requirement_and_refuses_a_bad_label() {
    let request: UpdateRunnerLabelsRequest =
        parsed(json!({ "labels": ["region=eu", "gpu", "gpu"] })).unwrap();
    assert_eq!(
        request.required().unwrap(),
        Labels::parse_list("gpu,region=eu").unwrap()
    );
    let cleared: UpdateRunnerLabelsRequest = parsed(json!({ "labels": [] })).unwrap();
    assert!(cleared.required().unwrap().is_empty());
    for label in ["GPU", "-x", "a b", ""] {
        let request: UpdateRunnerLabelsRequest = parsed(json!({ "labels": [label] })).unwrap();
        assert_eq!(
            request.required(),
            Err(Error::LabelInvalid(label.to_string()))
        );
    }
    assert!(parsed::<UpdateRunnerLabelsRequest>(json!({})).is_err());
}

#[tokio::test]
async fn a_malformed_body_is_refused_as_fleet_body_invalid() {
    for refusal in [
        rejected::<UpdateFleetPolicyRequest>(r#"{"minimum_version": 1}"#).await,
        rejected::<UpdateRunnerRequest>(r#"{"state": "active", "extra": 1}"#).await,
        rejected::<UpdateRunnerLabelsRequest>("not json").await,
    ] {
        let (status, body) = answered(refusal).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], BODY_INVALID);
    }
}

#[tokio::test]
async fn each_refusal_answers_with_its_status_and_code() {
    let (status, body) = answered(Refusal::Invalid(Error::VersionInvalid("1.4".into()))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], VERSION_INVALID);
    assert_eq!(
        body["error"],
        Error::VersionInvalid("1.4".into()).to_string()
    );

    let (status, body) = answered(Refusal::Invalid(Error::LabelInvalid("GPU".into()))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], LABEL_INVALID);

    let (status, body) = answered(Refusal::State("draining".into())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({
            "error": "\"draining\" is not a runner state. Use active, cordoned or revoked.",
            "code": STATE_INVALID,
        })
    );

    let (status, body) = answered(Refusal::Online).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], RUNNER_ONLINE);

    let (status, body) = answered(Refusal::NoRunnerLeft { remaining: 0 }).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body,
        json!({
            "error": zone_enterprise::fleet::Refusal::NoRunnerLeft { remaining: 0 }.message(),
            "code": NO_RUNNER_LEFT,
            "remaining": 0,
        })
    );

    let (status, body) = answered(Refusal::Unentitled(Unentitled(Feature::FleetPolicy))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], LICENSE_REQUIRED);

    let (status, body) = answered(Refusal::Missing("Runner not found")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "error": "Runner not found" }));

    let response = Refusal::Database(sqlx::Error::PoolTimedOut).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn access_refusals_answer_as_the_agent_policy_does() {
    use crate::routes::policy::refusal::Refusal as AccessRefusal;
    let (status, body) = answered(AccessRefusal::Missing("Organization not found").into()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "error": "Organization not found" }));
    let (status, _) = answered(AccessRefusal::Forbidden(Permission::PoliciesUpdate).into()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[test]
fn only_setting_or_raising_a_minimum_tightens_it() {
    assert!(policy::tightens(None, Some(&minimum("1.0.0"))));
    assert!(policy::tightens(
        Some(&minimum("1.4.0")),
        Some(&minimum("1.4.1"))
    ));
    assert!(!policy::tightens(
        Some(&minimum("1.4.0")),
        Some(&minimum("1.4.0"))
    ));
    assert!(!policy::tightens(
        Some(&minimum("1.4.0")),
        Some(&minimum("1.3.9"))
    ));
    assert!(!policy::tightens(Some(&minimum("1.4.0")), None));
    assert!(!policy::tightens(None, None));
}

#[test]
fn only_cordoning_or_revoking_reduces_what_a_runner_may_take() {
    let id = Uuid::new_v4();
    let reduction = |previous, next| runners::reduction(id, previous, next);
    assert_eq!(
        reduction(State::Active, State::Cordoned),
        Some(Change::Cordon(id))
    );
    assert_eq!(
        reduction(State::Active, State::Revoked),
        Some(Change::Revoke(id))
    );
    assert_eq!(
        reduction(State::Cordoned, State::Revoked),
        Some(Change::Revoke(id))
    );
    for (previous, next) in [
        (State::Cordoned, State::Active),
        (State::Revoked, State::Active),
        (State::Revoked, State::Cordoned),
        (State::Active, State::Active),
        (State::Cordoned, State::Cordoned),
        (State::Revoked, State::Revoked),
    ] {
        assert_eq!(reduction(previous, next), None, "{previous:?} → {next:?}");
    }
}

struct Server {
    database: Database,
    state: AppState,
    admin: UserRow,
    runner: Uuid,
}

impl Server {
    async fn new(plan: Option<Plan>) -> Self {
        let database = Database::create("fleet_routes").await;
        let state = AppState::new(test_config(), database.pool.clone(), None);
        state.disable_mcp();
        state.set_identity(Identity {
            name: format!("runner-{}", Uuid::new_v4().simple()),
            version: Version::parse("1.4.0").unwrap(),
            labels: Labels::parse_list("gpu").unwrap(),
        });
        let signing = SigningKey::from_bytes(&[0x11; 32]);
        state.install_keyring(
            Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
                .unwrap(),
        );
        if let Some(plan) = plan {
            let now = Utc::now();
            let license = zone_enterprise::sign(
                &Payload {
                    id: Uuid::new_v4(),
                    customer: "Acme Ltd".into(),
                    seats: 3,
                    issued_at: now - TimeDelta::days(1),
                    expires_at: now + TimeDelta::days(365),
                    key_id: KEY_ID.into(),
                    subscription: None,
                    plan: Some(plan),
                },
                &signing,
            );
            crate::db::licenses::replace(&database.pool, &license)
                .await
                .expect("the license");
        }
        let runner = registration::register(&state)
            .await
            .expect("this server's runner");
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, is_admin) \
             VALUES ($1, $1::text || '@zone.test', 'test-only', true)",
        )
        .bind(id)
        .execute(&database.pool)
        .await
        .expect("a server admin");
        let admin = users::get_user_by_id(&database.pool, id)
            .await
            .unwrap()
            .expect("the admin row");
        Self {
            database,
            state,
            admin,
            runner,
        }
    }

    async fn seed(&self, version: &str, state: State) -> Uuid {
        let registered = stored_runners::upsert(
            &self.database.pool,
            &format!("seeded-{}", Uuid::new_v4().simple()),
            Uuid::new_v4(),
            &Version::parse(version).unwrap(),
            &Labels::default(),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(registered.runner.id)
            .bind(state.as_str())
            .execute(&self.database.pool)
            .await
            .unwrap();
        registered.runner.id
    }

    async fn disconnect(&self, runner: Uuid) {
        sqlx::query(
            "UPDATE fleet_runners SET last_seen_at = now() - interval '5 minutes' WHERE id = $1",
        )
        .bind(runner)
        .execute(&self.database.pool)
        .await
        .unwrap();
    }

    async fn audited(&self, action: &str) -> Vec<(Option<Uuid>, String, Value, Value)> {
        sqlx::query_as(
            "SELECT organization_id, resource_type, \
                 COALESCE(old_values, 'null'::jsonb), COALESCE(new_values, 'null'::jsonb) \
             FROM audit_logs WHERE action = $1 ORDER BY sequence",
        )
        .bind(action)
        .fetch_all(&self.database.pool)
        .await
        .unwrap()
    }

    async fn minimum(&self, request: Value) -> Result<Option<String>, Refusal> {
        let request = parsed(request).unwrap();
        policy::write(&self.state, &self.admin, Ok(Json(request)))
            .await
            .map(|response| response.minimum_version)
    }

    async fn transition(&self, runner: Uuid, request: Value) -> Result<&'static str, Refusal> {
        let request = parsed(request).unwrap();
        runners::write(
            &self.state,
            &self.admin,
            Ok(Path(runner)),
            Ok(Json(request)),
        )
        .await
        .map(|response| response.state)
    }

    async fn remove(self) {
        drop(self.state);
        self.database.remove().await;
    }
}

#[tokio::test]
async fn setting_or_raising_a_minimum_that_leaves_no_runner_needs_confirmation() {
    let server = Server::new(Some(Plan::Enterprise)).await;

    let refused = server.minimum(json!({ "minimum_version": "2.0.0" })).await;
    assert!(
        matches!(refused, Err(Refusal::NoRunnerLeft { remaining: 0 })),
        "{refused:?}"
    );
    assert!(
        server
            .audited(actions::FLEET_POLICY_UPDATED)
            .await
            .is_empty()
    );

    assert_eq!(
        server
            .minimum(json!({ "minimum_version": "1.0.0" }))
            .await
            .unwrap(),
        Some("1.0.0".into())
    );
    let raised = server.minimum(json!({ "minimum_version": "1.5.0" })).await;
    assert!(
        matches!(raised, Err(Refusal::NoRunnerLeft { remaining: 0 })),
        "{raised:?}"
    );
    assert_eq!(
        server
            .minimum(json!({ "minimum_version": "1.5.0", "confirm": true }))
            .await
            .unwrap(),
        Some("1.5.0".into())
    );
    assert_eq!(
        server.audited(actions::FLEET_POLICY_UPDATED).await,
        [
            (
                None,
                resources::FLEET_POLICY.to_string(),
                json!({ "minimum_version": null }),
                json!({ "minimum_version": "1.0.0" }),
            ),
            (
                None,
                resources::FLEET_POLICY.to_string(),
                json!({ "minimum_version": "1.0.0" }),
                json!({ "minimum_version": "1.5.0" }),
            ),
        ]
    );
    server.remove().await;
}

#[tokio::test]
async fn lowering_or_clearing_a_minimum_needs_no_confirmation_and_an_unchanged_save_is_not_audited()
{
    let server = Server::new(Some(Plan::Enterprise)).await;
    server
        .minimum(json!({ "minimum_version": "2.0.0", "confirm": true }))
        .await
        .unwrap();

    assert_eq!(
        server
            .minimum(json!({ "minimum_version": "1.9.0" }))
            .await
            .unwrap(),
        Some("1.9.0".into())
    );
    assert_eq!(
        server
            .minimum(json!({ "minimum_version": null }))
            .await
            .unwrap(),
        None
    );
    assert_eq!(server.minimum(json!({})).await.unwrap(), None);
    assert_eq!(server.audited(actions::FLEET_POLICY_UPDATED).await.len(), 3);
    server.remove().await;
}

#[tokio::test]
async fn another_connected_current_runner_lets_a_tightening_change_through() {
    let server = Server::new(Some(Plan::Enterprise)).await;
    server.seed("2.0.0", State::Active).await;
    assert_eq!(
        server
            .minimum(json!({ "minimum_version": "2.0.0" }))
            .await
            .unwrap(),
        Some("2.0.0".into())
    );

    let other = Server::new(Some(Plan::Enterprise)).await;
    other.seed("1.4.0", State::Active).await;
    assert_eq!(
        other
            .transition(other.runner, json!({ "state": "cordoned" }))
            .await
            .unwrap(),
        "cordoned"
    );
    server.remove().await;
    other.remove().await;
}

#[tokio::test]
async fn cordoning_or_revoking_the_last_runner_needs_confirmation_and_loosening_never_does() {
    let server = Server::new(Some(Plan::Enterprise)).await;
    let runner = server.runner;
    let offline = server.seed("1.4.0", State::Active).await;
    server.disconnect(offline).await;
    server.seed("1.0.0", State::Active).await;
    server
        .minimum(json!({ "minimum_version": "1.4.0" }))
        .await
        .unwrap();

    for state in ["cordoned", "revoked"] {
        let refused = server.transition(runner, json!({ "state": state })).await;
        assert!(
            matches!(refused, Err(Refusal::NoRunnerLeft { remaining: 0 })),
            "{state}: {refused:?}"
        );
    }
    assert!(server.audited(actions::RUNNER_UPDATED).await.is_empty());

    assert_eq!(
        server
            .transition(runner, json!({ "state": "cordoned", "confirm": true }))
            .await
            .unwrap(),
        "cordoned"
    );
    assert_eq!(
        server
            .transition(runner, json!({ "state": "active" }))
            .await
            .unwrap(),
        "active"
    );
    assert_eq!(
        server
            .transition(runner, json!({ "state": "revoked", "confirm": true }))
            .await
            .unwrap(),
        "revoked"
    );
    assert_eq!(
        server
            .transition(runner, json!({ "state": "cordoned" }))
            .await
            .unwrap(),
        "cordoned"
    );
    assert_eq!(
        server
            .transition(runner, json!({ "state": "cordoned" }))
            .await
            .unwrap(),
        "cordoned"
    );

    let updated = server.audited(actions::RUNNER_UPDATED).await;
    let states: Vec<(Value, Value)> = updated
        .iter()
        .map(|(organization, resource, old, new)| {
            assert_eq!(*organization, None);
            assert_eq!(resource, resources::RUNNER);
            (old["state"].clone(), new["state"].clone())
        })
        .collect();
    assert_eq!(
        states,
        [
            (json!("active"), json!("cordoned")),
            (json!("cordoned"), json!("active")),
            (json!("active"), json!("revoked")),
            (json!("revoked"), json!("cordoned")),
        ]
    );
    server.remove().await;
}

#[tokio::test]
async fn a_change_is_refused_by_license_then_body_then_field_then_runner() {
    for plan in [None, Some(Plan::Team)] {
        let server = Server::new(plan).await;
        let refused = policy::write(
            &server.state,
            &server.admin,
            Err(
                match rejected::<UpdateFleetPolicyRequest>("not json").await {
                    Refusal::Body(rejection) => rejection,
                    other => panic!("{other:?}"),
                },
            ),
        )
        .await;
        assert!(
            matches!(
                refused,
                Err(Refusal::Unentitled(Unentitled(Feature::FleetPolicy)))
            ),
            "{refused:?}"
        );
        let refused = server
            .transition(Uuid::new_v4(), json!({ "state": "nonsense" }))
            .await;
        assert!(
            matches!(refused, Err(Refusal::Unentitled(_))),
            "{refused:?}"
        );
        server.remove().await;
    }

    let server = Server::new(Some(Plan::Enterprise)).await;
    let refused = server.minimum(json!({ "minimum_version": "1.4" })).await;
    assert!(
        matches!(refused, Err(Refusal::Invalid(Error::VersionInvalid(_)))),
        "{refused:?}"
    );
    let refused = server
        .transition(Uuid::new_v4(), json!({ "state": "nonsense" }))
        .await;
    assert!(matches!(refused, Err(Refusal::State(_))), "{refused:?}");
    let refused = server
        .transition(Uuid::new_v4(), json!({ "state": "cordoned" }))
        .await;
    assert!(matches!(refused, Err(Refusal::Missing(_))), "{refused:?}");
    server.remove().await;
}

#[tokio::test]
async fn only_an_offline_runner_is_removed_and_its_removal_is_audited() {
    let server = Server::new(Some(Plan::Enterprise)).await;
    let offline = server.seed("1.4.0", State::Active).await;

    let online = runners::removal(&server.state, &server.admin, Ok(Path(offline))).await;
    assert!(matches!(online, Err(Refusal::Online)), "{online:?}");

    server.disconnect(offline).await;
    runners::removal(&server.state, &server.admin, Ok(Path(offline)))
        .await
        .unwrap();
    let missing = runners::removal(&server.state, &server.admin, Ok(Path(offline))).await;
    assert!(matches!(missing, Err(Refusal::Missing(_))), "{missing:?}");

    let removed = server.audited(actions::RUNNER_REMOVED).await;
    assert_eq!(removed.len(), 1);
    let (organization, resource, old, new) = &removed[0];
    assert_eq!(*organization, None);
    assert_eq!(resource, resources::RUNNER);
    assert_eq!(old["state"], "active");
    assert_eq!(old["version"], "1.4.0");
    assert_eq!(*new, Value::Null);
    server.remove().await;
}

#[tokio::test]
async fn the_fleet_says_which_runners_serve_why_the_others_dont_and_which_served_it() {
    let server = Server::new(Some(Plan::Enterprise)).await;
    let cordoned = server.seed("1.4.0", State::Cordoned).await;
    let revoked = server.seed("1.4.0", State::Revoked).await;
    let outdated = server.seed("1.3.0", State::Active).await;
    let offline = server.seed("1.4.0", State::Active).await;
    server.disconnect(offline).await;
    server
        .minimum(json!({ "minimum_version": "1.4.0", "confirm": true }))
        .await
        .unwrap();

    let fleet = runners::listing(&server.state).await.unwrap();
    assert!(fleet.editable && fleet.enforced);
    assert_eq!(fleet.current, Some(server.runner));
    let judged = |id: Uuid| {
        let runner = fleet
            .runners
            .iter()
            .find(|runner| runner.id == id)
            .expect("the runner is listed");
        (
            runner.serving,
            runner.code,
            runner.connected,
            runner.outdated,
            runner.current,
        )
    };
    assert_eq!(judged(server.runner), (true, None, true, false, true));
    assert_eq!(
        judged(cordoned),
        (false, Some(RUNNER_CORDONED), true, false, false)
    );
    assert_eq!(
        judged(revoked),
        (false, Some(RUNNER_REVOKED), true, false, false)
    );
    assert_eq!(
        judged(outdated),
        (false, Some(RUNNER_OUTDATED), true, true, false)
    );
    assert_eq!(judged(offline), (false, None, false, false, false));
    server.remove().await;
}

#[tokio::test]
async fn without_an_enterprise_license_the_fleet_reads_but_is_neither_editable_nor_enforced() {
    let server = Server::new(Some(Plan::Team)).await;
    let fleet = runners::listing(&server.state).await.unwrap();
    assert!(!fleet.editable && !fleet.enforced);
    assert_eq!(fleet.runners.len(), 1);
    server.remove().await;
}
