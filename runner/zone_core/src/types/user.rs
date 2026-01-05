//! User types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// User with their roles and permissions (used for auth)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserWithPermissions {
    #[serde(flatten)]
    pub user: User,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

/// Request to create a new user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

/// Request to update a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> User {
        User {
            id: Uuid::new_v4(),
            email: "test@example.com".to_string(),
            display_name: Some("Test User".to_string()),
            is_active: true,
            is_admin: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login_at: None,
        }
    }

    #[test]
    fn test_user_serialization() {
        let user = create_test_user();
        let json = serde_json::to_string(&user).unwrap();

        assert!(json.contains("test@example.com"));
        assert!(json.contains("Test User"));

        let deserialized: User = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email, user.email);
    }

    #[test]
    fn test_user_without_display_name() {
        let mut user = create_test_user();
        user.display_name = None;

        let json = serde_json::to_string(&user).unwrap();
        let deserialized: User = serde_json::from_str(&json).unwrap();
        assert!(deserialized.display_name.is_none());
    }

    #[test]
    fn test_user_with_last_login() {
        let mut user = create_test_user();
        user.last_login_at = Some(Utc::now());

        let json = serde_json::to_string(&user).unwrap();
        let deserialized: User = serde_json::from_str(&json).unwrap();
        assert!(deserialized.last_login_at.is_some());
    }

    #[test]
    fn test_user_with_permissions() {
        let user = create_test_user();
        let user_with_perms = UserWithPermissions {
            user,
            roles: vec!["admin".to_string(), "user".to_string()],
            permissions: vec!["read".to_string(), "write".to_string()],
        };

        let json = serde_json::to_string(&user_with_perms).unwrap();
        assert!(json.contains("admin"));
        assert!(json.contains("read"));

        let deserialized: UserWithPermissions = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.roles.len(), 2);
        assert_eq!(deserialized.permissions.len(), 2);
    }

    #[test]
    fn test_user_with_permissions_empty() {
        let user = create_test_user();
        let user_with_perms = UserWithPermissions {
            user,
            roles: vec![],
            permissions: vec![],
        };

        let json = serde_json::to_string(&user_with_perms).unwrap();
        let deserialized: UserWithPermissions = serde_json::from_str(&json).unwrap();
        assert!(deserialized.roles.is_empty());
        assert!(deserialized.permissions.is_empty());
    }

    #[test]
    fn test_create_user_request() {
        let request = CreateUserRequest {
            email: "new@example.com".to_string(),
            password: "secret123".to_string(),
            display_name: Some("New User".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("new@example.com"));
        assert!(json.contains("secret123"));
    }

    #[test]
    fn test_create_user_request_without_display_name() {
        let request = CreateUserRequest {
            email: "new@example.com".to_string(),
            password: "secret123".to_string(),
            display_name: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: CreateUserRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.display_name.is_none());
    }

    #[test]
    fn test_update_user_request_partial() {
        let request = UpdateUserRequest {
            display_name: Some("Updated Name".to_string()),
            is_active: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateUserRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.display_name, Some("Updated Name".to_string()));
        assert!(deserialized.is_active.is_none());
    }

    #[test]
    fn test_update_user_request_all_fields() {
        let request = UpdateUserRequest {
            display_name: Some("Updated Name".to_string()),
            is_active: Some(false),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateUserRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.is_active, Some(false));
    }

    #[test]
    fn test_user_clone() {
        let user = create_test_user();
        let cloned = user.clone();
        assert_eq!(user.id, cloned.id);
        assert_eq!(user.email, cloned.email);
    }

    #[test]
    fn test_user_debug() {
        let user = create_test_user();
        let debug_str = format!("{:?}", user);
        assert!(debug_str.contains("User"));
        assert!(debug_str.contains("test@example.com"));
    }
}
