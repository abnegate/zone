//! Server error types

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Server error type
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Core error: {0}")]
    Core(#[from] zone_core::CoreError),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ServerError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ServerError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ServerError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            ServerError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ServerError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ServerError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
            }
            ServerError::Redis(e) => {
                tracing::error!("Redis error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Cache error".to_string())
            }
            ServerError::Json(e) => (StatusCode::BAD_REQUEST, format!("JSON error: {}", e)),
            ServerError::Core(e) => {
                tracing::error!("Core error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error".to_string(),
                )
            }
        };

        let body = Json(json!({
            "success": false,
            "error": message,
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ServerError>;

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    // ============ Error display tests ============

    #[test]
    fn test_not_found_display() {
        let err = ServerError::NotFound("User not found".to_string());
        assert_eq!(err.to_string(), "Not found: User not found");
    }

    #[test]
    fn test_bad_request_display() {
        let err = ServerError::BadRequest("Invalid input".to_string());
        assert_eq!(err.to_string(), "Bad request: Invalid input");
    }

    #[test]
    fn test_unauthorized_display() {
        let err = ServerError::Unauthorized("Invalid token".to_string());
        assert_eq!(err.to_string(), "Unauthorized: Invalid token");
    }

    #[test]
    fn test_forbidden_display() {
        let err = ServerError::Forbidden("Access denied".to_string());
        assert_eq!(err.to_string(), "Forbidden: Access denied");
    }

    #[test]
    fn test_conflict_display() {
        let err = ServerError::Conflict("Resource already exists".to_string());
        assert_eq!(err.to_string(), "Conflict: Resource already exists");
    }

    #[test]
    fn test_internal_display() {
        let err = ServerError::Internal("Something went wrong".to_string());
        assert_eq!(err.to_string(), "Internal error: Something went wrong");
    }

    #[test]
    fn test_json_error_display() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: ServerError = json_err.into();
        assert!(err.to_string().starts_with("JSON error:"));
    }

    #[test]
    fn test_core_error_display() {
        let core_err = zone_core::CoreError::Llm("Connection failed".to_string());
        let err: ServerError = core_err.into();
        assert!(err.to_string().contains("Core error"));
        assert!(err.to_string().contains("Connection failed"));
    }

    // ============ Debug tests ============

    #[test]
    fn test_error_debug() {
        let err = ServerError::NotFound("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("NotFound"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_all_variants_debug() {
        let variants: Vec<ServerError> = vec![
            ServerError::NotFound("test".to_string()),
            ServerError::BadRequest("test".to_string()),
            ServerError::Unauthorized("test".to_string()),
            ServerError::Forbidden("test".to_string()),
            ServerError::Conflict("test".to_string()),
            ServerError::Internal("test".to_string()),
        ];

        for err in variants {
            let debug_str = format!("{:?}", err);
            assert!(!debug_str.is_empty());
        }
    }

    // ============ IntoResponse status code tests ============

    #[tokio::test]
    async fn test_not_found_response_status() {
        let err = ServerError::NotFound("Resource not found".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_bad_request_response_status() {
        let err = ServerError::BadRequest("Invalid data".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_unauthorized_response_status() {
        let err = ServerError::Unauthorized("Not authenticated".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_forbidden_response_status() {
        let err = ServerError::Forbidden("No permission".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_conflict_response_status() {
        let err = ServerError::Conflict("Already exists".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_internal_response_status() {
        let err = ServerError::Internal("Server error".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_json_error_response_status() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("invalid").unwrap_err();
        let err: ServerError = json_err.into();
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_core_error_response_status() {
        let core_err = zone_core::CoreError::Llm("Error".to_string());
        let err: ServerError = core_err.into();
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ============ IntoResponse body tests ============

    #[tokio::test]
    async fn test_response_body_format() {
        let err = ServerError::NotFound("User 123 not found".to_string());
        let response = err.into_response();

        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "User 123 not found");
    }

    #[tokio::test]
    async fn test_response_body_internal_hides_details() {
        // Core errors should not expose internal details in response
        let core_err = zone_core::CoreError::Llm("Sensitive internal error".to_string());
        let err: ServerError = core_err.into();
        let response = err.into_response();

        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // Should show generic message, not the internal error details
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "Internal error");
    }

    // ============ From trait conversion tests ============

    #[test]
    fn test_from_serde_json_error() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("not a number").unwrap_err();
        let err: ServerError = json_err.into();
        assert!(matches!(err, ServerError::Json(_)));
    }

    #[test]
    fn test_from_core_error_llm() {
        let core_err = zone_core::CoreError::Llm("LLM failed".to_string());
        let err: ServerError = core_err.into();
        assert!(matches!(err, ServerError::Core(_)));
    }

    #[test]
    fn test_from_core_error_tool() {
        let core_err = zone_core::CoreError::Tool("Tool failed".to_string());
        let err: ServerError = core_err.into();
        assert!(matches!(err, ServerError::Core(_)));
    }

    #[test]
    fn test_from_core_error_session() {
        let core_err = zone_core::CoreError::Session("Session expired".to_string());
        let err: ServerError = core_err.into();
        assert!(matches!(err, ServerError::Core(_)));
    }

    #[test]
    fn test_from_core_error_cancelled() {
        let core_err = zone_core::CoreError::Cancelled;
        let err: ServerError = core_err.into();
        assert!(matches!(err, ServerError::Core(_)));
    }

    #[test]
    fn test_from_core_error_iteration_limit() {
        let core_err = zone_core::CoreError::IterationLimit(100);
        let err: ServerError = core_err.into();
        assert!(matches!(err, ServerError::Core(_)));
    }

    // ============ Result type alias tests ============

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
        let result: Result<i32> = Err(ServerError::NotFound("not found".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_result_with_question_mark() {
        fn fallible_operation() -> Result<i32> {
            Err(ServerError::BadRequest("invalid".to_string()))
        }

        fn caller() -> Result<i32> {
            let _value = fallible_operation()?;
            Ok(0)
        }

        assert!(caller().is_err());
    }

    // ============ Edge case tests ============

    #[test]
    fn test_empty_message() {
        let err = ServerError::NotFound("".to_string());
        assert_eq!(err.to_string(), "Not found: ");
    }

    #[test]
    fn test_unicode_message() {
        let err = ServerError::NotFound("message".to_string());
        assert!(err.to_string().contains("message"));
    }

    #[test]
    fn test_special_characters_message() {
        let err = ServerError::BadRequest("Invalid <script>alert('xss')</script>".to_string());
        assert!(err.to_string().contains("<script>"));
    }

    #[test]
    fn test_newline_in_message() {
        let err = ServerError::Internal("Line 1\nLine 2".to_string());
        assert!(err.to_string().contains("\n"));
    }

    #[tokio::test]
    async fn test_response_json_escapes_special_chars() {
        let err = ServerError::BadRequest("Test \"quotes\" and \\backslash".to_string());
        let response = err.into_response();

        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // JSON should properly escape special characters
        assert_eq!(json["error"], "Test \"quotes\" and \\backslash");
    }

    // ============ Error trait tests ============

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ServerError>();
        assert_sync::<ServerError>();
    }

    #[test]
    fn test_error_source() {
        use std::error::Error;

        let json_err: serde_json::Error = serde_json::from_str::<i32>("bad").unwrap_err();
        let err: ServerError = json_err.into();

        // Json error should have a source
        if let ServerError::Json(_) = &err {
            assert!(err.source().is_some());
        }
    }

    #[test]
    fn test_string_errors_no_source() {
        use std::error::Error;

        let err = ServerError::NotFound("test".to_string());
        assert!(err.source().is_none());

        let err = ServerError::BadRequest("test".to_string());
        assert!(err.source().is_none());

        let err = ServerError::Internal("test".to_string());
        assert!(err.source().is_none());
    }
}
