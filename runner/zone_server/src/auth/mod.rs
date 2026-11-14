//! Authentication and authorization
//!
//! This module provides:
//! - JWT token creation and validation
//! - Password hashing with Argon2id
//! - Authentication middleware for axum
//! - Organization and workspace membership guards

pub mod device;
pub mod github_guard;
pub mod jwt;
pub mod middleware;
pub mod model_guard;
pub mod organization_guard;
pub mod password;
pub mod permission;
pub mod time_zone;
pub mod workspace_guard;

pub use github_guard::{PullRequestMerger, ReleaseCreator};
pub use jwt::{
    AccessClaims, create_access_token, create_refresh_token, create_session_access_token,
    validate_access_token,
};
pub use middleware::{AuthSession, AuthUser, require_auth};
pub use model_guard::{ModelInstaller, ModelRemover};
pub use organization_guard::{OrgAdmin, OrgMember, OrgOwner};
pub use password::{hash_password, verify_password};
pub use permission::Permission;
pub use workspace_guard::{WorkspaceAdmin, WorkspaceMember, WorkspaceOwner, WorkspaceWriter};
