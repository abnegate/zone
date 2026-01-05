//! Session management
//!
//! Provides session storage and retrieval for agent executions.

mod file_store;

pub use file_store::FileSessionStore;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::agent::AgentState;

/// Session storage error
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(Uuid),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A stored session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session ID
    pub id: Uuid,
    /// Title/summary of the session
    pub title: String,
    /// The agent state
    pub state: AgentState,
    /// When the session was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the session was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Project directory this session is associated with
    pub project_dir: Option<String>,
}

impl Session {
    /// Create a new session from an agent state
    pub fn new(state: AgentState, title: impl Into<String>, project_dir: Option<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: state.id,
            title: title.into(),
            state,
            created_at: now,
            updated_at: now,
            project_dir,
        }
    }

    /// Update the session with a new state
    pub fn update(&mut self, state: AgentState) {
        self.state = state;
        self.updated_at = chrono::Utc::now();
    }
}

/// Session metadata for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub project_dir: Option<String>,
    pub finished: bool,
}

impl From<&Session> for SessionSummary {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id,
            title: session.title.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
            project_dir: session.project_dir.clone(),
            finished: session.state.finished,
        }
    }
}

/// Trait for session storage backends
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Save a session
    async fn save(&self, session: &Session) -> Result<(), SessionError>;

    /// Load a session by ID
    async fn load(&self, id: Uuid) -> Result<Session, SessionError>;

    /// Delete a session
    async fn delete(&self, id: Uuid) -> Result<(), SessionError>;

    /// List all sessions
    async fn list(&self) -> Result<Vec<SessionSummary>, SessionError>;

    /// Get the most recent session
    async fn most_recent(&self) -> Result<Option<Session>, SessionError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentState;

    #[test]
    fn test_session_new() {
        let state = AgentState::new("Test prompt", None);
        let session = Session::new(
            state.clone(),
            "Test Session",
            Some("/tmp/project".to_string()),
        );

        assert_eq!(session.id, state.id);
        assert_eq!(session.title, "Test Session");
        assert_eq!(session.project_dir, Some("/tmp/project".to_string()));
        assert!(!session.state.finished);
    }

    #[test]
    fn test_session_update() {
        let state = AgentState::new("Test prompt", None);
        let mut session = Session::new(state.clone(), "Test Session", None);

        let initial_updated = session.updated_at;

        // Wait a tiny bit to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        let mut new_state = AgentState::new("Updated prompt", None);
        new_state.complete("Done!");
        session.update(new_state);

        assert!(session.state.finished);
        assert!(session.updated_at > initial_updated);
    }

    #[test]
    fn test_session_summary_from_session() {
        let state = AgentState::new("Test prompt", None);
        let session = Session::new(state, "My Session", Some("/project".to_string()));

        let summary = SessionSummary::from(&session);

        assert_eq!(summary.id, session.id);
        assert_eq!(summary.title, "My Session");
        assert_eq!(summary.project_dir, Some("/project".to_string()));
        assert!(!summary.finished);
    }

    #[test]
    fn test_session_summary_finished() {
        let mut state = AgentState::new("Test prompt", None);
        state.complete("All done");

        let session = Session::new(state, "Finished Session", None);
        let summary = SessionSummary::from(&session);

        assert!(summary.finished);
    }

    #[test]
    fn test_session_serialization() {
        let state = AgentState::new("Test prompt", None);
        let session = Session::new(state, "Test Session", None);

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("Test Session"));
        assert!(json.contains("Test prompt"));

        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.title, session.title);
    }

    #[test]
    fn test_session_summary_serialization() {
        let state = AgentState::new("Test prompt", None);
        let session = Session::new(state, "Test Session", Some("/project".to_string()));
        let summary = SessionSummary::from(&session);

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("Test Session"));
        assert!(json.contains("/project"));

        let deserialized: SessionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, summary.id);
    }

    // ==================== Session Edge Cases ====================

    #[test]
    fn test_session_new_without_project_dir() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state.clone(), "No Project", None);

        assert_eq!(session.id, state.id);
        assert_eq!(session.title, "No Project");
        assert!(session.project_dir.is_none());
    }

    #[test]
    fn test_session_timestamps_on_creation() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Timestamp Test", None);

        // created_at and updated_at should be equal on creation
        assert_eq!(session.created_at, session.updated_at);
    }

    #[test]
    fn test_session_update_changes_timestamp() {
        let state = AgentState::new("Test", None);
        let mut session = Session::new(state, "Update Test", None);
        let original_updated = session.updated_at;

        // Wait to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        let new_state = AgentState::new("Updated", None);
        session.update(new_state);

        assert!(session.updated_at > original_updated);
        // created_at should not change
        assert!(session.created_at < session.updated_at);
    }

    #[test]
    fn test_session_update_preserves_id() {
        let state = AgentState::new("Test", None);
        let mut session = Session::new(state.clone(), "ID Test", None);
        let original_id = session.id;

        let new_state = AgentState::new("New", None);
        session.update(new_state.clone());

        // Session ID should remain the same (the original state's ID)
        assert_eq!(session.id, original_id);
        // But the internal state ID changes
        assert_eq!(session.state.id, new_state.id);
    }

    #[test]
    fn test_session_with_empty_title() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "", None);

        assert_eq!(session.title, "");
    }

    #[test]
    fn test_session_with_long_title() {
        let state = AgentState::new("Test", None);
        let long_title = "a".repeat(1000);
        let session = Session::new(state, long_title.clone(), None);

        assert_eq!(session.title, long_title);
    }

    #[test]
    fn test_session_with_unicode_title() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Session: Testing Unicode", None);

        assert_eq!(session.title, "Session: Testing Unicode");
    }

    #[test]
    fn test_session_clone() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Clone Test", Some("/path".to_string()));
        let cloned = session.clone();

        assert_eq!(cloned.id, session.id);
        assert_eq!(cloned.title, session.title);
        assert_eq!(cloned.project_dir, session.project_dir);
        assert_eq!(cloned.created_at, session.created_at);
    }

    #[test]
    fn test_session_serialization_roundtrip_full() {
        let mut state = AgentState::new("Test prompt", Some("System".to_string()));
        state.complete("Done");

        let session = Session::new(state, "Full Roundtrip", Some("/full/path".to_string()));

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.title, session.title);
        assert_eq!(deserialized.project_dir, session.project_dir);
        assert!(deserialized.state.finished);
        assert_eq!(deserialized.state.final_response, Some("Done".to_string()));
    }

    #[test]
    fn test_session_debug() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Debug Test", None);
        let debug_str = format!("{:?}", session);

        assert!(debug_str.contains("Session"));
        assert!(debug_str.contains("Debug Test"));
    }

    // ==================== SessionSummary Edge Cases ====================

    #[test]
    fn test_session_summary_without_project() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "No Project", None);
        let summary = SessionSummary::from(&session);

        assert!(summary.project_dir.is_none());
    }

    #[test]
    fn test_session_summary_preserves_timestamps() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Timestamp", None);
        let summary = SessionSummary::from(&session);

        assert_eq!(summary.created_at, session.created_at);
        assert_eq!(summary.updated_at, session.updated_at);
    }

    #[test]
    fn test_session_summary_clone() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Clone", None);
        let summary = SessionSummary::from(&session);
        let cloned = summary.clone();

        assert_eq!(cloned.id, summary.id);
        assert_eq!(cloned.title, summary.title);
        assert_eq!(cloned.finished, summary.finished);
    }

    #[test]
    fn test_session_summary_debug() {
        let state = AgentState::new("Test", None);
        let session = Session::new(state, "Debug", None);
        let summary = SessionSummary::from(&session);
        let debug_str = format!("{:?}", summary);

        assert!(debug_str.contains("SessionSummary"));
        assert!(debug_str.contains("Debug"));
    }

    #[test]
    fn test_session_summary_serialization_roundtrip() {
        let mut state = AgentState::new("Test", None);
        state.complete("Done");

        let session = Session::new(state, "Roundtrip", Some("/path".to_string()));
        let summary = SessionSummary::from(&session);

        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: SessionSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, summary.id);
        assert_eq!(deserialized.title, summary.title);
        assert_eq!(deserialized.project_dir, summary.project_dir);
        assert_eq!(deserialized.finished, summary.finished);
    }

    // ==================== SessionError Tests ====================

    #[test]
    fn test_session_error_not_found_display() {
        let id = uuid::Uuid::new_v4();
        let error = SessionError::NotFound(id);
        let display = format!("{}", error);

        assert!(display.contains("Session not found"));
        assert!(display.contains(&id.to_string()));
    }

    #[test]
    fn test_session_error_io_from() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let session_error: SessionError = io_error.into();
        let display = format!("{}", session_error);

        assert!(display.contains("IO error"));
    }

    #[test]
    fn test_session_error_serialization_from() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let session_error: SessionError = json_error.into();
        let display = format!("{}", session_error);

        assert!(display.contains("Serialization error"));
    }

    #[test]
    fn test_session_error_debug() {
        let id = uuid::Uuid::new_v4();
        let error = SessionError::NotFound(id);
        let debug_str = format!("{:?}", error);

        assert!(debug_str.contains("NotFound"));
    }

    // ==================== Session State Interaction Tests ====================

    #[test]
    fn test_session_with_completed_state() {
        let mut state = AgentState::new("Test", None);
        state.complete("Final response");

        let session = Session::new(state, "Completed", None);

        assert!(session.state.finished);
        assert_eq!(
            session.state.final_response,
            Some("Final response".to_string())
        );

        let summary = SessionSummary::from(&session);
        assert!(summary.finished);
    }

    #[test]
    fn test_session_with_failed_state() {
        let mut state = AgentState::new("Test", None);
        state.fail("Error message");

        let session = Session::new(state, "Failed", None);

        assert!(session.state.finished);
        assert_eq!(session.state.error, Some("Error message".to_string()));

        let summary = SessionSummary::from(&session);
        assert!(summary.finished);
    }

    #[test]
    fn test_session_update_to_completed() {
        let state = AgentState::new("Test", None);
        let mut session = Session::new(state, "To Complete", None);

        let summary_before = SessionSummary::from(&session);
        assert!(!summary_before.finished);

        let mut new_state = AgentState::new("Updated", None);
        new_state.complete("Done");
        session.update(new_state);

        let summary_after = SessionSummary::from(&session);
        assert!(summary_after.finished);
    }

    #[test]
    fn test_session_multiple_updates() {
        let state = AgentState::new("Initial", None);
        let mut session = Session::new(state, "Multiple Updates", None);

        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let new_state = AgentState::new(format!("Update {}", i), None);
            session.update(new_state);
        }

        // After multiple updates, timestamps should reflect latest
        assert!(session.updated_at > session.created_at);
    }

    #[test]
    fn test_session_with_special_path_characters() {
        let state = AgentState::new("Test", None);
        let session = Session::new(
            state,
            "Special Path",
            Some("/path/with spaces/and-dashes/under_scores".to_string()),
        );

        assert_eq!(
            session.project_dir,
            Some("/path/with spaces/and-dashes/under_scores".to_string())
        );

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.project_dir, session.project_dir);
    }

    #[test]
    fn test_session_id_matches_state_id() {
        let state = AgentState::new("Test", None);
        let state_id = state.id;
        let session = Session::new(state, "ID Match", None);

        assert_eq!(session.id, state_id);
    }
}
