use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::db::organization_members::{self, OrgRole};
use crate::db::users::{self, UserRow};
use crate::db::workspace_members::{self, WorkspaceRole};
use crate::db::{DbResult, email_verification, organizations, workspaces};

pub const ROLE: &str = "user";

const UNNAMED_ORGANIZATION: &str = "My Organization";
const WORKSPACE_NAME: &str = "Default Workspace";
const WORKSPACE_SLUG: &str = "default";
const WORKSPACE_DESCRIPTION: &str = "Your default workspace";
const SLUG_SUFFIX_LENGTH: usize = 8;

pub async fn insert(
    connection: &mut PgConnection,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    is_admin: bool,
    email_verified: bool,
) -> DbResult<UserRow> {
    let mut user = users::create_user(
        &mut *connection,
        &email.to_lowercase(),
        password_hash,
        display_name,
        is_admin,
    )
    .await?;
    users::assign_user_role(&mut *connection, user.id, ROLE).await?;
    if email_verified {
        email_verification::mark_email_verified(&mut *connection, user.id).await?;
        user.email_verified = true;
    }
    Ok(user)
}

/// A default organization and workspace the user owns. Best-effort: the user can create both
/// by hand, so a failure is logged and never undoes the account.
pub async fn furnish(pool: &PgPool, user: &UserRow) {
    if let Err(error) = organization(pool, user).await {
        tracing::error!(
            user_id = %user.id,
            %error,
            "Failed to create the default organization and workspace; the user can create them by hand"
        );
    }
}

async fn organization(pool: &PgPool, user: &UserRow) -> DbResult<()> {
    let name = organization_name(user.display_name.as_deref());
    let slug = organization_slug(&name, user.id);
    let organization = organizations::create_organization(pool, &name, &slug, None).await?;
    organization_members::add_member(pool, organization.id, user.id, OrgRole::Owner, None).await?;
    let workspace = workspaces::create_workspace(
        pool,
        organization.id,
        WORKSPACE_NAME,
        WORKSPACE_SLUG,
        Some(WORKSPACE_DESCRIPTION),
    )
    .await?;
    workspace_members::add_member(pool, workspace.id, user.id, WorkspaceRole::Owner, None).await?;
    tracing::info!(
        user_id = %user.id,
        organization_id = %organization.id,
        workspace_id = %workspace.id,
        "Created the default organization and workspace"
    );
    Ok(())
}

fn organization_name(display_name: Option<&str>) -> String {
    display_name
        .map(|name| format!("{name}'s Organization"))
        .unwrap_or_else(|| UNNAMED_ORGANIZATION.to_owned())
}

fn organization_slug(name: &str, user_id: Uuid) -> String {
    let base: String = name
        .to_lowercase()
        .replace(' ', "-")
        .replace('\'', "")
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '-')
        .collect();
    format!("{base}-{}", &user_id.to_string()[..SLUG_SUFFIX_LENGTH])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_organization_is_named_after_the_person() {
        assert_eq!(organization_name(Some("Ada")), "Ada's Organization");
        assert_eq!(organization_name(None), "My Organization");
    }

    #[test]
    fn the_slug_keeps_letters_digits_and_dashes_and_ends_with_the_user() {
        let user_id = Uuid::parse_str("0123abcd-0000-0000-0000-000000000000").unwrap();
        assert_eq!(
            organization_slug("Ada Lovelace's Organization", user_id),
            "ada-lovelaces-organization-0123abcd"
        );
        assert_eq!(
            organization_slug("Zoë & Co.'s Organization", user_id),
            "zoë--cos-organization-0123abcd"
        );
        assert_eq!(
            organization_slug(UNNAMED_ORGANIZATION, user_id),
            "my-organization-0123abcd"
        );
    }
}
