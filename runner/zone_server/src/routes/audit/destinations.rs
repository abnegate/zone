//! Audit destinations of an organization, managed by its owners and admins, and of the server,
//! managed by server admins. Reads always succeed and carry the license standing. Creating,
//! changing and testing a destination need Enterprise streaming; deleting one, or only disabling
//! it, never does, so a lapsed license can always stop data leaving.

use std::fmt;

use abnegate_secret::REDACTED;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::audit::destination::{SecretName, Settings};
use zone_enterprise::audit::{Error, FailureCode, Health, Kind, Pause, Standing};

use super::access::{self, ORGANIZATION_MISSING};
use super::refusal::Refusal;
use crate::auth::entitlement::Unentitled;
use crate::auth::{AuthUser, ServerAdmin};
use crate::db::audit::scope::Scope;
use crate::services::audit::destinations::{self, Actor, Draft, Overview, Revision, View};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct DestinationsResponse {
    pub standing: Standing,
    pub stalled_since: Option<DateTime<Utc>>,
    pub destinations: Vec<DestinationResponse>,
}

impl DestinationsResponse {
    fn new(standing: Standing, overview: Overview) -> Self {
        Self {
            standing,
            stalled_since: overview.stalled_since,
            destinations: overview
                .destinations
                .into_iter()
                .map(|view| DestinationResponse::new(view, standing.pause))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DestinationResponse {
    pub id: Uuid,
    pub name: String,
    pub kind: Kind,
    pub enabled: bool,
    pub settings: Settings,
    pub secrets: Vec<SecretName>,
    pub delivery: DeliveryResponse,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DestinationResponse {
    fn new(view: View, pause: Option<Pause>) -> Self {
        let delivery = DeliveryResponse::new(&view, pause);
        Self {
            id: view.id,
            name: view.name,
            kind: view.kind,
            enabled: view.enabled,
            settings: view.settings,
            secrets: view.secrets,
            delivery,
            created_at: view.created_at,
            updated_at: view.updated_at,
        }
    }
}

/// `next_attempt_at` is set only while delivery is going ahead: never for a disabled or paused
/// destination.
#[derive(Debug, Serialize)]
pub struct DeliveryResponse {
    pub health: Health,
    pub backlog: i64,
    pub oldest_pending_at: Option<DateTime<Utc>>,
    pub attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<FailureCode>,
    pub last_error: Option<String>,
    pub delivered_count: i64,
}

impl DeliveryResponse {
    fn new(view: &View, pause: Option<Pause>) -> Self {
        let delivery = &view.delivery;
        let health = Health::of(
            view.enabled,
            pause,
            u32::try_from(delivery.attempts).unwrap_or_default(),
        );
        let proceeding = matches!(health, Health::Healthy | Health::Failing);
        Self {
            health,
            backlog: view.backlog.count,
            oldest_pending_at: view.backlog.oldest,
            attempts: delivery.attempts,
            next_attempt_at: proceeding.then_some(delivery.next_attempt_at),
            last_success_at: delivery.last_success_at,
            last_failure_at: delivery.last_failure_at,
            last_error_code: delivery
                .last_error_code
                .as_deref()
                .and_then(FailureCode::parse),
            last_error: delivery.last_error.clone(),
            delivered_count: delivery.delivered_count,
        }
    }
}

/// A missing name or settings field reads as empty, so it is refused with that field's own code
/// rather than a generic body error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDestinationRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default = "on")]
    pub enabled: bool,
    #[serde(default)]
    pub settings: Value,
    #[serde(default)]
    pub secrets: Option<Value>,
}

impl From<CreateDestinationRequest> for Draft {
    fn from(request: CreateDestinationRequest) -> Self {
        Self {
            name: request.name,
            enabled: request.enabled,
            settings: request.settings,
            secrets: request.secrets.unwrap_or_default(),
        }
    }
}

impl fmt::Debug for CreateDestinationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateDestinationRequest")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("settings", &self.settings)
            .field("secrets", &self.secrets.as_ref().map(|_| REDACTED))
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDestinationRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub settings: Option<Value>,
    pub secrets: Option<Value>,
}

impl From<UpdateDestinationRequest> for Revision {
    fn from(request: UpdateDestinationRequest) -> Self {
        Self {
            name: request.name,
            enabled: request.enabled,
            settings: request.settings,
            secrets: request.secrets,
        }
    }
}

impl fmt::Debug for UpdateDestinationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateDestinationRequest")
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("settings", &self.settings)
            .field("secrets", &self.secrets.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// The outcome of sending one synthetic event to a destination. Never audited.
#[derive(Debug, Serialize)]
pub struct TestResponse {
    pub delivered: bool,
    pub error_code: Option<FailureCode>,
    pub error: Option<String>,
}

fn on() -> bool {
    true
}

type Body<T> = Result<Json<T>, JsonRejection>;

/// GET /api/organizations/{org_id}/audit-destinations
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let result = async {
        let (scope, _, transaction) = organization(&state, &auth, path).await?;
        listed(&state, scope, transaction).await
    };
    respond(StatusCode::OK, result.await)
}

/// POST /api/organizations/{org_id}/audit-destinations
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Body<CreateDestinationRequest>,
) -> Response {
    let result = async {
        let (scope, actor, transaction) = organization(&state, &auth, path).await?;
        created(&state, scope, &actor, transaction, body).await
    };
    respond(StatusCode::CREATED, result.await)
}

/// GET /api/organizations/{org_id}/audit-destinations/{destination_id}
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Response {
    let result = async {
        let (scope, _, transaction, id) = organization_destination(&state, &auth, path).await?;
        shown(&state, scope, transaction, id).await
    };
    respond(StatusCode::OK, result.await)
}

/// PATCH /api/organizations/{org_id}/audit-destinations/{destination_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Body<UpdateDestinationRequest>,
) -> Response {
    let result = async {
        let (scope, actor, transaction, id) = organization_destination(&state, &auth, path).await?;
        updated(&state, scope, &actor, transaction, id, body).await
    };
    respond(StatusCode::OK, result.await)
}

