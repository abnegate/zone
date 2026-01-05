//! Task types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A task to be executed by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

/// Request to create a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub project_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub priority: Option<TaskPriority>,
}

/// Request to update a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
}

/// A single execution run of a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRun {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: TaskRunStatus,
    pub model: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_message: Option<String>,
    pub iterations: i32,
    pub modified_files: Vec<String>,
}

/// Task run status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunStatus {
    #[default]
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// A log entry for a task run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunLog {
    pub id: Uuid,
    pub task_run_id: Uuid,
    pub phase: String,
    pub agent_type: Option<String>,
    pub log_level: LogLevel,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

/// Log level for task run logs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_task() -> Task {
        Task {
            id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            acceptance_criteria: Some("It should work".to_string()),
            status: TaskStatus::Pending,
            priority: TaskPriority::Medium,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_task_status_default() {
        assert_eq!(TaskStatus::default(), TaskStatus::Pending);
    }

    #[test]
    fn test_task_priority_default() {
        assert_eq!(TaskPriority::default(), TaskPriority::Medium);
    }

    #[test]
    fn test_task_run_status_default() {
        assert_eq!(TaskRunStatus::default(), TaskRunStatus::Running);
    }

    #[test]
    fn test_log_level_default() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }

    #[test]
    fn test_task_status_serialization() {
        assert_eq!(
            serde_json::to_string(&TaskStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn test_task_priority_serialization() {
        assert_eq!(
            serde_json::to_string(&TaskPriority::Low).unwrap(),
            "\"low\""
        );
        assert_eq!(
            serde_json::to_string(&TaskPriority::Medium).unwrap(),
            "\"medium\""
        );
        assert_eq!(
            serde_json::to_string(&TaskPriority::High).unwrap(),
            "\"high\""
        );
        assert_eq!(
            serde_json::to_string(&TaskPriority::Critical).unwrap(),
            "\"critical\""
        );
    }

    #[test]
    fn test_task_run_status_serialization() {
        assert_eq!(
            serde_json::to_string(&TaskRunStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&TaskRunStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskRunStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskRunStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn test_log_level_serialization() {
        assert_eq!(
            serde_json::to_string(&LogLevel::Debug).unwrap(),
            "\"debug\""
        );
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            "\"error\""
        );
    }

    #[test]
    fn test_task_serialization() {
        let task = create_test_task();
        let json = serde_json::to_string(&task).unwrap();

        assert!(json.contains("Test Task"));
        assert!(json.contains("pending"));
        assert!(json.contains("medium"));

        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, task.title);
    }

    #[test]
    fn test_task_without_project() {
        let mut task = create_test_task();
        task.project_id = None;

        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert!(deserialized.project_id.is_none());
    }

    #[test]
    fn test_create_task_request() {
        let request = CreateTaskRequest {
            project_id: Some(Uuid::new_v4()),
            title: "New Task".to_string(),
            description: Some("Description".to_string()),
            acceptance_criteria: Some("Criteria".to_string()),
            priority: Some(TaskPriority::High),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("New Task"));
        assert!(json.contains("high"));
    }

    #[test]
    fn test_create_task_request_minimal() {
        let request = CreateTaskRequest {
            project_id: None,
            title: "Minimal Task".to_string(),
            description: None,
            acceptance_criteria: None,
            priority: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: CreateTaskRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.priority.is_none());
    }

    #[test]
    fn test_update_task_request() {
        let request = UpdateTaskRequest {
            title: Some("Updated Title".to_string()),
            description: None,
            acceptance_criteria: None,
            status: Some(TaskStatus::Running),
            priority: Some(TaskPriority::Critical),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Updated Title"));
        assert!(json.contains("running"));
        assert!(json.contains("critical"));
    }

    #[test]
    fn test_task_run() {
        let task_run = TaskRun {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            status: TaskRunStatus::Running,
            model: "gpt-4".to_string(),
            started_at: Utc::now(),
            completed_at: None,
            result_message: None,
            iterations: 0,
            modified_files: vec![],
        };

        let json = serde_json::to_string(&task_run).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("running"));
    }

    #[test]
    fn test_task_run_completed() {
        let task_run = TaskRun {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            status: TaskRunStatus::Completed,
            model: "gpt-4".to_string(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            result_message: Some("Task completed successfully".to_string()),
            iterations: 5,
            modified_files: vec!["file1.rs".to_string(), "file2.rs".to_string()],
        };

        let json = serde_json::to_string(&task_run).unwrap();
        assert!(json.contains("completed"));
        assert!(json.contains("Task completed successfully"));
        assert!(json.contains("file1.rs"));
    }

    #[test]
    fn test_task_run_log() {
        let log = TaskRunLog {
            id: Uuid::new_v4(),
            task_run_id: Uuid::new_v4(),
            phase: "execution".to_string(),
            agent_type: Some("code_agent".to_string()),
            log_level: LogLevel::Info,
            message: "Processing task...".to_string(),
            created_at: Utc::now(),
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("execution"));
        assert!(json.contains("code_agent"));
        assert!(json.contains("Processing task..."));
    }

    #[test]
    fn test_task_run_log_without_agent_type() {
        let log = TaskRunLog {
            id: Uuid::new_v4(),
            task_run_id: Uuid::new_v4(),
            phase: "init".to_string(),
            agent_type: None,
            log_level: LogLevel::Debug,
            message: "Starting...".to_string(),
            created_at: Utc::now(),
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: TaskRunLog = serde_json::from_str(&json).unwrap();
        assert!(deserialized.agent_type.is_none());
    }

    #[test]
    fn test_status_equality() {
        assert_eq!(TaskStatus::Pending, TaskStatus::Pending);
        assert_ne!(TaskStatus::Pending, TaskStatus::Running);
        assert_eq!(TaskPriority::High, TaskPriority::High);
        assert_ne!(TaskPriority::High, TaskPriority::Low);
    }

    #[test]
    fn test_status_copy() {
        let status = TaskStatus::Running;
        let copied = status;
        assert_eq!(status, copied);

        let priority = TaskPriority::Critical;
        let copied_priority = priority;
        assert_eq!(priority, copied_priority);
    }
}
