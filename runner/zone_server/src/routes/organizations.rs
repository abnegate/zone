//! Organization endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_enterprise::Feature;

use crate::auth::{
    AuthUser, Grant, OrganizationMember, OrganizationPermit, Permission, entitlement,
};
use crate::db::DbResult;
use crate::db::audit::{actions, resources};
use crate::db::custom_roles::{AMBIGUOUS_ASSIGNMENT, Assignment, Invalid};
use crate::db::organization_members::{
    self, Assigned, MemberWithUser, OrgRole, OrganizationMemberRow, Removal as MemberRemoval,
    RoleChange,
};
use crate::db::{organizations, users, workspaces};
use crate::services::login::devices;
use crate::services::members;
use crate::state::AppState;
use crate::sync::hook::Removal;

use super::common::{
    AuditEvent, ErrorResponse, MembershipResponse, Timestamps, audit, exceeds, revoked,
};

const INVALID_ROLE: &str = "Invalid role. Must be one of: member, admin, owner";
const INTERNAL_ERROR: &str = "Internal server error";
const ORGANIZATION_NOT_FOUND: &str = "Organization not found";
const MEMBER_NOT_FOUND: &str = "Member not found";

fn refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorResponse::new(message))).into_response()
}

fn internal() -> Response {
    refusal(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_ERROR)
}

fn held(member: &OrganizationMember) -> MembershipResponse {
    MembershipResponse::organization(member.custom_role.clone(), member.grant.clone())
}