/// DELETE /api/organizations/{org_id}/audit-destinations/{destination_id}
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Response {
    let result = async {
        let (scope, actor, transaction, id) = organization_destination(&state, &auth, path).await?;
        deleted(&state, scope, &actor, transaction, id).await
    };
    removed(result.await)
}

/// GET /api/audit-destinations
pub async fn server_list(State(state): State<AppState>, ServerAdmin(_): ServerAdmin) -> Response {
    let result = async {
        let transaction = state.db().begin().await?;
        listed(&state, Scope::Server, transaction).await
    };
    respond(StatusCode::OK, result.await)
}

/// POST /api/audit-destinations
pub async fn server_create(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Body<CreateDestinationRequest>,
) -> Response {
    let result = async {
        let transaction = state.db().begin().await?;
        let actor = Actor {
            id: admin.id,
            email: &admin.email,
        };
        created(&state, Scope::Server, &actor, transaction, body).await
    };
    respond(StatusCode::CREATED, result.await)
}

/// GET /api/audit-destinations/{destination_id}
pub async fn server_get(
    State(state): State<AppState>,
    ServerAdmin(_): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let result = async {
        let id = destination(path)?;
        let transaction = state.db().begin().await?;
        shown(&state, Scope::Server, transaction, id).await
    };
    respond(StatusCode::OK, result.await)
}

/// PATCH /api/audit-destinations/{destination_id}
pub async fn server_update(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
    body: Body<UpdateDestinationRequest>,
) -> Response {
    let result = async {
        let id = destination(path)?;
        let transaction = state.db().begin().await?;
        let actor = Actor {
            id: admin.id,
            email: &admin.email,
        };
        updated(&state, Scope::Server, &actor, transaction, id, body).await
    };
    respond(StatusCode::OK, result.await)
}

/// DELETE /api/audit-destinations/{destination_id}
pub async fn server_delete(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let result = async {
        let id = destination(path)?;
        let transaction = state.db().begin().await?;
        let actor = Actor {
            id: admin.id,
            email: &admin.email,
        };
        deleted(&state, Scope::Server, &actor, transaction, id).await
    };
    removed(result.await)
}

/// The organization's scope, the caller, and a transaction holding the caller's role.
async fn organization<'a>(
    state: &AppState,
    auth: &'a AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<(Scope, Actor<'a>, Transaction<'static, Postgres>), Refusal> {
    let user = auth.0.user_id().map_err(|_| Refusal::Unauthenticated)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let mut transaction = state.db().begin().await?;
    access::organization(&mut transaction, organization, user).await?;
    let actor = Actor {
        id: user,
        email: &auth.0.email,
    };
    Ok((Scope::Organization(organization), actor, transaction))
}

async fn organization_destination<'a>(
    state: &AppState,
    auth: &'a AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<(Scope, Actor<'a>, Transaction<'static, Postgres>, Uuid), Refusal> {
    let Ok(Path((organization, id))) = path else {
        return Err(Error::DestinationNotFound.into());
    };
    let (scope, actor, transaction) =
        self::organization(state, auth, Ok(Path(organization))).await?;
    Ok((scope, actor, transaction, id))
}

