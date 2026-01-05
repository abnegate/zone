//! Project types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A project containing tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub github_repo: Option<String>,
    pub github_branch: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Project status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
    Completed,
}

/// Request to create a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub workspace_id: Option<Uuid>,
}

/// Request to update a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ProjectStatus>,
}

/// Request to link a GitHub repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkGitHubRequest {
    pub repo: String,
    pub branch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_project() -> Project {
        Project {
            id: Uuid::new_v4(),
            workspace_id: Some(Uuid::new_v4()),
            name: "Test Project".to_string(),
            description: Some("A test project".to_string()),
            status: ProjectStatus::Active,
            github_repo: None,
            github_branch: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_project_status_default() {
        assert_eq!(ProjectStatus::default(), ProjectStatus::Active);
    }

    #[test]
    fn test_project_status_serialization() {
        assert_eq!(
            serde_json::to_string(&ProjectStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&ProjectStatus::Archived).unwrap(),
            "\"archived\""
        );
        assert_eq!(
            serde_json::to_string(&ProjectStatus::Completed).unwrap(),
            "\"completed\""
        );
    }

    #[test]
    fn test_project_status_deserialization() {
        let active: ProjectStatus = serde_json::from_str("\"active\"").unwrap();
        assert_eq!(active, ProjectStatus::Active);

        let archived: ProjectStatus = serde_json::from_str("\"archived\"").unwrap();
        assert_eq!(archived, ProjectStatus::Archived);
    }

    #[test]
    fn test_project_status_equality() {
        assert_eq!(ProjectStatus::Active, ProjectStatus::Active);
        assert_ne!(ProjectStatus::Active, ProjectStatus::Archived);
    }

    #[test]
    fn test_project_serialization() {
        let project = create_test_project();
        let json = serde_json::to_string(&project).unwrap();

        assert!(json.contains("Test Project"));
        assert!(json.contains("active"));

        let deserialized: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, project.name);
    }

    #[test]
    fn test_project_without_workspace() {
        let mut project = create_test_project();
        project.workspace_id = None;

        let json = serde_json::to_string(&project).unwrap();
        let deserialized: Project = serde_json::from_str(&json).unwrap();
        assert!(deserialized.workspace_id.is_none());
    }

    #[test]
    fn test_project_with_github() {
        let mut project = create_test_project();
        project.github_repo = Some("owner/repo".to_string());
        project.github_branch = Some("main".to_string());

        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains("owner/repo"));
        assert!(json.contains("main"));
    }

    #[test]
    fn test_create_project_request() {
        let request = CreateProjectRequest {
            name: "New Project".to_string(),
            description: Some("Description".to_string()),
            workspace_id: Some(Uuid::new_v4()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Project"));
    }

    #[test]
    fn test_create_project_request_minimal() {
        let request = CreateProjectRequest {
            name: "Minimal".to_string(),
            description: None,
            workspace_id: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: CreateProjectRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.description.is_none());
        assert!(deserialized.workspace_id.is_none());
    }

    #[test]
    fn test_update_project_request_partial() {
        let request = UpdateProjectRequest {
            name: Some("Updated".to_string()),
            description: None,
            status: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateProjectRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, Some("Updated".to_string()));
    }

    #[test]
    fn test_update_project_request_with_status() {
        let request = UpdateProjectRequest {
            name: None,
            description: None,
            status: Some(ProjectStatus::Completed),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("completed"));
    }

    #[test]
    fn test_link_github_request() {
        let request = LinkGitHubRequest {
            repo: "owner/repo".to_string(),
            branch: Some("develop".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("owner/repo"));
        assert!(json.contains("develop"));
    }

    #[test]
    fn test_link_github_request_default_branch() {
        let request = LinkGitHubRequest {
            repo: "owner/repo".to_string(),
            branch: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: LinkGitHubRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.branch.is_none());
    }

    #[test]
    fn test_project_clone() {
        let project = create_test_project();
        let cloned = project.clone();
        assert_eq!(project.id, cloned.id);
        assert_eq!(project.name, cloned.name);
    }

    #[test]
    fn test_project_status_copy() {
        let status = ProjectStatus::Active;
        let copied = status;
        assert_eq!(status, copied);
    }
}
