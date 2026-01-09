//! Source types (content sources for the agent)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A content source (files, calendar, mail, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub name: String,
    pub source_type: SourceType,
    pub category: SourceCategory,
    pub config: serde_json::Value,
    pub is_active: bool,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Type of source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    // File sources
    Filesystem,
    GitHub,
    GitLab,
    // Content sources
    GoogleCalendar,
    GoogleMail,
    Notion,
    Slack,
    Web,
    // Text sources
    Text,
}

/// Category of source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCategory {
    File,
    Calendar,
    Mail,
    Document,
    Communication,
    Web,
    Text,
}

impl std::str::FromStr for SourceCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "file" => Ok(SourceCategory::File),
            "calendar" => Ok(SourceCategory::Calendar),
            "mail" => Ok(SourceCategory::Mail),
            "document" => Ok(SourceCategory::Document),
            "communication" => Ok(SourceCategory::Communication),
            "web" => Ok(SourceCategory::Web),
            "text" => Ok(SourceCategory::Text),
            _ => Err(format!("Unknown category: {}", s)),
        }
    }
}

impl SourceType {
    /// Get the category for this source type
    pub fn category(&self) -> SourceCategory {
        match self {
            SourceType::Filesystem | SourceType::GitHub | SourceType::GitLab => {
                SourceCategory::File
            }
            SourceType::GoogleCalendar => SourceCategory::Calendar,
            SourceType::GoogleMail => SourceCategory::Mail,
            SourceType::Notion => SourceCategory::Document,
            SourceType::Slack => SourceCategory::Communication,
            SourceType::Web => SourceCategory::Web,
            SourceType::Text => SourceCategory::Text,
        }
    }
}

/// Request to create a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSourceRequest {
    pub name: String,
    pub source_type: SourceType,
    pub config: serde_json::Value,
}

/// Request to update a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSourceRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub is_active: Option<bool>,
}