fn destination(path: Result<Path<Uuid>, PathRejection>) -> Result<Uuid, Refusal> {
    path.map(|Path(id)| id)
        .map_err(|_| Error::DestinationNotFound.into())
}

/// Delivery health is read on the pool once access is settled: the role lock gives the access
/// transaction an xid, which would hold the snapshot horizon back.
async fn listed(
    state: &AppState,
    scope: Scope,
    transaction: Transaction<'static, Postgres>,
) -> Result<DestinationsResponse, Refusal> {
    transaction.commit().await?;
    let standing = destinations::standing(state).await?;
    let overview = destinations::overview(state, scope).await?;
    Ok(DestinationsResponse::new(standing, overview))
}

async fn shown(
    state: &AppState,
    scope: Scope,
    transaction: Transaction<'static, Postgres>,
    id: Uuid,
) -> Result<DestinationResponse, Refusal> {
    transaction.commit().await?;
    let standing = destinations::standing(state).await?;
    let view = destinations::view(state, scope, id).await?;
    Ok(DestinationResponse::new(view, standing.pause))
}

async fn created(
    state: &AppState,
    scope: Scope,
    actor: &Actor<'_>,
    transaction: Transaction<'static, Postgres>,
    body: Body<CreateDestinationRequest>,
) -> Result<DestinationResponse, Refusal> {
    let standing = destinations::standing(state).await?;
    streams(&standing)?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let view = destinations::create(state, transaction, scope, actor, request.into()).await?;
    Ok(DestinationResponse::new(view, standing.pause))
}

async fn updated(
    state: &AppState,
    scope: Scope,
    actor: &Actor<'_>,
    transaction: Transaction<'static, Postgres>,
    id: Uuid,
    body: Body<UpdateDestinationRequest>,
) -> Result<DestinationResponse, Refusal> {
    let standing = destinations::standing(state).await?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let revision = Revision::from(request);
    if !revision.only_disables() {
        streams(&standing)?;
    }
    let view = destinations::update(state, transaction, scope, id, actor, revision).await?;
    Ok(DestinationResponse::new(view, standing.pause))
}

async fn deleted(
    state: &AppState,
    scope: Scope,
    actor: &Actor<'_>,
    transaction: Transaction<'static, Postgres>,
    id: Uuid,
) -> Result<(), Refusal> {
    destinations::delete(state, transaction, scope, id, actor).await?;
    Ok(())
}

fn streams(standing: &Standing) -> Result<(), Refusal> {
    if standing.streaming {
        return Ok(());
    }
    Err(Unentitled(Feature::AuditStreaming).into())
}

