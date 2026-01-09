//! Invitation database queries
//!
//! Handles creation, validation, acceptance, and management of organization invitations.
//! Tokens are hashed before storage for security.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use crate::utils::crypto::{generate_token, hash_token};

const TOKEN_EXPIRY_DAYS: i64 = 7; // Invitations expire in 7 days

/// Invitation row from database
#[derive(Debug, Clone)]
pub struct Invitation {
    pub id: Uuid,
    pub email: String,
    pub organization_id: Uuid,
    pub workspace_ids: Vec<Uuid>,
    pub org_role: String,
    pub workspace_role: String,
    pub invited_by: Uuid,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Create a new invitation
///
/// This function:
/// 1. Generates a unique random token
/// 2. Hashes the token for secure storage
/// 3. Stores the invitation with 7-day expiry
///
/// Returns the invitation and the plain token (to send in email)
pub async fn create_invitation(
    pool: &PgPool,
    email: &str,
    organization_id: Uuid,
    workspace_ids: Vec<Uuid>,
    org_role: &str,
    workspace_role: &str,
    invited_by: Uuid,
) -> DbResult<(Invitation, String)> {
    let token = generate_token();
    let token_hash = hash_token(&token);
    let expires_at = Utc::now() + Duration::days(TOKEN_EXPIRY_DAYS);

    let row = sqlx::query!(
        r#"
        INSERT INTO invitations (
            email, organization_id, workspace_ids, org_role, workspace_role,
            token_hash, invited_by, expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, email, organization_id, workspace_ids, org_role,
                  workspace_role, invited_by, expires_at, accepted_at, created_at
        "#,
        email,
        organization_id,
        &workspace_ids,
        org_role,
        workspace_role,
        token_hash,
        invited_by,
        expires_at
    )
    .fetch_one(pool)
    .await?;

    let invitation = Invitation {
        id: row.id,
        email: row.email,
        organization_id: row.organization_id,
        workspace_ids: row.workspace_ids.unwrap_or_default(),
        org_role: row.org_role,
        workspace_role: row.workspace_role,
        invited_by: row.invited_by,
        expires_at: row.expires_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at,
    };

    Ok((invitation, token))
}

/// Get an invitation by token
///
/// Returns the invitation if:
/// - Token is valid
/// - Invitation is not expired
/// - Invitation has not been accepted
///
/// Returns None otherwise
pub async fn get_invitation_by_token(pool: &PgPool, token: &str) -> DbResult<Option<Invitation>> {
    let token_hash = hash_token(token);

    let row = sqlx::query!(
        r#"
        SELECT id, email, organization_id, workspace_ids, org_role,
               workspace_role, invited_by, expires_at, accepted_at, created_at
        FROM invitations
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND accepted_at IS NULL
        "#,
        token_hash
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Invitation {
        id: r.id,
        email: r.email,
        organization_id: r.organization_id,
        workspace_ids: r.workspace_ids.unwrap_or_default(),
        org_role: r.org_role,
        workspace_role: r.workspace_role,
        invited_by: r.invited_by,
        expires_at: r.expires_at,
        accepted_at: r.accepted_at,
        created_at: r.created_at,
    }))
}

/// Accept an invitation
///
/// This function atomically:
/// 1. Verifies the token is valid, not expired, and not already accepted
/// 2. Marks the invitation as accepted
/// 3. Adds the user to the organization with the specified role
/// 4. Adds the user to all specified workspaces with the specified role
///
/// Returns an error if the token is invalid, expired, or already accepted
pub async fn accept_invitation(pool: &PgPool, token: &str, user_id: Uuid) -> DbResult<()> {
    let token_hash = hash_token(token);

    // Start a transaction
    let mut tx = pool.begin().await?;

    // Get and mark invitation as accepted atomically
    let row = sqlx::query!(
        r#"
        UPDATE invitations
        SET accepted_at = NOW()
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND accepted_at IS NULL
        RETURNING organization_id, workspace_ids, org_role, workspace_role
        "#,
        token_hash
    )
    .fetch_optional(&mut *tx)
    .await?;

    let invitation = row.ok_or(sqlx::Error::RowNotFound)?;

    // Add user to organization
    let org_role = invitation
        .org_role
        .parse()
        .unwrap_or(super::organization_members::OrgRole::Member);

    // Use reactivate_member to handle cases where user was previously a member
    super::organization_members::reactivate_member(
        &mut *tx,
        invitation.organization_id,
        user_id,
        org_role,
        None,
    )
    .await?;

    // Add user to workspaces
    let workspace_role = invitation
        .workspace_role
        .parse()
        .unwrap_or(super::workspace_members::WorkspaceRole::Member);

    for workspace_id in invitation.workspace_ids.unwrap_or_default() {
        // Use reactivate_member to handle cases where user was previously a member
        super::workspace_members::reactivate_member(
            &mut *tx,
            workspace_id,
            user_id,
            workspace_role,
            None,
        )
        .await?;
    }

    tx.commit().await?;

    Ok(())
}

/// List all pending invitations for an organization
///
/// Returns invitations that are:
/// - Not expired
/// - Not yet accepted
pub async fn list_pending_invitations(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Vec<Invitation>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, email, organization_id, workspace_ids, org_role,
               workspace_role, invited_by, expires_at, accepted_at, created_at
        FROM invitations
        WHERE organization_id = $1
          AND expires_at > NOW()
          AND accepted_at IS NULL
        ORDER BY created_at DESC
        "#,
        organization_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Invitation {
            id: r.id,
            email: r.email,
            organization_id: r.organization_id,
            workspace_ids: r.workspace_ids.unwrap_or_default(),
            org_role: r.org_role,
            workspace_role: r.workspace_role,
            invited_by: r.invited_by,
            expires_at: r.expires_at,
            accepted_at: r.accepted_at,
            created_at: r.created_at,
        })
        .collect())
}

