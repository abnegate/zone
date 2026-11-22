use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::refusal::Refusal;
use crate::auth::{Grant, Permission};
use crate::db::organization_members;

pub const ORGANIZATION_MISSING: &str = "Organization not found";

/// Reading spend and budgets takes `spend:read`. A read changes nothing, so the membership is
/// not locked. The caller's grant is returned so the answer can say whether they may also edit.
pub async fn admit(pool: &PgPool, organization: Uuid, user: Uuid) -> Result<Grant, Refusal> {
    let membership = organization_members::membership(pool, organization, user).await?;
    allowed(
        membership.map(|membership| membership.grant),
        Permission::SpendRead,
    )
}

/// Changing a budget takes `spend:update`. A member without it is refused, and someone outside
/// the organization, or whose role grants nothing in it, is told it doesn't exist. The membership
/// stays locked until the caller's transaction ends, so a change can't outlive the caller's
/// removal.
pub async fn administer(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<(), Refusal> {
    let membership = organization_members::lock_membership(connection, organization, user).await?;
    allowed(
        membership.map(|membership| membership.grant),
        Permission::SpendUpdate,
    )
    .map(drop)
}

fn allowed(grant: Option<Grant>, permission: Permission) -> Result<Grant, Refusal> {
    match grant {
        Some(grant) if grant.allows(permission) => Ok(grant),
        Some(grant) if grant.allows(Permission::OrganizationRead) => {
            Err(Refusal::Forbidden(permission))
        }
        _ => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}