fn respond<T: Serialize>(status: StatusCode, result: Result<T, Refusal>) -> Response {
    match result {
        Ok(body) => (status, Json(body)).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

fn removed(result: Result<(), Refusal>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;
    use zone_enterprise::audit::destination::WebhookSettings;
    use zone_enterprise::{Edition, Status};

    use super::*;
    use crate::db::audit::deliveries::Delivery;
    use crate::db::audit::events::Backlog;
    use crate::db::audit::position::Position;

    fn view(enabled: bool, attempts: i32) -> View {
        let at: DateTime<Utc> = "2026-11-01T00:00:00Z".parse().unwrap();
        View {
            id: Uuid::nil(),
            name: "Splunk".into(),
            kind: Kind::Webhook,
            enabled,
            settings: Settings::Webhook(WebhookSettings {
                url: "http://10.0.0.5/hook".into(),
            }),
            secrets: vec![SecretName::SigningSecret],
            delivery: Delivery {
                destination_id: Uuid::nil(),
                delivered: Position {
                    transaction_id: 1,
                    sequence: 2,
                },
                batch: None,
                attempts,
                next_attempt_at: at,
                lease_owner: None,
                lease_expires_at: None,
                last_success_at: Some(at),
                last_failure_at: None,
                last_error_code: (attempts > 0).then(|| "status".to_owned()),
                last_error: (attempts > 0).then(|| "HTTP 503".to_owned()),
                delivered_count: 7,
            },
            backlog: Backlog {
                count: 3,
                oldest: Some(at - TimeDelta::minutes(1)),
            },
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn a_destination_is_sent_with_its_secret_names_and_health_only() {
        let body = serde_json::to_value(DestinationResponse::new(view(true, 2), None)).unwrap();
        assert_eq!(
            body,
            json!({
                "id": Uuid::nil(),
                "name": "Splunk",
                "kind": "webhook",
                "enabled": true,
                "settings": {"kind": "webhook", "url": "http://10.0.0.5/hook"},
                "secrets": ["signing_secret"],
                "delivery": {
                    "health": "failing",
                    "backlog": 3,
                    "oldest_pending_at": "2026-10-31T23:59:00Z",
                    "attempts": 2,
                    "next_attempt_at": "2026-11-01T00:00:00Z",
                    "last_success_at": "2026-11-01T00:00:00Z",
                    "last_failure_at": null,
                    "last_error_code": "status",
                    "last_error": "HTTP 503",
                    "delivered_count": 7,
                },
                "created_at": "2026-11-01T00:00:00Z",
                "updated_at": "2026-11-01T00:00:00Z",
            })
        );
    }

    #[test]
    fn nothing_is_scheduled_for_a_disabled_or_paused_destination() {
        let disabled = DeliveryResponse::new(&view(false, 0), None);
        assert_eq!(disabled.health, Health::Disabled);
        assert_eq!(disabled.next_attempt_at, None);
        let paused = DeliveryResponse::new(&view(true, 0), Some(Pause::Lapsed));
        assert_eq!(paused.health, Health::Paused);
        assert_eq!(paused.next_attempt_at, None);
        let healthy = DeliveryResponse::new(&view(true, 0), None);
        assert_eq!(healthy.health, Health::Healthy);
        assert!(healthy.next_attempt_at.is_some());
    }

    #[test]
    fn an_unknown_stored_failure_code_is_not_sent() {
        let mut stored = view(true, 1);
        stored.delivery.last_error_code = Some("gone".into());
        assert_eq!(DeliveryResponse::new(&stored, None).last_error_code, None);
    }

    #[test]
    fn the_list_carries_the_standing_and_the_stall() {
        let standing = Standing {
            edition: Edition::Enterprise,
            status: Status::Lapsed,
            export: false,
            streaming: false,
            retention: false,
            applies: true,
            pause: Some(Pause::Lapsed),
        };
        let since: DateTime<Utc> = "2026-11-01T00:00:00Z".parse().unwrap();
        let body = serde_json::to_value(DestinationsResponse::new(
            standing,
            Overview {
                stalled_since: Some(since),
                destinations: vec![view(true, 0)],
            },
        ))
        .unwrap();
        assert_eq!(body["standing"]["pause"], "lapsed");
        assert_eq!(body["stalled_since"], "2026-11-01T00:00:00Z");
        assert_eq!(body["destinations"][0]["delivery"]["health"], "paused");
    }

    #[test]
    fn a_create_request_defaults_to_enabled_and_reads_missing_fields_as_empty() {
        let request: CreateDestinationRequest = serde_json::from_value(json!({})).unwrap();
        assert!(request.enabled);
        let draft = Draft::from(request);
        assert_eq!(draft.name, "");
        assert_eq!(draft.settings, Value::Null);
        assert_eq!(draft.secrets, Value::Null);
    }

    #[test]
    fn requests_refuse_unknown_fields() {
        assert!(serde_json::from_value::<CreateDestinationRequest>(json!({"url": "x"})).is_err());
        assert!(serde_json::from_value::<UpdateDestinationRequest>(json!({"kind": "s3"})).is_err());
    }

    #[test]
    fn an_update_request_carries_only_what_was_sent() {
        let request: UpdateDestinationRequest =
            serde_json::from_value(json!({"enabled": false})).unwrap();
        assert!(Revision::from(request).only_disables());
    }

    #[test]
    fn debug_never_shows_a_request_secret() {
        let create: CreateDestinationRequest = serde_json::from_value(json!({
            "name": "Splunk",
            "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"},
            "secrets": {"signing_secret": "whsec-plaintext"}
        }))
        .unwrap();
        let update: UpdateDestinationRequest =
            serde_json::from_value(json!({"secrets": {"signing_secret": "whsec-plaintext"}}))
                .unwrap();
        for shown in [format!("{create:?}"), format!("{update:?}")] {
            assert!(!shown.contains("whsec-plaintext"), "{shown}");
            assert!(shown.contains(REDACTED), "{shown}");
        }
    }

    #[test]
    fn a_test_outcome_names_the_failure_code() {
        let body = serde_json::to_value(TestResponse {
            delivered: false,
            error_code: Some(FailureCode::Configuration),
            error: Some(destinations::HOST_REFUSED.into()),
        })
        .unwrap();
        assert_eq!(body["delivered"], false);
        assert_eq!(body["error_code"], "configuration");
    }
}
