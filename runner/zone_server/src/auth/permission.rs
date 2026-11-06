//! Server-wide permissions granted through global roles.

use sqlx::PgPool;
use uuid::Uuid;

use crate::db::{DbResult, users};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ModelsCreate,
    ModelsDelete,
}

impl Permission {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ModelsCreate => "models:create",
            Self::ModelsDelete => "models:delete",
        }
    }

    pub fn refusal(self) -> String {
        let action = match self {
            Self::ModelsCreate => "Installing or training models on this server",
            Self::ModelsDelete => "Removing models from this server",
        };
        format!(
            "{action} requires the {} permission. Ask a server admin for access.",
            self.name()
        )
    }
}

/// Reads the grant from the database rather than a token, for callers such as
/// agent tools that act for a user without holding their claims.
pub async fn holds(pool: &PgPool, user_id: Uuid, permission: Permission) -> DbResult<bool> {
    Ok(users::get_user_with_permissions(pool, user_id)
        .await?
        .is_some_and(|user| {
            user.user.is_admin.unwrap_or(false)
                || user
                    .permissions
                    .iter()
                    .any(|name| name == permission.name())
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_seeded_permissions() {
        assert_eq!(Permission::ModelsCreate.name(), "models:create");
        assert_eq!(Permission::ModelsDelete.name(), "models:delete");
    }

    #[test]
    fn refusal_names_the_missing_permission() {
        let refusal = Permission::ModelsCreate.refusal();
        assert!(refusal.contains("models:create"), "{refusal}");
        assert!(refusal.contains("server admin"), "{refusal}");
        assert!(Permission::ModelsDelete.refusal().contains("models:delete"));
    }
}
