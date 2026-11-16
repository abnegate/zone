//! The server's license: a server-wide singleton only a server admin may read
//! or change.

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
use zone_enterprise::{Edition, License, Seats, Status, Tier, status};

use super::common::{AuditEvent, ErrorResponse, audit};
use crate::auth::ServerAdmin;
use crate::db::audit::{actions, resources};
use crate::error::ServerError;
use crate::services::license::SetError;
use crate::state::AppState;

const STORE_FAILED: &str = "Could not store the license";

#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub edition: Edition,
    pub status: Status,
    pub license: Option<LicenseDetails>,
    pub seats: SeatsResponse,
}

impl LicenseResponse {
    pub fn new(license: Option<&License>, seats: Seats, now: DateTime<Utc>) -> Self {
        Self {
            edition: Edition::from(license),
            status: status(license, now),
            license: license.map(LicenseDetails::from),
            seats: SeatsResponse::from(seats),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LicenseDetails {
    pub id: Uuid,
    pub customer: String,
    pub tier: Tier,
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
            tier: license.tier(),
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
    pub used: u32,
    pub licensed: Option<u32>,
    pub over: bool,
}

impl From<Seats> for SeatsResponse {
    fn from(seats: Seats) -> Self {
        Self {
            used: seats.used,
            licensed: seats.licensed,
            over: seats.over(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetLicenseRequest {
    pub key: String,
}

/// What the audit log keeps of a license. Never the key itself: whoever can
/// read the audit log could otherwise lift it onto another server.
fn recorded(license: &License) -> Value {
    json!({
        "id": license.id(),
        "customer": license.customer(),
        "tier": license.tier(),
        "seats": license.seats(),
        "expires_at": license.expires_at(),
        "key_id": license.key_id(),
    })
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

async fn respond(
    state: &AppState,
    license: Option<License>,
) -> Result<Json<LicenseResponse>, ServerError> {
    let now = Utc::now();
    let seats = state
        .licenses()
        .seats(state.db(), license.as_ref(), now)
        .await?;
    Ok(Json(LicenseResponse::new(license.as_ref(), seats, now)))
}

/// GET /api/license
pub async fn get(
    State(state): State<AppState>,
    _admin: ServerAdmin,
) -> Result<Json<LicenseResponse>, ServerError> {
    let license = state.licenses().load(state.db()).await?;
    respond(&state, license).await
}

/// PUT /api/license
pub async fn set(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Result<Json<SetLicenseRequest>, JsonRejection>,
) -> Result<Response, ServerError> {
    let Json(request) = match body {
        Ok(body) => body,
        Err(error) => return Ok(invalid(error.body_text())),
    };
    let change = match state
        .licenses()
        .set(state.db(), &request.key, Utc::now())
        .await
    {
        Ok(change) => change,
        Err(error) => return Ok(rejection(error)),
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
            old_values: change.previous.as_ref().map(recorded),
            new_values: Some(recorded(&change.current)),
        },
    )
    .await;
    Ok(respond(&state, Some(change.current)).await?.into_response())
}

/// DELETE /api/license
pub async fn remove(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
) -> Result<Json<LicenseResponse>, ServerError> {
    if let Some(removed) = state.licenses().remove(state.db()).await? {
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: None,
                actor_id: admin.id,
                actor_email: &admin.email,
                action: actions::LICENSE_REMOVED,
                resource_type: resources::LICENSE,
                resource_id: Some(removed.id()),
                old_values: Some(recorded(&removed)),
                new_values: None,
            },
        )
        .await;
    }
    respond(&state, None).await
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use chrono::TimeDelta;
    use zone_enterprise::{GRACE_DAYS, Keyring, MALFORMED, Payload, SigningKey, TrustedKey, sign};

    use super::*;
    use crate::services::license::LAPSED;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [0x11; 32];
    const SEATS: u32 = 3;

    fn license(tier: Tier) -> License {
        let signing = SigningKey::from_bytes(&SEED);
        let keyring =
            Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()]);
        let expires_at = "2027-11-01T00:00:00Z".parse().unwrap();
        let text = sign(
            &Payload {
                id: Uuid::new_v4(),
                customer: "Acme Ltd".into(),
                tier,
                seats: SEATS,
                issued_at: "2026-11-01T00:00:00Z".parse().unwrap(),
                expires_at,
                key_id: KEY_ID.into(),
            },
            &signing,
        );
        keyring.verify(&text).unwrap()
    }

    fn seats(used: u32, licensed: Option<u32>) -> Seats {
        Seats { used, licensed }
    }

    fn serialized(response: &LicenseResponse) -> Value {
        serde_json::to_value(response).unwrap()
    }

    async fn body(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[test]
    fn community_sends_null_license_and_licensed_seats_rather_than_omitting_them() {
        let response = LicenseResponse::new(None, seats(4, None), Utc::now());

        assert_eq!(
            serialized(&response),
            json!({
                "edition": "community",
                "status": "community",
                "license": null,
                "seats": { "used": 4, "licensed": null, "over": false },
            })
        );
    }

    #[test]
    fn a_license_is_sent_with_every_detail_and_its_grace_end() {
        let license = license(Tier::Team);
        let response =
            LicenseResponse::new(Some(&license), seats(2, Some(SEATS)), license.issued_at());

        assert_eq!(
            serialized(&response),
            json!({
                "edition": "team",
                "status": "active",
                "license": {
                    "id": license.id(),
                    "customer": "Acme Ltd",
                    "tier": "team",
                    "seats": SEATS,
                    "issued_at": "2026-11-01T00:00:00Z",
                    "expires_at": "2027-11-01T00:00:00Z",
                    "grace_ends_at": "2027-12-01T00:00:00Z",
                    "key_id": KEY_ID,
                },
                "seats": { "used": 2, "licensed": SEATS, "over": false },
            })
        );
    }

    #[test]
    fn the_edition_stays_the_tier_through_grace_and_lapse() {
        let license = license(Tier::Enterprise);
        let moments = [
            (license.expires_at(), Status::Grace),
            (
                license.expires_at() + TimeDelta::days(GRACE_DAYS + 1),
                Status::Lapsed,
            ),
        ];
        for (now, expected) in moments {
            let response = LicenseResponse::new(Some(&license), seats(1, Some(SEATS)), now);
            assert_eq!(response.status, expected);
            assert_eq!(response.edition, Edition::Enterprise, "while {expected:?}");
        }
    }

    #[test]
    fn more_seats_used_than_licensed_is_over() {
        assert!(SeatsResponse::from(seats(SEATS + 1, Some(SEATS))).over);
        assert!(!SeatsResponse::from(seats(SEATS, Some(SEATS))).over);
        assert!(!SeatsResponse::from(seats(SEATS + 1, None)).over);
    }

    #[test]
    fn the_audit_record_keeps_the_license_but_never_its_key() {
        let license = license(Tier::Team);

        let record = recorded(&license);

        assert_eq!(
            record,
            json!({
                "id": license.id(),
                "customer": "Acme Ltd",
                "tier": "team",
                "seats": SEATS,
                "expires_at": "2027-11-01T00:00:00Z",
                "key_id": KEY_ID,
            })
        );
        assert!(!record.to_string().contains(license.text()));
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
