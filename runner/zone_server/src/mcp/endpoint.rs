//! The HTTP endpoint a spawned coding agent reaches zone's tools through.
//!
//! One POST, JSON-RPC in and JSON-RPC out, which is MCP's Streamable HTTP
//! transport for a server that never pushes. The bearer token names the turn;
//! no other route accepts it, and this route accepts nothing else.

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response as HttpResponse};
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, ProtocolVersion, Tool as McpTool};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use zone_core::llm::{ToolDefinition, Toolset};

use super::protocol::{self, Request, Response};
use super::turn::Turn;

/// Where this endpoint is mounted. The turn's `Toolset` endpoint is this path
/// against whatever address the server is reachable on.
pub const PATH: &str = "/mcp";

/// Bytes one call's body may occupy.
///
/// A call's arguments can carry a whole file's contents, so axum's 2 MB
/// default would refuse writes the tool itself would have accepted. It stays
/// bounded rather than disabled because the body is buffered before the token
/// in it is checked.
pub const BODY_LIMIT: usize = 16 * 1024 * 1024;

const BEARER: &str = "Bearer ";

/// A tool that declares no parameters still has to declare a schema, or a
/// client has nothing to validate an empty argument object against.
const EMPTY_SCHEMA: &str = r#"{"type":"object","properties":{}}"#;

/// This endpoint's URL on a server reachable at `base`.
pub fn endpoint(base: &str) -> String {
    format!("{}{PATH}", base.trim_end_matches('/'))
}

pub async fn serve(headers: HeaderMap, body: Bytes) -> HttpResponse {
    let Some(turn) = bearer(&headers).and_then(Turn::find) else {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            axum::Json(Response::failed(
                Value::Null,
                ErrorData::new(ErrorCode::INVALID_REQUEST, "Unauthorized", None),
            )),
        )
            .into_response();
    };

    let request: Request = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(Response::failed(
                    Value::Null,
                    ErrorData::parse_error(error.to_string(), None),
                )),
            )
                .into_response();
        }
    };

    let Some(id) = request.id.clone() else {
        return StatusCode::ACCEPTED.into_response();
    };

    let answer = if request.jsonrpc == protocol::VERSION {
        dispatch(&turn, &request.method, request.params).await
    } else {
        Err(ErrorData::invalid_request(
            format!("Expected jsonrpc {}", protocol::VERSION),
            None,
        ))
    };

    axum::Json(Response::of(id, answer)).into_response()
}

async fn dispatch(turn: &Turn, method: &str, params: Option<Value>) -> Result<Value, ErrorData> {
    match method {
        protocol::INITIALIZE => Ok(initialize(params.as_ref())),
        protocol::PING => Ok(json!({})),
        protocol::TOOLS_LIST => Ok(json!({ "tools": catalog(turn) })),
        protocol::TOOLS_CALL => call(turn, params).await,
        unknown => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            format!("Unknown method '{unknown}'"),
            None,
        )),
    }
}

