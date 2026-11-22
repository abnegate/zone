//! An organization's custom roles. Every active member reads them, with the permission catalogue
//! they are composed from; creating, editing and deleting one takes `roles:update` and an
//! Enterprise license, and every change is audited.

mod name;
mod refusal;
mod snapshot;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zone_enterprise::{Edition, Feature, Status, status};

use crate::auth::entitlement::Unentitled;
use crate::auth::{AuthUser, Grant, Permission, Scope};
use crate::db::audit::{actions, resources};
use crate::db::custom_roles::{
    self, Change, Creation, CustomRoleRow, Definition, Deletion, Summary, Update,
};
use crate::db::organization_members;
use crate::db::scim::{GroupRecord, Mapping};
use crate::routes::common::{AuditEvent, audit};
use crate::routes::scim::console::mappings;
use crate::services::scim::{Remapped, console};
use crate::state::AppState;
use name::Name;
use refusal::Refusal;
use snapshot::Snapshot;

const ORGANIZATION_MISSING: &str = "Organization not found";
const ROLE_MISSING: &str = "Role not found";

/// A new custom role. The scope's base permission is added whether named or not.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    name: String,
    #[serde(default)]
    description: Option<String>,
    scope: Scope,
    permissions: Vec<String>,
}

/// A sparse edit of a custom role. A role's scope never changes, so naming it is refused with
/// every other unknown key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    name: Option<String>,
    #[serde(default, deserialize_with = "crate::db::actions::patch")]
    description: Option<Option<String>>,
    permissions: Option<Vec<String>>,
}

/// A permission a custom role may be composed from, as the catalogue lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct Entry {
    name: Permission,
    scope: Scope,
    composable: bool,
}

