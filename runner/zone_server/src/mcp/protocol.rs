//! The JSON-RPC 2.0 envelope MCP's Streamable HTTP transport carries.

use rmcp::model::ErrorData;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "2.0";

pub const INITIALIZE: &str = "initialize";
pub const PING: &str = "ping";
pub const TOOLS_CALL: &str = "tools/call";
pub const TOOLS_LIST: &str = "tools/list";

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    /// Absent on a notification, which is acknowledged with no body at all.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorData>,
}

impl Response {
    pub fn answered(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failed(id: Value, error: ErrorData) -> Self {
        Self {
            jsonrpc: VERSION,
            id,
            result: None,
            error: Some(error),
        }
    }

    pub fn of(id: Value, answer: Result<Value, ErrorData>) -> Self {
        match answer {
            Ok(result) => Self::answered(id, result),
            Err(error) => Self::failed(id, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;
    use serde_json::json;

    #[test]
    fn a_request_without_an_id_is_a_notification() {
        let request: Request = serde_json::from_value(
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .unwrap();

        assert!(request.id.is_none());
        assert_eq!(request.method, "notifications/initialized");
        assert!(request.params.is_none());
    }

    #[test]
    fn a_response_carries_a_result_or_an_error_but_never_both() {
        let answered = serde_json::to_value(Response::answered(json!(1), json!({"ok": true})))
            .expect("an answer serializes");
        assert_eq!(answered["jsonrpc"], "2.0");
        assert_eq!(answered["result"]["ok"], true);
        assert!(answered.get("error").is_none());

        let failed = serde_json::to_value(Response::failed(
            json!(1),
            ErrorData::new(ErrorCode::METHOD_NOT_FOUND, "nope", None),
        ))
        .expect("a failure serializes");
        assert_eq!(failed["error"]["code"], -32601);
        assert!(failed.get("result").is_none());
    }
}
