//! Authentication and authorization
//!
//! This module provides:
//! - JWT token creation and validation
//! - Password hashing with Argon2id
//! - Authentication middleware for axum

pub mod jwt;
pub mod middleware;
pub mod password;

pub use jwt::{create_access_token, create_refresh_token, validate_token};
pub use middleware::{AuthUser, require_auth};
pub use password::{hash_password, verify_password};