/// Source type info (for listing available types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTypeInfo {
    pub source_type: SourceType,
    pub category: SourceCategory,
    pub name: String,
    pub description: String,
    pub config_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_source() -> Source {
        Source {
            id: Uuid::new_v4(),
            name: "Test Source".to_string(),
            source_type: SourceType::Filesystem,
            category: SourceCategory::File,
            config: json!({"path": "/home/user/docs"}),
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_source_type_category_filesystem() {
        assert_eq!(SourceType::Filesystem.category(), SourceCategory::File);
    }

    #[test]
    fn test_source_type_category_github() {
        assert_eq!(SourceType::GitHub.category(), SourceCategory::File);
    }

    #[test]
    fn test_source_type_category_gitlab() {
        assert_eq!(SourceType::GitLab.category(), SourceCategory::File);
    }

    #[test]
    fn test_source_type_category_calendar() {
        assert_eq!(
            SourceType::GoogleCalendar.category(),
            SourceCategory::Calendar
        );
    }

    #[test]
    fn test_source_type_category_mail() {
        assert_eq!(SourceType::GoogleMail.category(), SourceCategory::Mail);
    }

    #[test]
    fn test_source_type_category_notion() {
        assert_eq!(SourceType::Notion.category(), SourceCategory::Document);
    }

    #[test]
    fn test_source_type_category_slack() {
        assert_eq!(SourceType::Slack.category(), SourceCategory::Communication);
    }

    #[test]
    fn test_source_type_category_web() {
        assert_eq!(SourceType::Web.category(), SourceCategory::Web);
    }

    #[test]
    fn test_source_type_category_text() {
        assert_eq!(SourceType::Text.category(), SourceCategory::Text);
    }

    #[test]
    fn test_source_type_serialization() {
        assert_eq!(
            serde_json::to_string(&SourceType::Filesystem).unwrap(),
            "\"filesystem\""
        );
        assert_eq!(
            serde_json::to_string(&SourceType::GitHub).unwrap(),
            "\"git_hub\""
        );
        assert_eq!(
            serde_json::to_string(&SourceType::GoogleCalendar).unwrap(),
            "\"google_calendar\""
        );
        assert_eq!(
            serde_json::to_string(&SourceType::Slack).unwrap(),
            "\"slack\""
        );
    }

    #[test]
    fn test_source_category_serialization() {
        assert_eq!(
            serde_json::to_string(&SourceCategory::File).unwrap(),
            "\"file\""
        );
        assert_eq!(
            serde_json::to_string(&SourceCategory::Calendar).unwrap(),
            "\"calendar\""
        );
        assert_eq!(
            serde_json::to_string(&SourceCategory::Mail).unwrap(),
            "\"mail\""
        );
        assert_eq!(
            serde_json::to_string(&SourceCategory::Communication).unwrap(),
            "\"communication\""
        );
    }

    #[test]
    fn test_source_serialization() {
        let source = create_test_source();
        let json = serde_json::to_string(&source).unwrap();

        assert!(json.contains("Test Source"));
        assert!(json.contains("filesystem"));

        let deserialized: Source = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, source.name);
    }

    #[test]
    fn test_source_with_last_sync() {
        let mut source = create_test_source();
        source.last_synced_at = Some(Utc::now());

        let json = serde_json::to_string(&source).unwrap();
        let deserialized: Source = serde_json::from_str(&json).unwrap();
        assert!(deserialized.last_synced_at.is_some());
    }

    #[test]
    fn test_create_source_request() {
        let request = CreateSourceRequest {
            name: "New Source".to_string(),
            source_type: SourceType::GitHub,
            config: json!({"repo": "owner/repo", "branch": "main"}),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Source"));
        assert!(json.contains("git_hub"));
    }

    #[test]
    fn test_update_source_request_partial() {
        let request = UpdateSourceRequest {
            name: Some("Updated Name".to_string()),
            config: None,
            is_active: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: UpdateSourceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, Some("Updated Name".to_string()));
        assert!(deserialized.config.is_none());
    }

    #[test]
    fn test_update_source_request_all_fields() {
        let request = UpdateSourceRequest {
            name: Some("Updated".to_string()),
            config: Some(json!({"new_key": "new_value"})),
            is_active: Some(false),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("new_key"));
        assert!(json.contains("false"));
    }

    #[test]
    fn test_source_type_info() {
        let info = SourceTypeInfo {
            source_type: SourceType::GitHub,
            category: SourceCategory::File,
            name: "GitHub".to_string(),
            description: "GitHub repositories".to_string(),
            config_schema: json!({
                "type": "object",
                "properties": {
                    "repo": {"type": "string"},
                    "branch": {"type": "string"}
                }
            }),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("GitHub"));
        assert!(json.contains("GitHub repositories"));
        assert!(json.contains("properties"));
    }

    #[test]
    fn test_source_type_equality() {
        assert_eq!(SourceType::GitHub, SourceType::GitHub);
        assert_ne!(SourceType::GitHub, SourceType::GitLab);
        assert_eq!(SourceCategory::File, SourceCategory::File);
        assert_ne!(SourceCategory::File, SourceCategory::Mail);
    }

    #[test]
    fn test_source_type_copy() {
        let source_type = SourceType::Slack;
        let copied = source_type;
        assert_eq!(source_type, copied);

        let category = SourceCategory::Communication;
        let copied_cat = category;
        assert_eq!(category, copied_cat);
    }

    #[test]
    fn test_source_config_with_complex_json() {
        let source = Source {
            id: Uuid::new_v4(),
            name: "Complex Source".to_string(),
            source_type: SourceType::Web,
            category: SourceCategory::Web,
            config: json!({
                "urls": ["https://example.com", "https://test.com"],
                "crawl_depth": 3,
                "options": {
                    "follow_redirects": true,
                    "timeout": 30
                }
            }),
            is_active: true,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&source).unwrap();
        let deserialized: Source = serde_json::from_str(&json).unwrap();

        assert!(deserialized.config.get("urls").is_some());
        assert_eq!(deserialized.config["crawl_depth"], 3);
    }
}
