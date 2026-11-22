use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::refusal::Refusal;
use crate::db::organization_members::{self, OrgRole};

pub const ORGANIZATION_MISSING: &str = "Organization not found";
pub const FORBIDDEN: &str = "Only organization owners and admins can see spend or change budgets";

/// [`administer`] for a read, which changes nothing and so needs no lock on the membership.
pub async fn admit(pool: &PgPool, organization: Uuid, user: Uuid) -> Result<(), Refusal> {
    allowed(organization_members::role(pool, organization, user).await?)
}

/// Spend and budgets are for the organization's owners and admins. A member is refused, and
/// someone outside the organization is told it doesn't exist. The membership stays locked until
/// the caller's transaction ends, so a change can't outlive the caller's removal.
pub async fn administer(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<(), Refusal> {
    allowed(organization_members::lock_role(connection, organization, user).await?)
}

fn allowed(role: Option<OrgRole>) -> Result<(), Refusal> {
    match role {
        Some(OrgRole::Owner | OrgRole::Admin) => Ok(()),
        Some(OrgRole::Member) => Err(Refusal::Forbidden(FORBIDDEN)),
        None => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}