/// Revoke (delete) an invitation
///
/// Returns an error if the invitation doesn't exist
pub async fn revoke_invitation(pool: &PgPool, invitation_id: Uuid) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM invitations WHERE id = $1", invitation_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Get a pending invitation for a specific email in an organization
///
/// Returns the invitation if:
/// - Email matches
/// - Organization matches
/// - Invitation is not expired
/// - Invitation has not been accepted
///
/// Returns None otherwise
pub async fn get_pending_invitation_for_email(
    pool: &PgPool,
    email: &str,
    organization_id: Uuid,
) -> DbResult<Option<Invitation>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email, organization_id, workspace_ids, org_role,
               workspace_role, invited_by, expires_at, accepted_at, created_at
        FROM invitations
        WHERE email = $1
          AND organization_id = $2
          AND expires_at > NOW()
          AND accepted_at IS NULL
        "#,
        email,
        organization_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Invitation {
        id: r.id,
        email: r.email,
        organization_id: r.organization_id,
        workspace_ids: r.workspace_ids.unwrap_or_default(),
        org_role: r.org_role,
        workspace_role: r.workspace_role,
        invited_by: r.invited_by,
        expires_at: r.expires_at,
        accepted_at: r.accepted_at,
        created_at: r.created_at,
    }))
}

/// Get an invitation by ID (regardless of status)
///
/// Returns the invitation if it exists, None otherwise
/// This is used to verify ownership before performing operations
pub async fn get_invitation_by_id(
    pool: &PgPool,
    invitation_id: Uuid,
) -> DbResult<Option<Invitation>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email, organization_id, workspace_ids, org_role,
               workspace_role, invited_by, expires_at, accepted_at, created_at
        FROM invitations
        WHERE id = $1
        "#,
        invitation_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Invitation {
        id: r.id,
        email: r.email,
        organization_id: r.organization_id,
        workspace_ids: r.workspace_ids.unwrap_or_default(),
        org_role: r.org_role,
        workspace_role: r.workspace_role,
        invited_by: r.invited_by,
        expires_at: r.expires_at,
        accepted_at: r.accepted_at,
        created_at: r.created_at,
    }))
}
