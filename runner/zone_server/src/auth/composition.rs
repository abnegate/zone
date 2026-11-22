//! Why a list of permission names cannot become a custom role.

use super::permission::Permission;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Composition {
    Unknown(String),
    NotComposable(Permission),
    OutOfScope(Permission),
    Requires {
        permission: Permission,
        needs: Permission,
    },
}

impl Composition {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unknown(_) => "permission_unknown",
            Self::NotComposable(_) => "permission_not_composable",
            Self::OutOfScope(_) => "permission_out_of_scope",
            Self::Requires { .. } => "permission_requires",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unknown(name) => format!("{name} is not a permission."),
            Self::NotComposable(permission) => {
                format!("{} cannot be given to a custom role.", permission.name())
            }
            Self::OutOfScope(permission) => format!(
                "{} is a {} permission and does not apply to this role.",
                permission.name(),
                permission.scope().name()
            ),
            Self::Requires { permission, needs } => format!(
                "{} can only be given together with {}.",
                permission.name(),
                needs.name()
            ),
        }
    }
}

impl std::fmt::Display for Composition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for Composition {}
