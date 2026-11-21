use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::refusal::Refusal;
use crate::db::organization_members::{self, OrgRole};

pub const ORGANIZATION_MISSING: &str = "Organization not found";
pub const FORBIDDEN: &str = "Only organization owners and admins can see spend or change budgets";

/// [`administer`] for a read, which holds nothing past the check.
pub async fn admit(pool: &PgPool, organization: Uuid, user: Uuid) -> Result<(), Refusal> {
    let mut connection = pool.acquire().await?;
    administer(&mut connection, organization, user).await
}

/// Spend and budgets are for the organization's owners and admins. A member is refused, and
/// someone outside the organization is told it doesn't exist.
pub async fn administer(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
) -> Result<(), Refusal> {
    match organization_members::lock_role(connection, organization, user).await? {
        Some(OrgRole::Owner | OrgRole::Admin) => Ok(()),
        Some(OrgRole::Member) => Err(Refusal::Forbidden(FORBIDDEN)),
        None => Err(Refusal::Missing(ORGANIZATION_MISSING)),
    }
}
