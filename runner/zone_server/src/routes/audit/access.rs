use sqlx::PgConnection;
use uuid::Uuid;

use super::refusal::Refusal;
use crate::auth::Permission;
use crate::db::organization_members;

pub const ORGANIZATION_MISSING: &str = "Organization not found";
const NEEDED: Permission = Permission::AuditUpdate;

/// Someone outside the organization is told it doesn't exist; a member is told which permission
/// they lack.
pub async fn organization(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<(), Refusal> {
    match organization_members::lock_membership(connection, organization, user).await? {
        Some(membership) if membership.grant.allows(NEEDED) => Ok(()),
        Some(_) => Err(Refusal::Forbidden(NEEDED)),
        None => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}