impl Entry {
    /// Every organization and workspace permission. Server permissions never go in a role.
    fn catalogue() -> Vec<Self> {
        Permission::ALL
            .into_iter()
            .filter(|permission| permission.scope() != Scope::Server)
            .map(|permission| Self {
                name: permission,
                scope: permission.scope(),
                composable: permission.composable(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
struct Role {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    description: Option<String>,
    scope: Scope,
    permissions: Grant,
    members: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CustomRoleRow> for Role {
    fn from(row: CustomRoleRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            name: row.name,
            description: row.description,
            scope: row.scope,
            permissions: row.grant,
            members: row.members,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct Listing {
    entitled: bool,
    editable: bool,
    edition: Edition,
    status: Status,
    catalogue: Vec<Entry>,
    roles: Vec<Role>,
}

/// The answer to creating or editing a role.
#[derive(Debug, Serialize)]
struct Saved {
    role: Role,
}

/// A role's path. A role id that isn't one names no role, as a missing role does.
struct Target {
    organization: Uuid,
    role: Option<Uuid>,
}

impl Target {
    fn parse(path: Result<Path<(String, String)>, PathRejection>) -> Result<Self, Refusal> {
        let Ok(Path((organization, role))) = path else {
            return Err(Refusal::Missing(ORGANIZATION_MISSING));
        };
        let organization = organization
            .parse()
            .map_err(|_| Refusal::Missing(ORGANIZATION_MISSING))?;
        Ok(Self {
            organization,
            role: role.parse().ok(),
        })
    }

    fn role(&self) -> Result<Uuid, Refusal> {
        self.role.ok_or(Refusal::Missing(ROLE_MISSING))
    }
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
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<Edit>, JsonRejection>,
) -> Response {
    respond(change(&state, &auth, path, body).await)
}

/// DELETE /api/organizations/{org_id}/roles/{role_id}
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
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

/// Someone allowed to change the organization's roles, on a license that allows it. Checks
/// `roles:update`, then the license, before anything in the request is read.
async fn editor(state: &AppState, auth: &AuthUser, organization: Uuid) -> Result<Uuid, Refusal> {
    let (editor, grant) = member(state, auth, organization).await?;
    if !grant.allows(Permission::RolesUpdate) {
        return Err(forbidden());
    }
    let stored = state.stored_license().await?;
    if !Feature::CustomRoles.entitled(stored.license(), Utc::now()) {
        return Err(Unentitled(Feature::CustomRoles).into());
    }
    Ok(editor)
}

fn forbidden() -> Refusal {
    Refusal::Forbidden(Permission::RolesUpdate.refusal())
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
    let stored = state.stored_license().await?;
    let (license, now) = (stored.license(), Utc::now());
    let entitled = Feature::CustomRoles.entitled(license, now);
    let roles = custom_roles::list(state.db(), organization).await?;
    Ok(Json(Listing {
        entitled,
        editable: entitled && grant.allows(Permission::RolesUpdate),
        edition: Edition::from(license),
        status: status(license, now),
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
    let created = match custom_roles::create(state.db(), organization, &definition, editor).await? {
        Creation::Created(created) => *created,
        Creation::Forbidden => return Err(forbidden()),
        Creation::NameTaken => return Err(Refusal::NameTaken),
    };
    record(
        state,
        auth,
        editor,
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
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<Edit>, JsonRejection>,
) -> Result<Response, Refusal> {
    let target = Target::parse(path)?;
    let editor = editor(state, auth, target.organization).await?;
    let Json(edit) = body.map_err(Refusal::Body)?;
    let name = edit.name.as_deref().map(Name::parse).transpose()?;
    let role = target.role()?;
    let Some(current) = custom_roles::get(state.db(), target.organization, role).await? else {
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
    let updated =
        custom_roles::update(state.db(), target.organization, role, &patch, editor).await?;
    let (previous, saved) = match updated {
        Update::Saved { previous, saved } => (*previous, *saved),
        Update::Forbidden => return Err(forbidden()),
        Update::Exceeds => return Err(Refusal::Exceeds),
        Update::NameTaken => return Err(Refusal::NameTaken),
        Update::Missing => return Err(Refusal::Missing(ROLE_MISSING)),
    };
    let (before, after) = (Snapshot::of(&previous), Snapshot::of(&saved));
    if before != after {
        record(
            state,
            auth,
            editor,
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
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, Refusal> {
    let target = Target::parse(path)?;
    let editor = editor(state, auth, target.organization).await?;
    let role = target.role()?;
    let (deleted, unmapped) =
        match custom_roles::delete(state.db(), target.organization, role, editor).await? {
            Deletion::Deleted { role, unmapped } => (*role, unmapped),
            Deletion::Forbidden => return Err(forbidden()),
            Deletion::InUse(members) => return Err(Refusal::InUse(members)),
            Deletion::Missing => return Err(Refusal::Missing(ROLE_MISSING)),
        };
    record(
        state,
        auth,
        editor,
        &deleted,
        actions::ROLE_DELETED,
        Snapshot::of(&deleted).value(),
        None,
    )
    .await;
    release(state, auth, editor, &deleted, unmapped).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn release(
    state: &AppState,
    auth: &AuthUser,
    editor: Uuid,
    deleted: &CustomRoleRow,
    unmapped: Vec<GroupRecord>,
) {
    let role = Summary {
        id: deleted.id,
        name: deleted.name.clone(),
    };
    let released = match console::release(
        state.db(),
        deleted.organization_id,
        &role,
        unmapped.clone(),
    )
    .await
    {
        Ok(released) => released,
        Err(error) => {
            tracing::error!(%error, role_id = %deleted.id, "Failed to settle the roles of the SCIM groups a deleted role was mapped to");
            unmapped
                .into_iter()
                .map(|group| Remapped {
                    previous: Mapping::Custom(role.clone()),
                    group,
                    outcomes: Vec::new(),
                })
                .collect()
        }
    };
    for remapped in &released {
        mappings::record(state, editor, &auth.0.email, remapped).await;
    }
}

async fn record(
    state: &AppState,
    auth: &AuthUser,
    editor: Uuid,
    role: &CustomRoleRow,
    action: &str,
    old_values: Option<Value>,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(role.organization_id),
            workspace_id: None,
            actor_id: editor,
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