#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(flatten)]
    membership: Option<MembershipResponse>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<organizations::OrganizationRow> for OrganizationResponse {
    fn from(row: organizations::OrganizationRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
            membership: None,
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl OrganizationResponse {
    fn with_membership(mut self, membership: MembershipResponse) -> Self {
        self.membership = Some(membership);
        self
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<workspaces::WorkspaceRow> for WorkspaceResponse {
    fn from(row: workspaces::WorkspaceRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateOrganizationRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOrganizationRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct SingleOrganizationResponse {
    organization: OrganizationResponse,
}

#[derive(Debug, Serialize)]
struct OrganizationsListResponse {
    organizations: Vec<OrganizationResponse>,
}

/// GET /api/organizations - List user's organizations
pub async fn list(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    let Ok(user_id) = Uuid::parse_str(&auth.0.sub) else {
        return refusal(StatusCode::UNAUTHORIZED, "Invalid user ID in token");
    };

    match organization_members::list_user_organizations_with_role(state.db(), user_id).await {
        Ok(memberships) => Json(OrganizationsListResponse {
            organizations: memberships
                .into_iter()
                .map(|membership| {
                    OrganizationResponse::from(membership.organization).with_membership(
                        MembershipResponse::organization(membership.custom_role, membership.grant),
                    )
                })
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// POST /api/organizations - Create new organization (user becomes owner)
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(request): Json<CreateOrganizationRequest>,
) -> impl IntoResponse {
    let Ok(user_id) = Uuid::parse_str(&auth.0.sub) else {
        return refusal(StatusCode::UNAUTHORIZED, "Invalid user ID in token");
    };

    if let Err(refusal) = members::admit(&state, Some(user_id)).await {
        return refusal.into_response();
    }

    let mut transaction = match state.db().begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            tracing::error!("Failed to start transaction: {}", error);
            return internal();
        }
    };

    let organization = match organizations::create_organization_tx(
        &mut transaction,
        &request.name,
        &request.slug,
        request.description.as_deref(),
    )
    .await
    {
        Ok(organization) => organization,
        Err(error) => {
            tracing::error!("Database error creating organization: {}", error);
            let _ = transaction.rollback().await;
            return internal();
        }
    };

    if let Err(error) = organization_members::add_member_tx(
        &mut transaction,
        organization.id,
        user_id,
        OrgRole::Owner,
        None,
    )
    .await
    {
        tracing::error!("Failed to add organization owner: {}", error);
        let _ = transaction.rollback().await;
        return refusal(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create organization membership",
        );
    }

    if let Err(error) = transaction.commit().await {
        tracing::error!("Failed to commit transaction: {}", error);
        return internal();
    }

    (
        StatusCode::CREATED,
        Json(SingleOrganizationResponse {
            organization: OrganizationResponse::from(organization),
        }),
    )
        .into_response()
}

/// GET /api/organizations/:org_id - Get org details (requires membership)
pub async fn get(State(state): State<AppState>, member: OrganizationMember) -> impl IntoResponse {
    match organizations::get_organization(state.db(), member.organization_id).await {
        Ok(Some(organization)) => Json(SingleOrganizationResponse {
            organization: OrganizationResponse::from(organization).with_membership(held(&member)),
        })
        .into_response(),
        Ok(None) => refusal(StatusCode::NOT_FOUND, ORGANIZATION_NOT_FOUND),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// PATCH /api/organizations/:org_id - Update org (requires organization:update)
pub async fn update(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::OrganizationUpdate.index() }>,
    Json(request): Json<UpdateOrganizationRequest>,
) -> impl IntoResponse {
    if request.is_active == Some(true)
        && let Err(refusal) = members::reactivate(&state, member.organization_id).await
    {
        return refusal.into_response();
    }

    match organizations::update_organization(
        state.db(),
        member.organization_id,
        request.name.as_deref(),
        request.description.as_deref(),
        request.is_active,
    )
    .await
    {
        Ok(Some(organization)) => {
            let organization =
                OrganizationResponse::from(organization).with_membership(held(&member));
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(member.organization_id),
                    workspace_id: None,
                    actor_id: member.user_id,
                    actor_email: &member.email,
                    action: actions::ORGANIZATION_UPDATED,
                    resource_type: resources::ORGANIZATION,
                    resource_id: Some(member.organization_id),
                    old_values: None,
                    new_values: Some(serde_json::json!({
                        "name": organization.name,
                        "description": organization.description,
                        "is_active": organization.is_active,
                    })),
                },
            )
            .await;
            Json(SingleOrganizationResponse { organization }).into_response()
        }
        Ok(None) => refusal(StatusCode::NOT_FOUND, ORGANIZATION_NOT_FOUND),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// DELETE /api/organizations/:org_id - Delete org (requires organization:delete)
pub async fn delete(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::OrganizationDelete.index() }>,
) -> impl IntoResponse {
    let removal = match Removal::of_organization(&state, member.organization_id).await {
        Ok(removal) => removal,
        Err(error) => {
            tracing::error!(%error, "Failed to read the syncs whose webhooks the delete removes");
            return internal();
        }
    };
    let deleted = match tokio::spawn(erase(state.clone(), member.organization_id)).await {
        Ok(deleted) => deleted,
        Err(error) => {
            tracing::error!(organization = %member.organization_id, %error, "The delete did not finish");
            return internal();
        }
    };
    match deleted {
        Ok(true) => {
            removal.run(&state).await;
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(member.organization_id),
                    workspace_id: None,
                    actor_id: member.user_id,
                    actor_email: &member.email,
                    action: actions::ORGANIZATION_DELETED,
                    resource_type: resources::ORGANIZATION,
                    resource_id: Some(member.organization_id),
                    old_values: None,
                    new_values: None,
                },
            )
            .await;
            if let Err(error) = devices::forget(&state, member.organization_id).await {
                tracing::error!(
                    organization = %member.organization_id,
                    %error,
                    "Could not forget a deleted organization's coding agent sign-ins"
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => refusal(StatusCode::NOT_FOUND, ORGANIZATION_NOT_FOUND),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// Deletes the organization and stops its MCP servers, which hold its secrets. Run as a task of
/// its own: a caller that disconnects drops the handler wherever it waits, and a delete that
/// committed must still stop the servers on this replica.
async fn erase(state: AppState, organization: Uuid) -> DbResult<bool> {
    let deleted = organizations::delete_organization(state.db(), organization).await?;
    if deleted {
        state.mcp().forget(organization);
    }
    Ok(deleted)
}

#[derive(Debug, Serialize)]
struct SingleWorkspaceResponse {
    workspace: WorkspaceResponse,
}

/// POST /api/organizations/:org_id/workspaces
pub async fn create_workspace(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::WorkspacesCreate.index() }>,
    Json(request): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    let workspace = match workspaces::create_workspace_with_owner(
        state.db(),
        member.organization_id,
        &request.name,
        &request.slug,
        request.description.as_deref(),
        member.user_id,
    )
    .await
    {
        Ok(workspace) => WorkspaceResponse::from(workspace),
        Err(error) => {
            tracing::error!("Database error creating workspace: {}", error);
            return internal();
        }
    };

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(member.organization_id),
            workspace_id: Some(workspace.id),
            actor_id: member.user_id,
            actor_email: &member.email,
            action: actions::WORKSPACE_CREATED,
            resource_type: resources::WORKSPACE,
            resource_id: Some(workspace.id),
            old_values: None,
            new_values: serde_json::to_value(&workspace).ok(),
        },
    )
    .await;

    (
        StatusCode::CREATED,
        Json(SingleWorkspaceResponse { workspace }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct OrganizationMemberResponse {
    id: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
    #[serde(flatten)]
    membership: MembershipResponse,
    is_active: bool,
    invited_by: Option<Uuid>,
    email: Option<String>,
    display_name: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl OrganizationMemberResponse {
    fn seat(row: OrganizationMemberRow, membership: MembershipResponse) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            user_id: row.user_id,
            membership,
            is_active: row.is_active,
            invited_by: row.invited_by,
            email: None,
            display_name: None,
            timestamps: Timestamps::from_naive(Some(row.created_at), Some(row.updated_at)),
        }
    }
}

impl From<MemberWithUser> for OrganizationMemberResponse {
    fn from(member: MemberWithUser) -> Self {
        Self {
            email: Some(member.email),
            display_name: member.display_name,
            ..Self::seat(
                member.member,
                MembershipResponse::organization(member.custom_role, member.grant),
            )
        }
    }
}

/// A member as the console lists them, holding what the write that seated or
/// changed them recorded, with the account's email and name when it can be
/// read.
async fn describe(
    state: &AppState,
    row: OrganizationMemberRow,
    membership: MembershipResponse,
) -> OrganizationMemberResponse {
    let seated = OrganizationMemberResponse::seat(row, membership);
    match users::get_user_by_id(state.db(), seated.user_id).await {
        Ok(Some(user)) => OrganizationMemberResponse {
            email: Some(user.email),
            display_name: user.display_name,
            ..seated
        },
        Ok(None) => seated,
        Err(error) => {
            tracing::warn!(%error, user_id = %seated.user_id, "Failed to read the member's account");
            seated
        }
    }
}

async fn audit_member(
    state: &AppState,
    actor: &OrganizationMember,
    action: &str,
    user_id: Uuid,
    membership: Option<&MembershipResponse>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(actor.organization_id),
            workspace_id: None,
            actor_id: actor.user_id,
            actor_email: &actor.email,
            action,
            resource_type: resources::MEMBER,
            resource_id: Some(user_id),
            old_values: None,
            new_values: membership.map(|membership| membership.audited(user_id)),
        },
    )
    .await;
}

/// Add member request. The console's organization form names the invitee by
/// email, the workspace form and the API's own callers by id, so either
/// identifies the member and exactly one must be given.
#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    user_id: Option<Uuid>,
    email: Option<String>,
    role: String,
}

/// A member's new role: a built-in `role` or a `custom_role_id`, exactly one.
#[derive(Debug, Deserialize)]
pub struct UpdateMemberRoleRequest {
    role: Option<String>,
    custom_role_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct OrganizationMembersListResponse {
    members: Vec<OrganizationMemberResponse>,
}

/// GET /api/organizations/:org_id/members - List members (requires membership)
pub async fn list_members(
    State(state): State<AppState>,
    member: OrganizationMember,
) -> impl IntoResponse {
    match organization_members::list_members_with_users(state.db(), member.organization_id).await {
        Ok(members) => Json(OrganizationMembersListResponse {
            members: members
                .into_iter()
                .map(OrganizationMemberResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// POST /api/organizations/:org_id/members - Add member (requires members:create)
pub async fn add_member(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::MembersCreate.index() }>,
    Json(request): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let Ok(role) = request.role.parse::<OrgRole>() else {
        return refusal(StatusCode::BAD_REQUEST, INVALID_ROLE);
    };

    if role >= OrgRole::Admin && !member.allows(Permission::AdministratorsUpdate) {
        return refusal(
            StatusCode::FORBIDDEN,
            "Only owners can grant admin or owner role",
        );
    }
    if !member.grant.covers(&Grant::organization(role)) {
        return exceeds().into_response();
    }

    let target = match (request.user_id, request.email.as_deref()) {
        (Some(user_id), None) => user_id,
        (None, Some(email)) => match users::get_user_by_email(state.db(), email).await {
            Ok(Some(user)) => user.id,
            Ok(None) => {
                return refusal(
                    StatusCode::NOT_FOUND,
                    "No account uses that email. Send an invitation instead.",
                );
            }
            Err(error) => {
                tracing::error!("Database error resolving email: {}", error);
                return internal();
            }
        },
        _ => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "Name the member by either user_id or email",
            );
        }
    };

    if let Err(refusal) = members::admit(&state, Some(target)).await {
        return refusal.into_response();
    }

    let (added, status) =
        match organization_members::get_member(state.db(), member.organization_id, target).await {
            Ok(Some(existing)) if existing.is_active => {
                return refusal(
                    StatusCode::CONFLICT,
                    "User is already a member of this organization",
                );
            }
            Ok(Some(_)) => (
                organization_members::reactivate_member(
                    state.db(),
                    member.organization_id,
                    target,
                    role,
                    Some(member.user_id),
                )
                .await,
                StatusCode::OK,
            ),
            Ok(None) => (
                organization_members::add_member(
                    state.db(),
                    member.organization_id,
                    target,
                    role,
                    Some(member.user_id),
                )
                .await,
                StatusCode::CREATED,
            ),
            Err(error) => {
                tracing::error!("Database error checking existing member: {}", error);
                return internal();
            }
        };

    match added {
        Ok(row) => {
            let user_id = row.user_id;
            let membership = MembershipResponse::organization(None, Grant::organization(row.role));
            let response = describe(&state, row, membership).await;
            audit_member(
                &state,
                &member,
                actions::MEMBER_ADDED,
                user_id,
                Some(&response.membership),
            )
            .await;
            (status, Json(response)).into_response()
        }
        Err(error) => {
            tracing::error!("Database error adding member: {}", error);
            internal()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct MemberPath {
    org_id: Uuid,
    user_id: Uuid,
}

/// PATCH /api/organizations/:org_id/members/:user_id - Update role (requires members:update)
pub async fn update_member_role(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::MembersUpdate.index() }>,
    Path(path): Path<MemberPath>,
    Json(request): Json<UpdateMemberRoleRequest>,
) -> impl IntoResponse {
    let assignment =
        match Assignment::<OrgRole>::parse(request.role.as_deref(), request.custom_role_id) {
            Ok(assignment) => assignment,
            Err(Invalid::Unknown) => return refusal(StatusCode::BAD_REQUEST, INVALID_ROLE),
            Err(Invalid::Ambiguous) => {
                return refusal(StatusCode::BAD_REQUEST, AMBIGUOUS_ASSIGNMENT);
            }
        };

    match assignment {
        Assignment::Custom(_) => {
            if let Err(unentitled) = entitlement::require(&state, Feature::CustomRoles).await {
                return unentitled.into_response();
            }
        }
        Assignment::BuiltIn(OrgRole::Owner) if !member.allows(Permission::AdministratorsUpdate) => {
            return refusal(StatusCode::FORBIDDEN, "Only owners can grant owner role");
        }
        Assignment::BuiltIn(_) => {}
    }

    match organization_members::change_role(
        state.db(),
        member.organization_id,
        path.user_id,
        assignment,
        member.user_id,
    )
    .await
    {
        Ok(RoleChange::Applied(assigned)) => {
            let Assigned {
                member: row,
                membership,
            } = *assigned;
            let user_id = row.user_id;
            let membership =
                MembershipResponse::organization(membership.custom_role, membership.grant);
            let response = describe(&state, row, membership).await;
            audit_member(
                &state,
                &member,
                actions::MEMBER_ROLE_CHANGED,
                user_id,
                Some(&response.membership),
            )
            .await;
            Json(response).into_response()
        }
        Ok(RoleChange::Revoked) => revoked(Permission::MembersUpdate).into_response(),
        Ok(RoleChange::Forbidden) => refusal(
            StatusCode::FORBIDDEN,
            "Only owners can change the role of an admin or owner",
        ),
        Ok(RoleChange::Uncovered | RoleChange::Exceeds) => exceeds().into_response(),
        Ok(RoleChange::Missing) => refusal(StatusCode::NOT_FOUND, MEMBER_NOT_FOUND),
        Ok(RoleChange::LastOwner) => refusal(
            StatusCode::FORBIDDEN,
            "Cannot demote the last owner of the organization",
        ),
        Ok(RoleChange::RoleMissing) => refusal(StatusCode::NOT_FOUND, "Role not found"),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

/// DELETE /api/organizations/:org_id/members/:user_id - Remove member (requires members:delete)
pub async fn remove_member(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::MembersDelete.index() }>,
    Path(path): Path<MemberPath>,
) -> impl IntoResponse {
    match organization_members::remove_guarded(
        state.db(),
        member.organization_id,
        path.user_id,
        member.user_id,
    )
    .await
    {
        Ok(MemberRemoval::Removed) => {
            audit_member(&state, &member, actions::MEMBER_REMOVED, path.user_id, None).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(MemberRemoval::Revoked) => revoked(Permission::MembersDelete).into_response(),
        Ok(MemberRemoval::Forbidden) => refusal(
            StatusCode::FORBIDDEN,
            "Only owners can remove admins or owners",
        ),
        Ok(MemberRemoval::Uncovered) => exceeds().into_response(),
        Ok(MemberRemoval::LastOwner) => refusal(
            StatusCode::FORBIDDEN,
            "Cannot remove the last owner of the organization",
        ),
        Ok(MemberRemoval::Missing) => refusal(StatusCode::NOT_FOUND, MEMBER_NOT_FOUND),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            internal()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Weak;
    use std::time::Duration;

    use abnegate_secret::SecretValue;
    use abnegate_secret::encrypt_value;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;
    use crate::config::Config;
    use crate::db::projects;
    use crate::db::sync_config;
    use crate::services::mcp_servers::load;
    use crate::services::mcp_servers::organization::OrganizationHub;
    use crate::services::mcp_servers::testing::Tenant;
    use crate::services::mcp_servers::testing::stub;
    use crate::state::test_config;
    use crate::sync::hook::HOOK_ID;
    use crate::sync::hook::HOOK_SIGNED;

    const REPOSITORY: &str = "http://127.0.0.1/acme/project";
    const WEBHOOK: u64 = 7;
    const POLL: Duration = Duration::from_millis(20);
    const PATIENCE: Duration = Duration::from_secs(20);
    const NEVER: Duration = Duration::from_secs(3600);
    const HELD: &str = "SELECT pg_backend_pid() FROM organizations WHERE id = $1 FOR UPDATE";
    const BLOCKING: &str = "SELECT EXISTS(SELECT 1 FROM pg_stat_activity \
         WHERE wait_event_type = 'Lock' AND $1 = ANY(pg_blocking_pids(pid)))";

    fn owner(tenant: &Tenant) -> OrganizationPermit<{ Permission::OrganizationDelete.index() }> {
        OrganizationPermit(OrganizationMember {
            organization_id: tenant.organization,
            user_id: tenant.user,
            email: tenant.email.clone(),
            role: OrgRole::Owner,
            custom_role: None,
            grant: Grant::organization(OrgRole::Owner),
        })
    }

    /// How many servers the organization's hub connected, and the hub, held only weakly.
    async fn serving(state: &AppState, tenant: &Tenant) -> (usize, Weak<OrganizationHub>) {
        tenant.store(state, stub("stub")).await;
        let hub = load::organization(state, tenant.organization)
            .await
            .expect("the organization's hub");
        (hub.connected(), Arc::downgrade(&hub))
    }

    /// A GitHub sync in the tenant's workspace whose webhook Zone registered.
    async fn hooked(state: &AppState, tenant: &Tenant) {
        let project =
            projects::create_project(&tenant.pool, "Hooked", None, Some(tenant.workspace))
                .await
                .expect("a project");
        let token = encrypt_value(&SecretValue::new("ghp_hooks"), state.master_key())
            .expect("a sealed token");
        projects::link_github(&tenant.pool, project.id, REPOSITORY, Some(&token), None)
            .await
            .expect("the project is connected to GitHub");
        sync_config::create_sync_config(
            &tenant.pool,
            project.id,
            "github",
            true,
            json!({ "external_repo_url": REPOSITORY, HOOK_ID: WEBHOOK, HOOK_SIGNED: true }),
            None,
        )
        .await
        .expect("a sync with a registered webhook");
    }

    async fn eventually(mut met: impl AsyncFnMut() -> bool) -> bool {
        tokio::time::timeout(PATIENCE, async {
            while !met().await {
                tokio::time::sleep(POLL).await;
            }
        })
        .await
        .is_ok()
    }

    async fn removing(github: &MockServer) -> bool {
        github
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .any(|request| request.method.as_str() == "DELETE")
    }

    async fn blocking(tenant: &Tenant, holder: i32) -> bool {
        sqlx::query_scalar(BLOCKING)
            .bind(holder)
            .fetch_one(&tenant.pool)
            .await
            .expect("the sessions the holder blocks")
    }

    async fn stored(tenant: &Tenant) -> bool {
        organizations::get_organization(&tenant.pool, tenant.organization)
            .await
            .expect("the organization is read")
            .is_some()
    }

    #[tokio::test]
    async fn deleting_an_organization_stops_its_mcp_servers() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let (connected, held) = serving(&state, &tenant).await;

        let response = delete(State(state.clone()), owner(&tenant))
            .await
            .into_response();
        let forgotten = state
            .mcp()
            .existing_organization(tenant.organization)
            .is_none();
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }

    #[tokio::test]
    async fn a_caller_that_leaves_while_a_webhook_is_removed_leaves_no_mcp_server_running() {
        let github = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("/repos/acme/project/hooks/{WEBHOOK}")))
            .respond_with(ResponseTemplate::new(204).set_delay(NEVER))
            .mount(&github)
            .await;
        let tenant = Tenant::new().await;
        let state = tenant
            .configured(Config {
                github_api_url: github.uri(),
                github_api_allows_http: true,
                mcp_commands: true,
                ..test_config()
            })
            .await;
        hooked(&state, &tenant).await;
        let (connected, held) = serving(&state, &tenant).await;

        let reached = {
            let deleting = delete(State(state.clone()), owner(&tenant));
            tokio::pin!(deleting);
            tokio::select! {
                _ = &mut deleting => false,
                reached = eventually(async || removing(&github).await) => reached,
            }
        };
        let kept = stored(&tenant).await;
        let forgotten = state
            .mcp()
            .existing_organization(tenant.organization)
            .is_none();
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert!(reached, "the delete never came to remove the webhook");
        assert!(!kept, "the organization was not deleted");
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }

    #[tokio::test]
    async fn a_caller_that_leaves_before_the_delete_commits_leaves_no_mcp_server_running() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let (connected, held) = serving(&state, &tenant).await;
        let mut holding = tenant.pool.begin().await.expect("a transaction");
        let holder: i32 = sqlx::query_scalar(HELD)
            .bind(tenant.organization)
            .fetch_one(&mut *holding)
            .await
            .expect("the organization is held");

        let waited = {
            let deleting = delete(State(state.clone()), owner(&tenant));
            tokio::pin!(deleting);
            tokio::select! {
                _ = &mut deleting => false,
                waited = eventually(async || blocking(&tenant, holder).await) => waited,
            }
        };
        holding
            .commit()
            .await
            .expect("the organization is released");
        let deleted = eventually(async || !stored(&tenant).await).await;
        let forgotten = eventually(async || {
            state
                .mcp()
                .existing_organization(tenant.organization)
                .is_none()
        })
        .await;
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert!(waited, "the delete never waited for the organization");
        assert!(deleted, "the organization was not deleted");
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }
}
