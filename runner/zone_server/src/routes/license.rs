use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::{Edition, License, Seats, Status, status};

use super::common::{AuditEvent, ErrorResponse, audit};
use crate::auth::ServerAdmin;
use crate::db::DbResult;
use crate::db::audit::{actions, resources};
use crate::db::license_renewal::{self, LicenseRenewal};
use crate::error::ServerError;
use crate::services::license::renewal::{Mode, Outcome};
use crate::services::license::{SetError, Stored, summary};
use crate::state::AppState;

const STORE_FAILED: &str = "Could not store the license";
const VERIFIED: &str = "verified";

#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub edition: Edition,
    pub status: Status,
    pub license: Option<LicenseDetails>,
    pub seats: SeatsResponse,
    pub unverified: Option<UnverifiedResponse>,
    pub renewal: Option<RenewalResponse>,
}

impl LicenseResponse {
    pub fn new(
        stored: &Stored,
        seats: SeatsResponse,
        renewal: Option<RenewalResponse>,
        now: DateTime<Utc>,
    ) -> Self {
        let license = stored.license();
        Self {
            edition: Edition::from(license),
            status: status(license, now),
            license: license.map(LicenseDetails::from),
            seats,
            unverified: stored.refusal().map(UnverifiedResponse::from),
            renewal: license.and(renewal),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LicenseDetails {
    pub id: Uuid,
    pub customer: String,
    pub seats: u32,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub grace_ends_at: DateTime<Utc>,
    pub key_id: String,
}

impl From<&License> for LicenseDetails {
    fn from(license: &License) -> Self {
        Self {
            id: license.id(),
            customer: license.customer().to_owned(),
            seats: license.seats(),
            issued_at: license.issued_at(),
            expires_at: license.expires_at(),
            grace_ends_at: license.grace_ends_at(),
            key_id: license.key_id().to_owned(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SeatsResponse {
    pub used: Option<u32>,
    pub licensed: Option<u32>,
    pub over: bool,
}

impl SeatsResponse {
    fn uncounted(license: Option<&License>) -> Self {
        Self {
            used: None,
            licensed: license.map(License::seats),
            over: false,
        }
    }
}

impl From<Seats> for SeatsResponse {
    fn from(seats: Seats) -> Self {
        Self {
            used: Some(seats.used),
            licensed: seats.licensed,
            over: seats.over(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct UnverifiedResponse {
    pub code: &'static str,
}

impl From<&zone_enterprise::Error> for UnverifiedResponse {
    fn from(error: &zone_enterprise::Error) -> Self {
        Self { code: error.code() }
    }
}

#[derive(Debug, Serialize)]
pub struct RenewalResponse {
    pub mode: Mode,
    pub attempted_at: Option<DateTime<Utc>>,
    pub outcome: Option<Outcome>,
}

impl RenewalResponse {
    pub fn new(license: &License, running: bool, attempt: Option<LicenseRenewal>) -> Self {
        let mode = Mode::of(license, running);
        let Some(attempt) = attempt else {
            return Self {
                mode,
                attempted_at: None,
                outcome: None,
            };
        };
        let outcome = Outcome::parse(&attempt.outcome);
        if outcome.is_none() {
            tracing::warn!(
                outcome = %attempt.outcome,
                "Unknown renewal outcome stored; reporting it as unknown"
            );
        }
        Self {
            mode,
            attempted_at: Some(attempt.attempted_at),
            outcome,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetLicenseRequest {
    pub key: String,
}

fn former(stored: &Stored) -> Option<Value> {
    match stored {
        Stored::Empty => None,
        Stored::Verified(license) => Some(summary(license)),
        Stored::Unverified(_) => Some(json!({ VERIFIED: false })),
    }
}

fn invalid(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response()
}

fn rejection(error: SetError) -> Response {
    match error.code() {
        Some(code) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::with_code(error.to_string(), code)),
        )
            .into_response(),
        None => {
            tracing::error!(%error, "Failed to store the license");
            ServerError::Internal(STORE_FAILED.to_string()).into_response()
        }
    }
}

/// The license has already loaded, or a change has already committed, so a
/// failed count must not fail the response and hide it; it reports usage as
/// unknown rather than inventing a figure.
fn counted(seats: DbResult<Seats>, license: Option<&License>) -> SeatsResponse {
    match seats {
        Ok(seats) => SeatsResponse::from(seats),
        Err(error) => {
            tracing::error!(%error, "Could not count seats; reporting usage as unknown");
            SeatsResponse::uncounted(license)
        }
    }
}

fn attempted(renewal: DbResult<Option<LicenseRenewal>>) -> Option<LicenseRenewal> {
    renewal.unwrap_or_else(|error| {
        tracing::warn!(%error, "Could not read the license renewal status; reporting no attempt");
        None
    })
}

fn answer(
    stored: &Stored,
    seats: DbResult<Seats>,
    renewal: Option<RenewalResponse>,
    now: DateTime<Utc>,
) -> LicenseResponse {
    LicenseResponse::new(stored, counted(seats, stored.license()), renewal, now)
}

async fn renewal(state: &AppState, license: Option<&License>) -> Option<RenewalResponse> {
    let license = license?;
    let attempt = attempted(license_renewal::get(state.db()).await);
    Some(RenewalResponse::new(
        license,
        state.renewal().is_some(),
        attempt,
    ))
}

async fn respond(state: &AppState, stored: Stored) -> Json<LicenseResponse> {
    let now = Utc::now();
    let license = stored.license();
    let (seats, renewal) = tokio::join!(
        state.licenses().seats(state.db(), license, now),
        renewal(state, license),
    );
    Json(answer(&stored, seats, renewal, now))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: ServerAdmin,
) -> Result<Json<LicenseResponse>, ServerError> {
    let stored = state.licenses().load(state.db()).await?;
    Ok(respond(&state, stored).await)
}

pub async fn set(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Result<Json<SetLicenseRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(body) => body,
        Err(error) => return invalid(error.body_text()),
    };
    let change = match state
        .licenses()
        .set(state.db(), &request.key, Utc::now())
        .await
    {
        Ok(change) => change,
        Err(error) => return rejection(error),
    };
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: None,
            actor_id: admin.id,
            actor_email: &admin.email,
            action: actions::LICENSE_SET,
            resource_type: resources::LICENSE,
            resource_id: Some(change.current.id()),
            old_values: former(&change.previous),
            new_values: Some(summary(&change.current)),
        },
    )
    .await;
    respond(&state, Stored::Verified(change.current))
        .await
        .into_response()
}

pub async fn remove(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
) -> Result<Json<LicenseResponse>, ServerError> {
    let removed = state.licenses().remove(state.db()).await?;
    if let Some(old_values) = former(&removed) {
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: None,
                actor_id: admin.id,
                actor_email: &admin.email,
                action: actions::LICENSE_REMOVED,
                resource_type: resources::LICENSE,
                resource_id: removed.license().map(License::id),
                old_values: Some(old_values),
                new_values: None,
            },
        )
        .await;
    }
    Ok(respond(&state, Stored::Empty).await)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use chrono::TimeDelta;
    use zone_enterprise::{
        GRACE_DAYS, Keyring, MALFORMED, Payload, SigningKey, TrustedKey, UNKNOWN_KEY, sign,
    };

    use super::*;
    use crate::services::license::LAPSED;
    use crate::state::{test_config, unreachable_pool};

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [0x11; 32];
    const SEATS: u32 = 3;
    const SUBSCRIPTION: &str = "4f6c1a2e-8b3d-4c5e-9f70-1a2b3c4d5e6f";

    fn license() -> License {
        issued(None)
    }

    fn subscribed() -> License {
        issued(Some(SUBSCRIPTION))
    }

    fn issued(subscription: Option<&str>) -> License {
        let signing = SigningKey::from_bytes(&SEED);
        let keyring =
            Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
                .unwrap();
        let expires_at = "2027-11-01T00:00:00Z".parse().unwrap();
        let text = sign(
            &Payload {
                id: Uuid::new_v4(),
                customer: "Acme Ltd".into(),
                seats: SEATS,
                issued_at: "2026-11-01T00:00:00Z".parse().unwrap(),
                expires_at,
                key_id: KEY_ID.into(),
                subscription: subscription.map(str::to_owned),
            },
            &signing,
        );
        keyring.verify(&text).unwrap()
    }

    fn seats(used: u32, licensed: Option<u32>) -> Seats {
        Seats { used, licensed }
    }

    fn attempt(outcome: &str) -> LicenseRenewal {
        LicenseRenewal {
            attempted_at: "2027-10-20T03:00:00Z".parse().unwrap(),
            outcome: outcome.into(),
        }
    }

    fn unattempted(license: &License) -> Option<RenewalResponse> {
        Some(RenewalResponse::new(license, true, None))
    }

    fn serialized(response: &LicenseResponse) -> Value {
        serde_json::to_value(response).unwrap()
    }

    async fn body(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[test]
    fn community_sends_null_license_and_licensed_seats_rather_than_omitting_them() {
        let response =
            LicenseResponse::new(&Stored::Empty, seats(4, None).into(), None, Utc::now());

        assert_eq!(
            serialized(&response),
            json!({
                "edition": "community",
                "status": "community",
                "license": null,
                "seats": { "used": 4, "licensed": null, "over": false },
                "unverified": null,
                "renewal": null,
            })
        );
    }

    #[test]
    fn a_license_is_sent_with_every_detail_and_its_grace_end() {
        let license = license();
        let response = LicenseResponse::new(
            &Stored::Verified(license.clone()),
            seats(2, Some(SEATS)).into(),
            unattempted(&license),
            license.issued_at(),
        );

        assert_eq!(
            serialized(&response),
            json!({
                "edition": "enterprise",
                "status": "active",
                "license": {
                    "id": license.id(),
                    "customer": "Acme Ltd",
                    "seats": SEATS,
                    "issued_at": "2026-11-01T00:00:00Z",
                    "expires_at": "2027-11-01T00:00:00Z",
                    "grace_ends_at": "2027-12-01T00:00:00Z",
                    "key_id": KEY_ID,
                },
                "seats": { "used": 2, "licensed": SEATS, "over": false },
                "unverified": null,
                "renewal": { "mode": "manual", "attempted_at": null, "outcome": null },
            })
        );
    }

    #[test]
    fn the_edition_stays_enterprise_through_grace_and_lapse() {
        let license = license();
        let moments = [
            (license.expires_at(), Status::Grace),
            (
                license.expires_at() + TimeDelta::days(GRACE_DAYS + 1),
                Status::Lapsed,
            ),
        ];
        for (now, expected) in moments {
            let response = LicenseResponse::new(
                &Stored::Verified(license.clone()),
                seats(1, Some(SEATS)).into(),
                unattempted(&license),
                now,
            );
            assert_eq!(response.status, expected);
            assert_eq!(response.edition, Edition::Enterprise, "while {expected:?}");
        }
    }

    #[test]
    fn a_stored_key_that_no_longer_verifies_is_community_with_its_refusal_code() {
        let response = LicenseResponse::new(
            &Stored::Unverified(zone_enterprise::Error::UnknownKey {
                id: "2031-4".into(),
            }),
            seats(4, None).into(),
            None,
            Utc::now(),
        );

        assert_eq!(
            serialized(&response),
            json!({
                "edition": "community",
                "status": "community",
                "license": null,
                "seats": { "used": 4, "licensed": null, "over": false },
                "unverified": { "code": UNKNOWN_KEY },
                "renewal": null,
            })
        );
    }

    #[test]
    fn more_seats_used_than_licensed_is_over() {
        assert!(SeatsResponse::from(seats(SEATS + 1, Some(SEATS))).over);
        assert!(!SeatsResponse::from(seats(SEATS, Some(SEATS))).over);
        assert!(!SeatsResponse::from(seats(SEATS + 1, None)).over);
    }

    #[test]
    fn the_audit_keeps_what_a_change_replaced() {
        let license = license();

        assert_eq!(former(&Stored::Empty), None);
        assert_eq!(
            former(&Stored::Verified(license.clone())),
            Some(summary(&license))
        );
        assert_eq!(
            former(&Stored::Unverified(zone_enterprise::Error::Signature)),
            Some(json!({ "verified": false }))
        );
    }

    #[test]
    fn seats_that_could_not_be_counted_are_reported_as_unknown_and_never_over() {
        let license = license();
        let cases = [
            (
                counted(Err(sqlx::Error::PoolClosed), Some(&license)),
                json!({ "used": null, "licensed": SEATS, "over": false }),
            ),
            (
                counted(Err(sqlx::Error::PoolClosed), None),
                json!({ "used": null, "licensed": null, "over": false }),
            ),
            (
                counted(Ok(seats(2, Some(SEATS))), Some(&license)),
                json!({ "used": 2, "licensed": SEATS, "over": false }),
            ),
            (
                counted(Ok(seats(SEATS + 1, Some(SEATS))), Some(&license)),
                json!({ "used": SEATS + 1, "licensed": SEATS, "over": true }),
            ),
        ];
        for (seats, expected) in cases {
            assert_eq!(serde_json::to_value(&seats).unwrap(), expected);
        }
    }

    #[test]
    fn a_loaded_license_is_answered_in_full_when_its_seats_cannot_be_counted() {
        let license = license();
        let stored = Stored::Verified(license.clone());

        let response = answer(
            &stored,
            Err(sqlx::Error::PoolClosed),
            unattempted(&license),
            license.issued_at(),
        );

        let body = serialized(&response);
        assert_eq!(body["edition"], "enterprise", "{body}");
        assert_eq!(body["status"], "active", "{body}");
        assert_eq!(body["license"]["id"], json!(license.id()), "{body}");
        assert_eq!(
            body["seats"],
            json!({ "used": null, "licensed": SEATS, "over": false })
        );
    }

    #[test]
    fn a_stored_key_that_no_longer_verifies_keeps_its_refusal_when_seats_cannot_be_counted() {
        let stored = Stored::Unverified(zone_enterprise::Error::Signature);

        let body = serialized(&answer(
            &stored,
            Err(sqlx::Error::PoolClosed),
            None,
            Utc::now(),
        ));

        assert_eq!(
            body["unverified"],
            json!({ "code": stored.refusal().unwrap().code() })
        );
        assert_eq!(
            body["seats"],
            json!({ "used": null, "licensed": null, "over": false })
        );
    }

    #[test]
    fn counted_seats_are_answered_as_counted() {
        let license = license();
        let stored = Stored::Verified(license.clone());

        let body = serialized(&answer(
            &stored,
            Ok(seats(SEATS + 1, Some(SEATS))),
            unattempted(&license),
            license.issued_at(),
        ));

        assert_eq!(
            body["seats"],
            json!({ "used": SEATS + 1, "licensed": SEATS, "over": true })
        );
    }

    #[test]
    fn each_renewal_mode_is_sent_by_its_wire_name() {
        let cases = [
            (subscribed(), true, "automatic"),
            (subscribed(), false, "disabled"),
            (license(), true, "manual"),
            (license(), false, "manual"),
        ];
        for (license, enabled, mode) in cases {
            assert_eq!(
                serde_json::to_value(RenewalResponse::new(&license, enabled, None)).unwrap(),
                json!({ "mode": mode, "attempted_at": null, "outcome": null }),
                "enabled: {enabled}"
            );
        }
    }

    #[test]
    fn the_last_attempt_is_sent_with_its_time_and_outcome() {
        for outcome in Outcome::ALL {
            let renewal =
                RenewalResponse::new(&subscribed(), true, Some(attempt(outcome.as_str())));

            assert_eq!(
                serde_json::to_value(renewal).unwrap(),
                json!({
                    "mode": "automatic",
                    "attempted_at": "2027-10-20T03:00:00Z",
                    "outcome": outcome.as_str(),
                })
            );
        }
    }

    #[test]
    fn an_unknown_stored_outcome_is_sent_as_null_and_keeps_its_time() {
        let renewal = RenewalResponse::new(&subscribed(), true, Some(attempt("expired")));

        assert_eq!(
            serde_json::to_value(renewal).unwrap(),
            json!({
                "mode": "automatic",
                "attempted_at": "2027-10-20T03:00:00Z",
                "outcome": null,
            })
        );
    }

    #[test]
    fn a_renewal_status_that_cannot_be_read_is_reported_as_never_attempted() {
        assert_eq!(attempted(Err(sqlx::Error::PoolClosed)), None);
        assert_eq!(attempted(Ok(None)), None);
        assert_eq!(
            attempted(Ok(Some(attempt("current")))),
            Some(attempt("current"))
        );
    }

    #[tokio::test]
    async fn the_renewal_mode_follows_whether_the_sweep_can_run() {
        let license = subscribed();
        let running = AppState::new(test_config(), unreachable_pool(), None);
        let stopped = AppState::new(test_config(), unreachable_pool(), None);
        stopped.without_renewal();

        for (state, mode) in [(running, Mode::Automatic), (stopped, Mode::Disabled)] {
            state.disable_mcp();
            let renewal = renewal(&state, Some(&license))
                .await
                .expect("a verified license is answered with its renewal");

            assert_eq!(renewal.mode, mode);
            assert_eq!(renewal.attempted_at, None);
            assert_eq!(renewal.outcome, None);
        }
    }

    #[test]
    fn renewal_is_null_unless_a_license_is_verified() {
        let license = subscribed();
        let unverified = Stored::Unverified(zone_enterprise::Error::Signature);
        for stored in [Stored::Empty, unverified] {
            let response = LicenseResponse::new(
                &stored,
                seats(1, None).into(),
                unattempted(&license),
                Utc::now(),
            );

            let body = serialized(&response);
            assert_eq!(body["renewal"], Value::Null, "{body}");
            assert!(body.get("renewal").is_some(), "renewal is omitted: {body}");
        }
    }

    #[test]
    fn a_verified_license_is_answered_with_its_renewal() {
        let license = subscribed();
        let stored = Stored::Verified(license.clone());

        let body = serialized(&answer(
            &stored,
            Ok(seats(1, Some(SEATS))),
            Some(RenewalResponse::new(
                &license,
                true,
                Some(attempt("unreachable")),
            )),
            license.issued_at(),
        ));

        assert_eq!(
            body["renewal"],
            json!({
                "mode": "automatic",
                "attempted_at": "2027-10-20T03:00:00Z",
                "outcome": "unreachable",
            })
        );
    }

    #[tokio::test]
    async fn an_unreadable_body_is_a_bad_request_without_a_code() {
        let response = invalid("Missing field `key`".into());

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body(response).await,
            json!({ "error": "Missing field `key`" })
        );
    }

    #[tokio::test]
    async fn a_refused_key_is_a_bad_request_with_its_code() {
        let cases = [
            (
                SetError::Refused(zone_enterprise::Error::Malformed),
                MALFORMED,
            ),
            (SetError::Lapsed, LAPSED),
        ];
        for (error, code) in cases {
            let message = error.to_string();
            let response = rejection(error);

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                body(response).await,
                json!({ "error": message, "code": code })
            );
        }
    }

    #[tokio::test]
    async fn a_database_failure_is_a_server_error_that_names_no_refusal() {
        let response = rejection(SetError::Database(sqlx::Error::PoolClosed));

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body(response).await;
        assert_eq!(body["error"], STORE_FAILED);
        assert!(body.get("code").is_none(), "{body}");
    }
}
