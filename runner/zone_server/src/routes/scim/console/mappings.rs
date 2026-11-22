use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::Feature;

use super::group_response::GroupResponse;
use super::mapping_request::MappingRequest;
use super::refusal::Refusal;
use super::{respond, target};
use crate::auth::{OrganizationMember, OrganizationPermit, Permission, entitlement};
use crate::db::audit::{actions, resources};
use crate::db::custom_roles::{Assignment, Invalid};
use crate::db::organization_members::OrgRole;
use crate::db::scim::{Applied, Mapping, Outcome};
use crate::routes::common::{AuditEvent, audit};
use crate::services::scim::{Remapped, Remapping, console};
use crate::state::AppState;

const ROLE: &str = "role";
const CUSTOM_ROLE: &str = "custom_role";
const USER_ID: &str = "user_id";
const CHANGED: &str = "changed";

/// PUT /api/organizations/{org_id}/scim/groups/{group_id}/mapping
pub async fn put(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<MappingRequest>, JsonRejection>,
) -> Response {
    respond(map(&state, &member, path, body).await)
}

/// DELETE /api/organizations/{org_id}/scim/groups/{group_id}/mapping
pub async fn delete(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Response {
    respond(unmap(&state, &member, path).await)
}

async fn map(
    state: &AppState,
    member: &OrganizationMember,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<MappingRequest>, JsonRejection>,
) -> Result<Response, Refusal> {
    entitlement::require(state, Feature::Scim).await?;
    let group = target(path).ok_or(Refusal::GroupMissing)?;
    let Json(request) = body.map_err(|rejection| Refusal::Mapping(Some(rejection)))?;
    let assignment = Assignment::<OrgRole>::parse(request.role.as_deref(), request.custom_role_id)
        .map_err(|(Invalid::Unknown | Invalid::Ambiguous)| Refusal::Mapping(None))?;
    if let Assignment::Custom(_) = assignment {
        entitlement::require(state, Feature::CustomRoles).await?;
    }
    let remapping = console::map(
        state.db(),
        member.organization_id,
        group,
        assignment,
        member,
    )
    .await?;
    settle(state, member, remapping).await
}

async fn unmap(
    state: &AppState,
    member: &OrganizationMember,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Result<Response, Refusal> {
    entitlement::require(state, Feature::Scim).await?;
    let group = target(path).ok_or(Refusal::GroupMissing)?;
    let remapping = console::unmap(state.db(), member.organization_id, group, member).await?;
    settle(state, member, remapping).await
}

async fn settle(
    state: &AppState,
    member: &OrganizationMember,
    remapping: Remapping,
) -> Result<Response, Refusal> {
    let remapped = match remapping {
        Remapping::Remapped(remapped) => *remapped,
        Remapping::GroupMissing => return Err(Refusal::GroupMissing),
        Remapping::RoleMissing => return Err(Refusal::RoleMissing),
        Remapping::OwnerUnmappable => return Err(Refusal::OwnerUnmappable),
        Remapping::Exceeds => return Err(Refusal::Exceeds),
    };
    record(state, member, &remapped).await;
    Ok(Json(GroupResponse::from(remapped.group)).into_response())
}

/// Audits the mapping with every member whose role it changed. Saving the mapping a group already
/// had, and changing nobody's role, records nothing.
async fn record(state: &AppState, member: &OrganizationMember, remapped: &Remapped) {
    let mapping = &remapped.group.mapping;
    let changes: Vec<Value> = remapped.outcomes.iter().filter_map(changed).collect();
    if *mapping == remapped.previous && changes.is_empty() {
        return;
    }
    let action = if mapping.is_mapped() {
        actions::SCIM_GROUP_MAPPED
    } else {
        actions::SCIM_GROUP_UNMAPPED
    };
    let mut new_values = snapshot(mapping);
    new_values[CHANGED] = Value::Array(changes);
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(member.organization_id),
            workspace_id: None,
            actor_id: member.user_id,
            actor_email: &member.email,
            action,
            resource_type: resources::SCIM_GROUP,
            resource_id: Some(remapped.group.id),
            old_values: Some(snapshot(&remapped.previous)),
            new_values: Some(new_values),
        },
    )
    .await;
}

fn snapshot(mapping: &Mapping) -> Value {
    match mapping {
        Mapping::Unmapped => json!({ ROLE: null, CUSTOM_ROLE: null }),
        Mapping::BuiltIn(role) => json!({ ROLE: role.as_str(), CUSTOM_ROLE: null }),
        Mapping::Custom(summary) => json!({ ROLE: null, CUSTOM_ROLE: summary }),
    }
}

fn changed(outcome: &Outcome) -> Option<Value> {
    let Applied::Changed { to, .. } = &outcome.applied else {
        return None;
    };
    Some(json!({
        USER_ID: outcome.user_id,
        ROLE: to.role.as_str(),
        CUSTOM_ROLE: to.custom_role,
    }))
}
