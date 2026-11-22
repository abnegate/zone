use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::Feature;

use super::create_token_request::CreateTokenRequest;
use super::issued_response::IssuedResponse;
use super::refusal::Refusal;
use super::{respond, target};
use crate::auth::{OrganizationMember, OrganizationPermit, Permission, entitlement};
use crate::db::audit::{actions, resources};
use crate::db::scim::TokenRecord;
use crate::routes::common::{AuditEvent, audit};
use crate::services::scim::{Issued, console};
use crate::state::AppState;

const NAME: &str = "name";
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

/// POST /api/organizations/{org_id}/scim/tokens
pub async fn create(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
    body: Result<Json<CreateTokenRequest>, JsonRejection>,
) -> Response {
    respond(issue(&state, &member, body).await)
}

/// POST /api/organizations/{org_id}/scim/tokens/{token_id}/rotations
pub async fn rotate(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Response {
    respond(replace(&state, &member, path).await)
}

/// DELETE /api/organizations/{org_id}/scim/tokens/{token_id}
///
/// Needs no license, so a lapsed one never keeps a leaked token working.
pub async fn revoke(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Response {
    respond(end(&state, &member, path).await)
}

async fn issue(
    state: &AppState,
    member: &OrganizationMember,
    body: Result<Json<CreateTokenRequest>, JsonRejection>,
) -> Result<Response, Refusal> {
    entitlement::require(state, Feature::Scim).await?;
    let Json(request) = body.map_err(|rejection| Refusal::TokenName(Some(rejection)))?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err(Refusal::TokenName(None));
    }
    let issued = console::issue(state.db(), member.organization_id, name, member.user_id).await?;
    record(
        state,
        member,
        &issued.token,
        actions::SCIM_TOKEN_CREATED,
        None,
        named(&issued.token),
    )
    .await;
    Ok(answer(issued))
}

async fn replace(
    state: &AppState,
    member: &OrganizationMember,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Response, Refusal> {
    entitlement::require(state, Feature::Scim).await?;
    let token = target(path).ok_or(Refusal::TokenMissing)?;
    let issued = console::rotate(state.db(), member.organization_id, token)
        .await?
        .ok_or(Refusal::TokenMissing)?;
    record(
        state,
        member,
        &issued.token,
        actions::SCIM_TOKEN_ROTATED,
        None,
        named(&issued.token),
    )
    .await;
    Ok(answer(issued))
}

async fn end(
    state: &AppState,
    member: &OrganizationMember,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Response, Refusal> {
    let token = target(path).ok_or(Refusal::TokenMissing)?;
    let revoked = console::revoke(state.db(), member.organization_id, token)
        .await?
        .ok_or(Refusal::TokenMissing)?;
    record(
        state,
        member,
        &revoked,
        actions::SCIM_TOKEN_REVOKED,
        named(&revoked),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn answer(issued: Issued) -> Response {
    let mut response = (StatusCode::CREATED, Json(IssuedResponse::from(issued))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, NO_STORE);
    response
}

fn named(token: &TokenRecord) -> Option<Value> {
    Some(json!({ NAME: token.name }))
}

async fn record(
    state: &AppState,
    member: &OrganizationMember,
    token: &TokenRecord,
    action: &str,
    old_values: Option<Value>,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(member.organization_id),
            workspace_id: None,
            actor_id: member.user_id,
            actor_email: &member.email,
            action,
            resource_type: resources::SCIM_TOKEN,
            resource_id: Some(token.id),
            old_values,
            new_values,
        },
    )
    .await;
}
