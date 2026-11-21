//! An organization's custom roles. Every active member reads them, with the permission catalogue
//! they are composed from; creating, editing and deleting one takes `roles:update` and an
//! Enterprise license, and every change is audited.

pub mod draft;
pub mod edit;
pub mod entry;
pub mod in_use;
pub mod listing;
pub mod name;
pub mod refusal;
pub mod role;
pub mod saved;
pub mod snapshot;
pub mod standing;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::auth::{AuthUser, Grant, Permission};
use crate::db::audit::{actions, resources};
use crate::db::custom_roles::{self, Change, Creation, Definition, Deletion, Update};
use crate::db::organization_members;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;
use draft::Draft;
use edit::Edit;
use entry::Entry;
use listing::Listing;
use name::Name;
use refusal::Refusal;
use role::Role;
use saved::Saved;
use snapshot::Snapshot;
use standing::Standing;

const ORGANIZATION_MISSING: &str = "Organization not found";
const ROLE_MISSING: &str = "Role not found";

/// Someone allowed to change the organization's roles, on a license that allows it.
struct Editor {
    id: Uuid,
    grant: Grant,
}

/// GET /api/organizations/{org_id}/roles
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    respond(read(&state, &auth, path).await)
}

/// POST /api/organizations/{org_id}/roles
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Response {
    respond(insert(&state, &auth, path, body).await)
}

/// PATCH /api/organizations/{org_id}/roles/{role_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<Edit>, JsonRejection>,
) -> Response {
    respond(change(&state, &auth, path, body).await)
}

/// DELETE /api/organizations/{org_id}/roles/{role_id}
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Response {
    respond(remove(&state, &auth, path).await)
}

fn respond(result: Result<Response, Refusal>) -> Response {
    result.unwrap_or_else(IntoResponse::into_response)
}

/// The caller's grant in the organization. Someone who isn't an active member is told the
/// organization doesn't exist.
async fn member(
    state: &AppState,
    auth: &AuthUser,
    organization: Uuid,
) -> Result<(Uuid, Grant), Refusal> {
    let user = auth.0.user_id().map_err(|_| Refusal::Unauthenticated)?;
    let grant = organization_members::membership(state.db(), organization, user)
        .await?
        .map(|membership| membership.grant)
        .filter(|grant| grant.allows(Permission::OrganizationRead))
        .ok_or(Refusal::Missing(ORGANIZATION_MISSING))?;
    Ok((user, grant))
}

/// Checks `roles:update`, then the license, before anything in the request is read.
async fn editor(state: &AppState, auth: &AuthUser, organization: Uuid) -> Result<Editor, Refusal> {
    let (id, grant) = member(state, auth, organization).await?;
    if !grant.allows(Permission::RolesUpdate) {
        return Err(Refusal::Forbidden(Permission::RolesUpdate.refusal()));
    }
    Standing::of(&state.stored_license().await?, Utc::now()).require()?;
    Ok(Editor { id, grant })
}

async fn read(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Response, Refusal> {
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let (_, grant) = member(state, auth, organization).await?;
    let standing = Standing::of(&state.stored_license().await?, Utc::now());
    let roles = custom_roles::list(state.db(), organization).await?;
    Ok(Json(Listing {
        entitled: standing.entitled,
        editable: standing.editable(&grant),
        edition: standing.edition,
        status: standing.status,
        catalogue: Entry::catalogue(),
        roles: roles.into_iter().map(Role::from).collect(),
    })
    .into_response())
}

async fn insert(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Result<Response, Refusal> {
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let editor = editor(state, auth, organization).await?;
    let Json(draft) = body.map_err(Refusal::Body)?;
    let name = Name::parse(&draft.name)?;
    if draft.scope.base().is_none() {
        return Err(Refusal::Scope);
    }
    let grant = Grant::compose(draft.scope, &draft.permissions)?;
    let definition = Definition {
        name: name.as_str(),
        description: draft.description.as_deref(),
        scope: draft.scope,
        grant: &grant,
    };
    let created =
        match custom_roles::create(state.db(), organization, &definition, editor.id).await? {
            Creation::Created(created) => *created,
            Creation::NameTaken => return Err(Refusal::NameTaken),
        };
    record(
        state,
        auth,
        &editor,
        &created,
        actions::ROLE_CREATED,
        None,
        Snapshot::of(&created).value(),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(Saved {
            role: Role::from(created),
        }),
    )
        .into_response())
}

async fn change(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<Edit>, JsonRejection>,
) -> Result<Response, Refusal> {
    let Ok(Path((organization, role))) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let editor = editor(state, auth, organization).await?;
    let Json(edit) = body.map_err(Refusal::Body)?;
    let name = edit.name.as_deref().map(Name::parse).transpose()?;
    let Some(current) = custom_roles::get(state.db(), organization, role).await? else {
        return Err(Refusal::Missing(ROLE_MISSING));
    };
    let grant = edit
        .permissions
        .as_deref()
        .map(|names| Grant::compose(current.scope, names))
        .transpose()?;
    let patch = Change {
        name: name.map(|name| name.as_str()),
        description: edit.description.as_ref().map(Option::as_deref),
        grant: grant.as_ref(),
    };
    let (previous, saved) =
        match custom_roles::update(state.db(), organization, role, &patch, &editor.grant).await? {
            Update::Saved { previous, saved } => (*previous, *saved),
            Update::Exceeds => return Err(Refusal::Exceeds),
            Update::NameTaken => return Err(Refusal::NameTaken),
            Update::Missing => return Err(Refusal::Missing(ROLE_MISSING)),
        };
    let (before, after) = (Snapshot::of(&previous), Snapshot::of(&saved));
    if before != after {
        record(
            state,
            auth,
            &editor,
            &saved,
            actions::ROLE_UPDATED,
            before.value(),
            after.value(),
        )
        .await;
    }
    Ok(Json(Saved {
        role: Role::from(saved),
    })
    .into_response())
}

async fn remove(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Response, Refusal> {
    let Ok(Path((organization, role))) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let editor = editor(state, auth, organization).await?;
    let deleted = match custom_roles::delete(state.db(), organization, role).await? {
        Deletion::Deleted(deleted) => *deleted,
        Deletion::InUse(members) => return Err(Refusal::InUse(members)),
        Deletion::Missing => return Err(Refusal::Missing(ROLE_MISSING)),
    };
    record(
        state,
        auth,
        &editor,
        &deleted,
        actions::ROLE_DELETED,
        Snapshot::of(&deleted).value(),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn record(
    state: &AppState,
    auth: &AuthUser,
    editor: &Editor,
    role: &custom_roles::CustomRoleRow,
    action: &str,
    old_values: Option<Value>,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(role.organization_id),
            workspace_id: None,
            actor_id: editor.id,
            actor_email: &auth.0.email,
            action,
            resource_type: resources::ROLE,
            resource_id: Some(role.id),
            old_values,
            new_values,
        },
    )
    .await;
}