/// Answer the handshake in the version the client opened it in, so a client
/// pinned to an older revision of the protocol is not told to speak a newer
/// one it does not know.
fn initialize(params: Option<&Value>) -> Value {
    let version = params
        .and_then(|params| params.get("protocolVersion"))
        .cloned()
        .and_then(|version| serde_json::from_value::<ProtocolVersion>(version).ok())
        .unwrap_or_default();

    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": Toolset::SERVER,
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn catalog(turn: &Turn) -> Vec<McpTool> {
    turn.tools()
        .all_definitions()
        .iter()
        .map(describe)
        .collect()
}

fn describe(definition: &ToolDefinition) -> McpTool {
    let schema = match &definition.function.parameters {
        Value::Object(schema) => schema.clone(),
        _ => serde_json::from_str::<Map<String, Value>>(EMPTY_SCHEMA)
            .expect("the empty schema is valid JSON"),
    };
    McpTool::new(
        definition.function.name.clone(),
        definition.function.description.clone(),
        Arc::new(schema),
    )
}

async fn call(turn: &Turn, params: Option<Value>) -> Result<Value, ErrorData> {
    let params: CallToolRequestParams = serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|error| {
            ErrorData::invalid_params(format!("Unreadable tool call: {error}"), None)
        })?;

    let name = params.name.as_ref();
    if !turn.tools().has(name) {
        return Err(ErrorData::invalid_params(
            format!("Unknown tool '{name}'"),
            None,
        ));
    }

    let arguments = params.arguments.map_or_else(
        || json!({}).to_string(),
        |arguments| Value::Object(arguments).to_string(),
    );

    serde_json::to_value(turn.run(name, &arguments).await)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix(BEARER)
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::super::testing::{open, state, wait_for_card, write};
    use super::*;
    use crate::agent::{ApprovalGate, ApprovalPolicy};
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use http_body_util::BodyExt;
    use std::time::Duration;
    use tower::ServiceExt;

    async fn post(token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut request = HttpRequest::builder()
            .method("POST")
            .uri(PATH)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("{BEARER}{token}"));
        }
        let response = crate::routes::create_router(state())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .expect("the router answers");

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let parsed = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("the endpoint answers JSON")
        };
        (status, parsed)
    }

    fn rpc(id: i64, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    fn call_of(name: &str, arguments: Value) -> Value {
        json!({"name": name, "arguments": arguments})
    }

    #[tokio::test]
    async fn the_handshake_names_zone_and_echoes_the_clients_protocol_version() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(
            Some(&opened.token),
            rpc(
                1,
                protocol::INITIALIZE,
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "claude-code", "version": "2.1.269"},
                }),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["id"], 1);
        assert_eq!(answer["jsonrpc"], "2.0");
        assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(answer["result"]["serverInfo"]["name"], Toolset::SERVER);
        assert!(
            answer["result"]["capabilities"]["tools"].is_object(),
            "{answer}"
        );
    }

    #[tokio::test]
    async fn a_call_without_the_turns_bearer_is_refused() {
        let opened = open(ApprovalPolicy::auto()).await;

        for offered in [None, Some("not-a-token"), Some("")] {
            let (status, _) = post(offered, rpc(1, protocol::TOOLS_LIST, json!({}))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{offered:?}");
        }

        let (status, _) = post(Some(&opened.token), rpc(1, protocol::TOOLS_LIST, json!({}))).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn tools_list_answers_with_the_turns_own_registry() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) =
            post(Some(&opened.token), rpc(2, protocol::TOOLS_LIST, json!({}))).await;

        assert_eq!(status, StatusCode::OK);
        let tools = answer["result"]["tools"].as_array().expect("a tool array");
        let served: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool name"))
            .collect();
        let registered: Vec<&str> = opened
            .tools
            .all_definitions()
            .iter()
            .map(|definition| definition.function.name.as_str())
            .collect();
        assert_eq!(served, registered);
        assert!(!served.is_empty());
        assert_eq!(
            opened.lease.toolset().tools.len(),
            served.len(),
            "the agent is allowed exactly the tools this endpoint serves it"
        );

        let write_file = tools
            .iter()
            .find(|tool| tool["name"] == "write_file")
            .expect("the host tools are served");
        assert_eq!(write_file["inputSchema"]["type"], "object");
        assert!(
            write_file["description"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }

    /// Two turns are open at once; a token reaches its own turn's tools and
    /// its own reader, and reaches the other's not at all once it is revoked.
    #[tokio::test]
    async fn a_token_for_one_chat_cannot_reach_another_chats_tools() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut mine = open(ApprovalPolicy::auto()).await;
        let mut theirs = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(
            Some(&mine.token),
            rpc(
                3,
                protocol::TOOLS_CALL,
                call_of("write_file", write(&directory.path().join("mine.txt"))),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["result"]["isError"], false, "{answer}");

        assert!(
            mine.events.try_recv().is_ok(),
            "the call belongs to the turn whose token made it"
        );
        assert!(
            theirs.events.try_recv().is_err(),
            "and reaches no other turn's reader"
        );

        drop(mine.lease);

        let (status, _) = post(Some(&mine.token), rpc(4, protocol::TOOLS_LIST, json!({}))).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "a finished turn's token must reach nothing"
        );

        let (status, answer) =
            post(Some(&theirs.token), rpc(5, protocol::TOOLS_LIST, json!({}))).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_mcp_error_not_a_panic() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(
            Some(&opened.token),
            rpc(6, protocol::TOOLS_CALL, call_of("not_a_tool", json!({}))),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["error"]["code"], -32602, "{answer}");
        assert!(answer.get("result").is_none(), "{answer}");
    }

    #[tokio::test]
    async fn an_unknown_method_is_a_method_not_found_error() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(Some(&opened.token), rpc(7, "resources/list", json!({}))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["error"]["code"], -32601, "{answer}");
    }

    #[tokio::test]
    async fn a_notification_is_accepted_with_no_body() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, body) = post(
            Some(&opened.token),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body, Value::Null);
    }

    #[tokio::test]
    async fn a_body_that_is_not_json_rpc_is_refused_before_a_tool_is_reached() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(Some(&opened.token), json!("not a request")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(answer["error"]["code"], -32700, "{answer}");

        let (status, answer) = post(
            Some(&opened.token),
            json!({"jsonrpc": "1.0", "id": 8, "method": protocol::TOOLS_LIST}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["error"]["code"], -32600, "{answer}");
    }

    #[tokio::test]
    async fn a_ping_is_answered() {
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = post(Some(&opened.token), rpc(9, protocol::PING, json!({}))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["result"], json!({}), "{answer}");
    }

    #[tokio::test]
    async fn a_confirmed_call_over_mcp_waits_for_the_console_and_a_denial_refuses_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("gated.txt");
        let mut opened = open(ApprovalPolicy::required(ApprovalGate::new())).await;
        let token = opened.token.clone();
        let request = rpc(
            10,
            protocol::TOOLS_CALL,
            call_of("write_file", write(&path)),
        );

        let calling = tokio::spawn(async move { post(Some(&token), request).await });
        let id = wait_for_card(&mut opened.events).await;
        assert!(!calling.is_finished(), "the call must wait for the answer");

        assert!(opened.approval.decide(&id, false), "the denial lands");

        let (status, answer) = tokio::time::timeout(Duration::from_secs(5), calling)
            .await
            .expect("a denied call answers at once")
            .expect("the call task");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        assert!(!path.exists(), "a denied write must change nothing");
    }

    #[tokio::test]
    async fn an_auto_approve_turn_answers_a_call_without_asking_anybody() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("auto.txt");
        let opened = open(ApprovalPolicy::auto()).await;

        let (status, answer) = tokio::time::timeout(
            Duration::from_secs(5),
            post(
                Some(&opened.token),
                rpc(
                    11,
                    protocol::TOOLS_CALL,
                    call_of("write_file", write(&path)),
                ),
            ),
        )
        .await
        .expect("an auto-approve turn must not wait for anybody");

        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["result"]["isError"], false, "{answer}");
        assert_eq!(answer["result"]["content"][0]["type"], "text");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "written");
    }

    /// A call carrying a file's contents is a call, not an upload, and the
    /// platform default would have refused it before the handler saw it.
    #[tokio::test]
    async fn a_call_larger_than_the_platform_default_still_reaches_its_tool() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("large.txt");
        let opened = open(ApprovalPolicy::auto()).await;
        let content = "x".repeat(3 * 1024 * 1024);

        let (status, answer) = post(
            Some(&opened.token),
            rpc(
                12,
                protocol::TOOLS_CALL,
                call_of(
                    "write_file",
                    json!({
                        "path": path.to_string_lossy(),
                        "content": content,
                        "reason": "Write a file bigger than the default body limit.",
                    }),
                ),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(answer["result"]["isError"], false, "{answer}");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            content.len() as u64
        );
    }

    #[tokio::test]
    async fn the_endpoint_url_is_the_path_this_route_is_mounted_on() {
        assert_eq!(
            endpoint("http://127.0.0.1:8080"),
            format!("http://127.0.0.1:8080{PATH}")
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8080/"),
            format!("http://127.0.0.1:8080{PATH}")
        );
    }
}
