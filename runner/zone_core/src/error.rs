//! Core error types

use thiserror::Error;

/// Core error type for Zone
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Source error: {0}")]
    Source(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Agent iteration limit exceeded: {0} iterations")]
    IterationLimit(u32),

    #[error("Agent cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_error_display() {
        let err = CoreError::Llm("Connection refused".to_string());
        assert_eq!(err.to_string(), "LLM error: Connection refused");
    }

    #[test]
    fn test_tool_error_display() {
        let err = CoreError::Tool("File not found".to_string());
        assert_eq!(err.to_string(), "Tool error: File not found");
    }

    #[test]
    fn test_source_error_display() {
        let err = CoreError::Source("Invalid source config".to_string());
        assert_eq!(err.to_string(), "Source error: Invalid source config");
    }

    #[test]
    fn test_session_error_display() {
        let err = CoreError::Session("Session expired".to_string());
        assert_eq!(err.to_string(), "Session error: Session expired");
    }

    #[test]
    fn test_serialization_error_from_serde() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: CoreError = json_err.into();
        assert!(matches!(err, CoreError::Serialization(_)));
        assert!(err.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_io_error_from_std() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let err: CoreError = io_err.into();
        assert!(matches!(err, CoreError::Io(_)));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn test_iteration_limit_error() {
        let err = CoreError::IterationLimit(100);
        assert_eq!(
            err.to_string(),
            "Agent iteration limit exceeded: 100 iterations"
        );
    }

    #[test]
    fn test_cancelled_error() {
        let err = CoreError::Cancelled;
        assert_eq!(err.to_string(), "Agent cancelled");
    }

    #[test]
    fn test_other_error() {
        let err = CoreError::Other("Something went wrong".to_string());
        assert_eq!(err.to_string(), "Something went wrong");
    }

    #[test]
    fn test_result_type_ok() {
        fn get_result() -> Result<i32> {
            Ok(42)
        }
        let result = get_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_result_type_err() {
        let result: Result<i32> = Err(CoreError::Cancelled);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_debug() {
        let err = CoreError::Llm("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Llm"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        // CoreError should be Send + Sync for use across threads
        assert_send::<CoreError>();
        assert_sync::<CoreError>();
    }
}
