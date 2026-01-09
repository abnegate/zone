//! Authentication and authorization
//!
//! This module provides:
//! - JWT token creation and validation
//! - Password hashing with Argon2id
//! - Authentication middleware for axum
//! - Organization and workspace membership guards

pub mod jwt;
pub mod middleware;
pub mod organization_guard;
pub mod password;
pub mod workspace_guard;

pub use jwt::{create_access_token, create_refresh_token, validate_token};
pub use middleware::{AuthUser, require_auth};
pub use organization_guard::{OrgAdmin, OrgMember, OrgOwner};
pub use password::{hash_password, verify_password};
pub use workspace_guard::{WorkspaceAdmin, WorkspaceMember, WorkspaceWriter};
