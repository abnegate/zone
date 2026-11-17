use sqlx::PgConnection;
use uuid::Uuid;

use super::refusal::Refusal;
use crate::db::organization_members::{self, OrgRole};

pub const ORGANIZATION_MISSING: &str = "Organization not found";
pub const FORBIDDEN: &str =
    "Only organization owners and admins can manage audit export, streaming and retention";

/// Someone outside the organization is told it doesn't exist; a member is told who may.
pub async fn organization(
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
