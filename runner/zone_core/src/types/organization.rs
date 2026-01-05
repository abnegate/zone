//! Organization and Workspace types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An organization (top-level tenant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create an organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrganizationRequest {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

/// Request to update an organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOrganizationRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

/// A workspace within an organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
}

/// Request to update a workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

/// Workspace theme settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTheme {
    pub workspace_id: Uuid,
    pub primary_color: Option<String>,
    pub logo_url: Option<String>,
    pub custom_css: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_organization() -> Organization {
        Organization {
            id: Uuid::new_v4(),
            name: "Test Org".to_string(),
            slug: "test-org".to_string(),
            description: Some("A test organization".to_string()),
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_test_workspace(org_id: Uuid) -> Workspace {
        Workspace {
            id: Uuid::new_v4(),
            organization_id: org_id,
            name: "Test Workspace".to_string(),
            slug: "test-workspace".to_string(),
            description: Some("A test workspace".to_string()),
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_organization_serialization() {
        let org = create_test_organization();
        let json = serde_json::to_string(&org).unwrap();

        assert!(json.contains("Test Org"));
        assert!(json.contains("test-org"));

        let deserialized: Organization = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, org.name);
        assert_eq!(deserialized.slug, org.slug);
    }

    #[test]
    fn test_organization_without_description() {
        let mut org = create_test_organization();
        org.description = None;

        let json = serde_json::to_string(&org).unwrap();
        let deserialized: Organization = serde_json::from_str(&json).unwrap();
        assert!(deserialized.description.is_none());
    }

    #[test]
    fn test_organization_inactive() {
        let mut org = create_test_organization();
        org.is_active = false;

        let json = serde_json::to_string(&org).unwrap();
        let deserialized: Organization = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.is_active);
    }

    #[test]
    fn test_create_organization_request() {
        let request = CreateOrganizationRequest {
            name: "New Org".to_string(),
            slug: "new-org".to_string(),
            description: Some("New organization".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Org"));
        assert!(json.contains("new-org"));
    }

    #[test]
    fn test_create_organization_request_minimal() {
        let request = CreateOrganizationRequest {
            name: "Minimal".to_string(),
            slug: "minimal".to_string(),
            description: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: CreateOrganizationRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.description.is_none());
    }

    #[test]
    fn test_update_organization_request_partial() {
        let request = UpdateOrganizationRequest {
            name: Some("Updated Name".to_string()),
            description: None,
            is_active: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateOrganizationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, Some("Updated Name".to_string()));
    }

    #[test]
    fn test_update_organization_request_all_fields() {
        let request = UpdateOrganizationRequest {
            name: Some("Updated".to_string()),
            description: Some("New description".to_string()),
            is_active: Some(false),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Updated"));
        assert!(json.contains("New description"));
        assert!(json.contains("false"));
    }

    #[test]
    fn test_workspace_serialization() {
        let org_id = Uuid::new_v4();
        let workspace = create_test_workspace(org_id);
        let json = serde_json::to_string(&workspace).unwrap();

        assert!(json.contains("Test Workspace"));
        assert!(json.contains("test-workspace"));
        assert!(json.contains(&org_id.to_string()));

        let deserialized: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, workspace.name);
        assert_eq!(deserialized.organization_id, org_id);
    }

    #[test]
    fn test_workspace_without_description() {
        let mut workspace = create_test_workspace(Uuid::new_v4());
        workspace.description = None;

        let json = serde_json::to_string(&workspace).unwrap();
        let deserialized: Workspace = serde_json::from_str(&json).unwrap();
        assert!(deserialized.description.is_none());
    }

    #[test]
    fn test_create_workspace_request() {
        let request = CreateWorkspaceRequest {
            name: "New Workspace".to_string(),
            slug: "new-workspace".to_string(),
            description: Some("Description".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Workspace"));
    }

    #[test]
    fn test_update_workspace_request() {
        let request = UpdateWorkspaceRequest {
            name: Some("Updated".to_string()),
            description: None,
            is_active: Some(true),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateWorkspaceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.is_active, Some(true));
    }

    #[test]
    fn test_workspace_theme() {
        let theme = WorkspaceTheme {
            workspace_id: Uuid::new_v4(),
            primary_color: Some("#007bff".to_string()),
            logo_url: Some("https://example.com/logo.png".to_string()),
            custom_css: Some(".header { color: blue; }".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&theme).unwrap();
        assert!(json.contains("#007bff"));
        assert!(json.contains("logo.png"));
        assert!(json.contains(".header"));
    }

    #[test]
    fn test_workspace_theme_minimal() {
        let theme = WorkspaceTheme {
            workspace_id: Uuid::new_v4(),
            primary_color: None,
            logo_url: None,
            custom_css: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&theme).unwrap();
        let deserialized: WorkspaceTheme = serde_json::from_str(&json).unwrap();
        assert!(deserialized.primary_color.is_none());
        assert!(deserialized.logo_url.is_none());
        assert!(deserialized.custom_css.is_none());
    }

    #[test]
    fn test_organization_clone() {
        let org = create_test_organization();
        let cloned = org.clone();
        assert_eq!(org.id, cloned.id);
        assert_eq!(org.name, cloned.name);
    }

    #[test]
    fn test_workspace_clone() {
        let workspace = create_test_workspace(Uuid::new_v4());
        let cloned = workspace.clone();
        assert_eq!(workspace.id, cloned.id);
        assert_eq!(workspace.name, cloned.name);
    }
}
