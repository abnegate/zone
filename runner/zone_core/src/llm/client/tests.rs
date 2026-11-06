use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use abnegate_agent_cli::{
    AgentEvent, AgentStream, CREDITS_REQUIRED, Output, STDERR_HEADING, StepStatus, Subagent,
    SubagentStatus, SubagentStep,
};
use abnegate_llm::{
    ChatResponse, ChatStreamChunk, Credential, Effort, Error, Limit, LlmConfig, Message,
    ProviderError, RequestOptions, ToolCall, ToolDefinition, Trust, Usage, Window, WindowKind,
};
use futures::{Stream, StreamExt};
use tempfile::TempDir;

use super::*;
use crate::llm::cli::{SignIn, settings};
use crate::llm::window::FIVE_HOURS;
use stand_in::StandIn;

mod environment;
mod images;
mod isolation;
mod models;
mod occupancy;
mod stand_in;
mod stream;
mod toolset;
mod wording;

const COMPLETION: &str = r#"{
    "id": "completion",
    "object": "chat.completion",
    "created": 0,
    "model": "test",
    "choices": [{
        "index": 0,
        "message": {"role": "assistant", "content": "hi"},
        "finish_reason": "stop"
    }]
}"#;

fn completion() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(200).set_body_raw(COMPLETION, "application/json")
}

fn endpoint(base_url: &str, api_key: &str) -> LlmConfig {
    LlmConfig::new(base_url, "test", api_key)
}

mod authorization {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Match, Mock, MockServer, Request};

    use super::*;

    struct Unauthorized;

    impl Match for Unauthorized {
        fn matches(&self, request: &Request) -> bool {
            !request.headers.contains_key(reqwest::header::AUTHORIZATION)
        }
    }

    async fn turn(server: &MockServer, api_key: &str) -> Result<ChatResponse, Error> {
        LlmClient::new(endpoint(&server.uri(), api_key))
            .chat(&[Message::user("hi")], None)
            .await
    }

    #[tokio::test]
    async fn an_endpoint_without_a_key_is_sent_no_authorization_header() {
        for blank in ["", "   "] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(Unauthorized)
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let answered = turn(&server, blank).await;

            assert!(answered.is_ok(), "{blank:?}: {answered:?}");
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn an_endpoint_with_a_key_is_sent_it_as_a_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-endpoint-key"))
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;

        let answered = turn(&server, "sk-endpoint-key").await;

        assert!(answered.is_ok(), "{answered:?}");
        server.verify().await;
    }
}

mod refusal {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const KEY: &str = "sk-proj-AbCdEfGh1234567890wxyz";

    fn echoes() -> Vec<String> {
        vec![
            format!(r#"{{"error":{{"message":"bad key {KEY}"}}}}"#),
            r#"{"error":{"message":"Incorrect API key provided: sk-proj-****************wxyz."}}"#
                .to_string(),
            r#"{"error":"{\"message\":\"invalid x-api-key: ****wxyz\"}"}"#.to_string(),
            r#"{"detail":"litellm.AuthenticationError: api_key=sk-proj-AbCd****"}"#.to_string(),
            "rejected key:sk-proj-AbCd****".to_string(),
        ]
    }

    async fn refused(body: &str, stream: bool) -> Error {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(401).set_body_string(body))
            .mount(&server)
            .await;
        let client = LlmClient::new(endpoint(&server.uri(), KEY));
        let messages = [Message::user("hi")];
        if stream {
            client
                .chat_stream(&messages, None)
                .await
                .err()
                .expect("the endpoint refused the stream")
        } else {
            client
                .chat(&messages, None)
                .await
                .expect_err("the endpoint refused the turn")
        }
    }

    #[tokio::test]
    async fn a_refusal_never_carries_the_key_it_was_sent() {
        for body in echoes() {
            for stream in [false, true] {
                let error = refused(&body, stream).await;
                let Error::Api {
                    status, message, ..
                } = &error
                else {
                    panic!("{error:?}");
                };
                let shown = error.to_string();

                assert_eq!(*status, 401);
                for leaked in [KEY, "wxyz", "sk-proj-AbCd"] {
                    assert!(!message.contains(leaked), "{body} -> {message}");
                    assert!(!shown.contains(leaked), "{body} -> {shown}");
                }
                assert!(message.contains(abnegate_secret::REDACTED), "{message}");
            }
        }
    }

    #[tokio::test]
    async fn a_refusal_without_the_key_is_reported_as_the_endpoint_sent_it() {
        let body = r#"{"error":{"message":"The model `gpt-9` does not exist"}}"#;

        let error = refused(body, false).await;

        assert!(
            matches!(&error, Error::Api { status: 401, message, .. } if message == body),
            "{error:?}"
        );
    }
}

mod tenant {
    use serde_json::json;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const KEY: &str = "sk-tenant-AbCdEfGh1234567890wxyz";
    const REPORTED_LIMIT: usize = 500;
    const ELLIPSIS: char = '…';

    fn client(base_url: &str) -> LlmClient {
        LlmClient::new(endpoint(base_url, KEY).with_trust(Trust::Tenant))
    }

    async fn refusal(response: ResponseTemplate) -> Error {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(response)
            .mount(&server)
            .await;
        client(&server.uri())
            .chat(&[Message::user("hi")], None)
            .await
            .expect_err("the endpoint refused the turn")
    }

    fn reported(error: &Error) -> (u16, String) {
        let Error::Api {
            status, message, ..
        } = error
        else {
            panic!("{error:?}");
        };
        let envelope: serde_json::Value = serde_json::from_str(message).expect("an error envelope");
        let text = envelope["error"]["message"]
            .as_str()
            .expect("an error message")
            .to_string();
        assert_eq!(
            envelope,
            json!({ "error": { "message": text } }),
            "{message}"
        );
        (*status, text)
    }

    #[tokio::test]
    async fn a_tenant_endpoint_is_never_sent_to_a_metadata_address() {
        for base_url in [
            "http://169.254.169.254/latest",
            "http://169.254.170.2/v1",
            "http://[fe80::1]:8080/v1",
            "http://[fd00:ec2::254]/v1",
            "http://[::ffff:169.254.169.254]/v1",
            "http://[fd20:ce::254]/computeMetadata/v1",
            "http://168.63.129.16/machine",
            "http://metadata.google.internal/computeMetadata/v1",
            "http://METADATA.GOOGLE.INTERNAL./v1",
        ] {
            let refused = client(base_url).chat(&[Message::user("hi")], None).await;

            assert!(
                matches!(&refused, Err(Error::InvalidConfig(message)) if message.contains("metadata")),
                "{base_url}: {refused:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_tenant_endpoint_is_never_redirected_to_a_metadata_address() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let refused = client(&server.uri())
            .chat(&[Message::user("hi")], None)
            .await
            .expect_err("a redirect is refused");

        assert_eq!(reported(&refused), (307, "Temporary Redirect".to_string()));
        server.verify().await;
    }

    #[tokio::test]
    async fn a_tenant_endpoint_is_never_followed_through_a_redirect() {
        let elsewhere = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/admin", elsewhere.uri())),
            )
            .expect(1)
            .mount(&server)
            .await;

        let refused = client(&server.uri())
            .chat(&[Message::user("hi")], None)
            .await
            .expect_err("a redirect is refused");

        assert_eq!(reported(&refused), (307, "Temporary Redirect".to_string()));
        server.verify().await;
        elsewhere.verify().await;
    }

    #[tokio::test]
    async fn a_tenant_endpoint_still_reaches_loopback() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;
        let localhost = server.uri().replace("127.0.0.1", "localhost");

        let answered = client(&localhost).chat(&[Message::user("hi")], None).await;

        assert!(answered.is_ok(), "{answered:?}");
        server.verify().await;
    }

    #[tokio::test]
    async fn a_tenant_refusal_reports_only_its_status_and_error_message() {
        let error = refusal(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "The model `gpt-9` does not exist",
                "type": "invalid_request_error",
            },
            "reflected": "ami-id: i-0123456789abcdef0",
        })))
        .await;

        assert_eq!(
            reported(&error),
            (400, "The model `gpt-9` does not exist".to_string())
        );
        assert!(!error.to_string().contains("ami-id"), "{error}");
    }

    #[tokio::test]
    async fn a_tenant_refusal_without_an_error_message_reports_its_status_alone() {
        for body in [
            "<html><body>instance-id: i-0123456789abcdef0</body></html>",
            r#"{"Code":"Success","AccessKeyId":"ASIAEXAMPLE"}"#,
            "",
        ] {
            let error = refusal(ResponseTemplate::new(502).set_body_string(body)).await;

            assert_eq!(reported(&error), (502, "Bad Gateway".to_string()), "{body}");
        }
    }

    #[tokio::test]
    async fn a_tenant_refusal_is_capped_and_never_carries_the_key() {
        let long = format!("{}{KEY} sk-tenant-****wxyz", "x".repeat(2_000));
        let error = refusal(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "error": { "message": format!("bad key {KEY}") } })),
        )
        .await;
        let (_, message) = reported(&error);
        assert!(!message.contains(KEY), "{message}");

        let error = refusal(
            ResponseTemplate::new(400).set_body_json(json!({ "error": { "message": long } })),
        )
        .await;
        let (_, message) = reported(&error);

        assert_eq!(message.chars().count(), REPORTED_LIMIT + 1);
        assert!(message.ends_with(ELLIPSIS), "{message}");
        assert!(!message.contains("wxyz"), "{message}");
    }

    #[tokio::test]
    async fn a_tenant_refusal_still_says_when_a_model_takes_no_tools() {
        for body in [
            json!({ "error": "llava:7b does not support tools" }),
            json!({ "error": { "message": "llava:7b does not support tools" } }),
        ] {
            let error = refusal(ResponseTemplate::new(400).set_body_json(body)).await;

            assert!(error.unsupported_tools(), "{error:?}");
        }
    }
}

mod deadline {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const DEADLINE: Duration = Duration::from_millis(200);
    const STALL: Duration = Duration::from_secs(5);

    async fn stalling() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(completion().set_delay(STALL))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_completion_that_outlives_its_timeout_fails_with_the_timeout() {
        let server = stalling().await;
        let client = LlmClient::new(endpoint(&server.uri(), "").with_timeout(DEADLINE));

        let started = tokio::time::Instant::now();
        let error = client
            .chat(&[Message::user("hi")], None)
            .await
            .expect_err("a stalled completion fails");

        assert!(
            matches!(error, Error::Timeout(waited) if waited == DEADLINE),
            "{error:?}"
        );
        assert!(started.elapsed() < STALL, "the client waited out the stall");
    }

    #[tokio::test]
    async fn a_stream_that_sends_nothing_fails_with_the_read_timeout() {
        let server = stalling().await;
        let client = LlmClient::new(endpoint(&server.uri(), "").with_read_timeout(DEADLINE));

        let started = tokio::time::Instant::now();
        let error = client
            .chat_stream(&[Message::user("hi")], None)
            .await
            .err()
            .expect("a silent stream fails");

        assert!(
            matches!(error, Error::Timeout(waited) if waited == DEADLINE),
            "{error:?}"
        );
        assert!(started.elapsed() < STALL, "the client waited out the stall");
    }

    #[tokio::test]
    async fn an_operator_endpoint_that_redirects_is_refused_rather_than_followed() {
        let elsewhere = MockServer::start().await;
        Mock::given(any())
            .respond_with(completion())
            .expect(0)
            .mount(&elsewhere)
            .await;
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(307).insert_header(
                "location",
                format!("{}/v1/chat/completions", elsewhere.uri()),
            ))
            .expect(1)
            .mount(&server)
            .await;

        let refused = LlmClient::new(endpoint(&server.uri(), "sk-operator-key"))
            .chat(&[Message::user("hi")], None)
            .await
            .expect_err("a redirect is refused");

        assert!(
            matches!(refused, Error::Api { status: 307, .. }),
            "{refused:?}"
        );
        server.verify().await;
        elsewhere.verify().await;
    }
}

mod shape {
    use serde_json::{Value, json};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

    use super::*;
    use abnegate_llm::{Dialect, TEMPLATE_STOPS};

    const TEMPERATURE: f32 = 1.4;
    const RESERVED: u32 = 321;

    struct Without(&'static str);

    impl Match for Without {
        fn matches(&self, request: &Request) -> bool {
            serde_json::from_slice::<Value>(&request.body)
                .is_ok_and(|body| body.get(self.0).is_none())
        }
    }

    fn streamed() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
    }

    fn stops() -> Vec<String> {
        TEMPLATE_STOPS
            .iter()
            .map(|stop| (*stop).to_string())
            .chain(["User:", " ", "Human:", "###", "END", "STOP"].map(String::from))
            .collect()
    }

    fn client(server: &MockServer, dialect: Dialect) -> LlmClient {
        LlmClient::new(
            endpoint(&server.uri(), "")
                .with_temperature(TEMPERATURE)
                .with_maximum_tokens(RESERVED)
                .with_dialect(dialect),
        )
        .with_stop(stops())
        .with_ollama_context("model", 8192)
    }

    async fn last_body(server: &MockServer) -> Value {
        let requests = server.received_requests().await.unwrap_or_default();
        let request = requests.last().expect("a request reached the endpoint");
        serde_json::from_slice(&request.body).expect("a JSON body")
    }

    async fn sent(server: &MockServer, dialect: Dialect, model: &str) -> Value {
        let answered = client(server, dialect)
            .chat_with_model(model, &[Message::user("hi")], None)
            .await;
        assert!(answered.is_ok(), "{dialect:?} {model}: {answered:?}");
        last_body(server).await
    }

    fn stop_count(body: &Value) -> usize {
        body.get("stop")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    }

    #[tokio::test]
    async fn openai_is_sent_at_most_four_stops_and_no_template_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({
                "max_tokens": RESERVED,
                "stop": ["User:", " ", "Human:", "###"],
            })))
            .and(Without("max_completion_tokens"))
            .and(Without("num_ctx"))
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;

        let body = sent(&server, Dialect::OpenAI, "gpt-4o").await;

        assert_eq!(stop_count(&body), 4, "{body}");
        assert!(body.get("temperature").is_some(), "{body}");
        server.verify().await;
    }

    #[tokio::test]
    async fn an_openai_reasoning_model_is_sent_no_stop_no_temperature_and_a_completion_budget() {
        for model in ["o3-mini", "o1", "o4-mini", "gpt-5"] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(body_partial_json(
                    json!({ "max_completion_tokens": RESERVED }),
                ))
                .and(Without("max_tokens"))
                .and(Without("temperature"))
                .and(Without("stop"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let body = sent(&server, Dialect::OpenAI, model).await;

            assert_eq!(stop_count(&body), 0, "{model}: {body}");
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn an_openai_chat_variant_keeps_its_temperature_and_stops() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({ "max_tokens": RESERVED })))
            .and(Without("max_completion_tokens"))
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;

        let body = sent(&server, Dialect::OpenAI, "gpt-5-chat-latest").await;

        assert_eq!(stop_count(&body), 4, "{body}");
        assert_eq!(
            body["temperature"].as_f64().map(|value| value as f32),
            Some(TEMPERATURE),
            "{body}"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn an_openai_reasoning_model_streams_under_the_same_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({
                "max_completion_tokens": RESERVED,
                "stream": true,
            })))
            .and(Without("max_tokens"))
            .and(Without("temperature"))
            .and(Without("stop"))
            .respond_with(streamed())
            .expect(1)
            .mount(&server)
            .await;

        let mut stream = client(&server, Dialect::OpenAI)
            .chat_stream_with_model("o3", &[Message::user("hi")], None)
            .await
            .expect("the stream opens");
        while let Some(chunk) = stream.next().await {
            assert!(chunk.is_ok(), "{chunk:?}");
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn openai_is_sent_reasoning_effort_only_for_a_model_that_reasons() {
        for (model, expected) in [("o3", Some("high")), ("gpt-4o", None)] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(completion())
                .mount(&server)
                .await;
            let answered = client(&server, Dialect::OpenAI)
                .with_reasoning(model, Effort::High)
                .chat_with_model(model, &[Message::user("hi")], None)
                .await;
            assert!(answered.is_ok(), "{answered:?}");
            let body = last_body(&server).await;

            assert_eq!(
                body.get("reasoning_effort").and_then(Value::as_str),
                expected,
                "{model}: {body}"
            );
        }
    }

    #[tokio::test]
    async fn reasoning_effort_is_model_bound_and_uses_the_resolved_level() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(completion())
            .mount(&server)
            .await;
        let client =
            LlmClient::new(endpoint(&server.uri(), "")).with_reasoning("thinker", Effort::High);
        let effort = |body: Value| {
            body.get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::to_string)
        };

        for (client, model, expected) in [
            (client.clone(), "thinker", Some("high")),
            (client.clone(), "other", None),
            (client.without_reasoning(), "thinker", None),
        ] {
            let answered = client
                .chat_with_model(model, &[Message::user("Hi")], None)
                .await;
            assert!(answered.is_ok(), "{answered:?}");

            assert_eq!(
                effort(last_body(&server).await).as_deref(),
                expected,
                "{model}"
            );
        }
    }

    #[tokio::test]
    async fn anthropic_is_sent_every_non_blank_stop_and_a_temperature_of_at_most_one() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({
                "max_tokens": RESERVED,
                "temperature": 1.0,
            })))
            .and(Without("max_completion_tokens"))
            .and(Without("num_ctx"))
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;

        let body = sent(&server, Dialect::Anthropic, "claude-sonnet-4-5").await;

        assert_eq!(stop_count(&body), stops().len() - 1, "{body}");
        assert!(
            body["stop"]
                .as_array()
                .is_some_and(|stops| stops.iter().all(|stop| stop != " ")),
            "{body}"
        );
        server.verify().await;
    }

    const UPLOAD: &str = "data:image/png;base64,dXBsb2Fk";
    const SCREENSHOT: &str = "data:image/png;base64,c2NyZWVu";
    const GENERATED: &str = "data:image/png;base64,Z2VuZXJhdGVk";
    const DRAWN: &str = "data:image/png;base64,ZHJhd24=";

    fn history() -> Vec<Message> {
        let mut asked = Message::user("What is on screen?");
        asked.images = vec![UPLOAD.to_string()];
        let mut requested = Message::assistant_with_tools(vec![
            ToolCall::function("call_1", "screenshot", ""),
            ToolCall::function("call_2", "list", r#"{"path":"."}"#),
        ]);
        requested.reasoning_content = Some("I should look first".to_string());
        requested.thinking_blocks =
            vec![json!({ "type": "thinking", "thinking": "look", "signature": "c2ln" })];
        let mut captured = Message::tool_result("call_1", "captured");
        captured.images = vec![SCREENSHOT.to_string()];
        let listed = Message::tool_result("call_2", "a.rs");
        let mut answered = Message::assistant("Here it is");
        answered.images = vec![GENERATED.to_string()];
        answered.reasoning_content = Some("the screen shows a.rs".to_string());
        let mut drawn = Message::assistant("");
        drawn.images = vec![DRAWN.to_string()];
        vec![
            Message::system("Be brief."),
            asked,
            requested,
            captured,
            listed,
            answered,
            drawn,
            Message::user("Again."),
        ]
    }

    fn tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::function("screenshot", "Capture the screen", json!({})),
            ToolDefinition::function(
                "list",
                "List a directory",
                json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
            ),
        ]
    }

    fn image(url: &str) -> Value {
        json!({ "type": "image_url", "image_url": { "url": url } })
    }

    fn forwarded(label: &str, url: &str) -> Value {
        json!({ "role": "user", "content": [{ "type": "text", "text": label }, image(url)] })
    }

    fn provider_contract() -> (Value, Value) {
        let messages = json!([
            { "role": "system", "content": "Be brief." },
            {
                "role": "user",
                "content": [{ "type": "text", "text": "What is on screen?" }, image(UPLOAD)],
            },
            {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "screenshot", "arguments": "{}" },
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": { "name": "list", "arguments": r#"{"path":"."}"# },
                    },
                ],
            },
            { "role": "tool", "content": "captured", "tool_call_id": "call_1" },
            { "role": "tool", "content": "a.rs", "tool_call_id": "call_2" },
            forwarded("[Image from the tool result for call_1]", SCREENSHOT),
            { "role": "assistant", "content": "Here it is" },
            forwarded("[Image from the previous assistant message]", GENERATED),
            forwarded("[Image from the previous assistant message]", DRAWN),
            { "role": "user", "content": "Again." },
        ]);
        let tools = json!([
            {
                "type": "function",
                "function": {
                    "name": "screenshot",
                    "description": "Capture the screen",
                    "parameters": { "type": "object", "properties": {} },
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "list",
                    "description": "List a directory",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                    },
                },
            },
        ]);
        (messages, tools)
    }

    async fn contract(dialect: Dialect, stream: bool) -> Value {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(if stream { streamed() } else { completion() })
            .expect(1)
            .mount(&server)
            .await;
        let client = client(&server, dialect);
        let (messages, tools) = (history(), tools());
        if stream {
            let mut chunks = client
                .chat_stream_with_model("model", &messages, Some(&tools))
                .await
                .expect("the stream opens");
            while let Some(chunk) = chunks.next().await {
                assert!(chunk.is_ok(), "{dialect:?}: {chunk:?}");
            }
        } else {
            let answered = client
                .chat_with_model("model", &messages, Some(&tools))
                .await;
            assert!(answered.is_ok(), "{dialect:?}: {answered:?}");
        }
        server.verify().await;
        last_body(&server).await
    }

    #[tokio::test]
    async fn a_provider_api_is_sent_only_the_message_shapes_it_accepts() {
        let (messages, tools) = provider_contract();
        for dialect in [Dialect::OpenAI, Dialect::Anthropic] {
            for stream in [false, true] {
                let body = contract(dialect, stream).await;

                assert_eq!(body["messages"], messages, "{dialect:?} stream={stream}");
                assert_eq!(body["tools"], tools, "{dialect:?} stream={stream}");
                assert_eq!(body["stream"], json!(stream), "{dialect:?}");
                assert!(body.get("tool_choice").is_none(), "{dialect:?}: {body}");
            }
        }
    }

    #[tokio::test]
    async fn a_provider_api_is_never_sent_an_empty_tool_list() {
        for dialect in [Dialect::OpenAI, Dialect::Anthropic] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(Without("tools"))
                .respond_with(completion())
                .expect(1)
                .mount(&server)
                .await;

            let answered = client(&server, dialect)
                .chat_with_model("model", &[Message::user("hi")], Some(&[]))
                .await;

            assert!(answered.is_ok(), "{dialect:?}: {answered:?}");
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn a_compatible_endpoint_is_sent_the_history_as_it_always_was() {
        let history = history();
        let tools = tools();
        for stream in [false, true] {
            let body = contract(Dialect::Compatible, stream).await;

            assert_eq!(
                body["messages"],
                serde_json::to_value(&history).expect("a history"),
                "stream={stream}"
            );
            assert_eq!(
                body["tools"],
                serde_json::to_value(&tools).expect("tools"),
                "stream={stream}"
            );
            assert_eq!(
                body["messages"][3]["content"][1],
                image(SCREENSHOT),
                "LiteLLM translates a tool result's image itself"
            );
        }
    }

    #[tokio::test]
    async fn a_compatible_endpoint_is_sent_the_request_as_it_always_was() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({
                "max_tokens": RESERVED,
                "num_ctx": 8192,
            })))
            .and(Without("max_completion_tokens"))
            .respond_with(completion())
            .expect(1)
            .mount(&server)
            .await;

        let body = sent(&server, Dialect::Compatible, "model").await;

        assert_eq!(stop_count(&body), stops().len(), "{body}");
        assert_eq!(
            body["temperature"].as_f64().map(|value| value as f32),
            Some(TEMPERATURE),
            "{body}"
        );
        server.verify().await;
    }
}

mod failure {
    use super::*;

    const SESSION_LIMIT: &str =
        "rate limit reached (five_hour, rejected): You've hit your session limit · resets 5pm";

    /// The task worker and the chat's sign-in remedy classify a failure by
    /// its text, so every variant's rendering is pinned here.
    #[tokio::test]
    async fn every_failure_renders_the_words_zone_classifies_it_by() {
        let unreachable = reqwest::Client::new()
            .post("http://127.0.0.1:1/v1/chat/completions?key=sk-in-the-query")
            .send()
            .await
            .expect_err("nothing listens on port 1");
        let transport = Error::from(unreachable).to_string();
        assert!(transport.starts_with("HTTP error: "), "{transport}");
        assert!(
            !transport.contains("sk-in-the-query") && !transport.contains("127.0.0.1:1"),
            "a transport failure carried its URL: {transport}"
        );

        let malformed =
            serde_json::from_str::<serde_json::Value>("invalid json").expect_err("not JSON");
        let json = Error::from(malformed).to_string();
        assert!(json.starts_with("JSON error: "), "{json}");

        for (error, rendered) in [
            (
                Error::api(429, "slow down"),
                "API error: 429 - slow down".to_string(),
            ),
            (
                Error::Stream("Connection reset".to_string()),
                "Stream error: Connection reset".to_string(),
            ),
            (
                Error::InvalidConfig("LLM base_url must include a host".to_string()),
                "Invalid configuration: LLM base_url must include a host".to_string(),
            ),
            (
                Error::Timeout(Duration::from_secs(600)),
                "the endpoint sent nothing for 600s".to_string(),
            ),
            (
                Error::agent("Not logged in · Please run /login"),
                "Not logged in · Please run /login".to_string(),
            ),
            (
                Error::limited("claude", Limit::new(SESSION_LIMIT)),
                format!("claude: {SESSION_LIMIT}"),
            ),
        ] {
            assert_eq!(error.to_string(), rendered, "{error:?}");
        }
    }

    #[tokio::test]
    async fn an_endpoint_url_the_client_cannot_send_to_is_refused_before_any_request() {
        for (base_url, rule) in [
            ("file:///etc/passwd", "http or https"),
            ("https://user:pass@api.openai.com/v1", "userinfo"),
            ("/v1", "absolute URL"),
        ] {
            let refused = LlmClient::new(endpoint(base_url, ""))
                .chat(&[Message::user("hi")], None)
                .await;

            assert!(
                matches!(&refused, Err(Error::InvalidConfig(message)) if message.contains(rule)),
                "{base_url}: {refused:?}"
            );
        }
    }

    fn session_limit() -> Limit {
        Limit::new(SESSION_LIMIT).with_resets_at(
            chrono::DateTime::from_timestamp(1_790_208_000, 0).expect("a valid timestamp"),
        )
    }

    /// Whatever reads a failure's text, the task worker's retry policy and
    /// the chat's sign-in remedy among them, reads a limit exactly as it read
    /// the agent failure the same refusal was before limits had their own
    /// variant.
    #[test]
    fn a_limit_error_reads_as_the_agent_error_it_replaces() {
        for message in [
            SESSION_LIMIT.to_string(),
            format!(
                "{CREDITS_REQUIRED}: Fable 5.1 requires usage credits. Switch to another model to continue."
            ),
            format!("You have hit your usage limit.{STDERR_HEADING}warning: the plan is spent"),
        ] {
            let limit = session_limit().with_message(message.clone());
            let before = Error::Agent(ProviderError::agent("claude", &message).to_string());
            let provider = ProviderError::limited("claude", limit.clone());

            assert_eq!(provider.to_string(), before.to_string());
            assert_eq!(Error::from(provider).to_string(), before.to_string());
            assert_eq!(
                Error::limited("claude", limit.clone()).to_string(),
                before.to_string()
            );
            assert_eq!(
                format!("Stream error: {}", Error::limited("claude", limit)),
                format!("Stream error: claude: {message}")
            );
        }
    }
}

/// The line a signed-out claude 2.1.269 ends its run with. The subtype
/// says success and the run still failed.
const SIGNED_OUT: &str = r#"{"type":"result","subtype":"success","is_error":true,"terminal_reason":"api_error","result":"Not logged in · Please run /login"}"#;
const REFUSAL: &str = "Not logged in \u{b7} Please run /login";

/// What `claude --print --output-format stream-json` really emits when the
/// host session has expired: the refusal arrives as assistant text first,
/// and only the result that follows says it was never an answer.
const SIGNED_OUT_ASSISTANT: &str = r#"{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"Not logged in · Please run /login"}],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":1}},"session_id":"s1"}"#;

const SESSION_LIMIT: &str =
    "rate limit reached (five_hour, rejected): You've hit your session limit · resets 5pm";

/// A stand-in for an agent CLI, so no test needs one signed in on the host.
fn fake(directory: &TempDir, script: &str) -> PathBuf {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let path = directory.path().join("agent");
    let mut file = std::fs::File::create(&path).expect("the fake agent");
    write!(file, "#!/bin/sh\n{script}\n").expect("the fake agent body");
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the fake agent to be executable");
    path
}

fn agent_client(executable: PathBuf) -> LlmClient {
    let settings = settings()
        .with_executable(executable)
        .with_timeout(Duration::from_secs(20));
    LlmClient::new(LlmConfig::default()).with_backend(LlmBackend::cli(AgentKind::Claude, settings))
}

fn agent_events(events: Vec<Result<AgentEvent, ProviderError>>) -> AgentStream {
    Box::pin(futures::stream::iter(events))
}

fn turn(reported: Reported) -> Turn {
    Turn::new(AgentKind::Claude, SignIn::Instance, None, None, reported)
}

fn occupancy() -> Occupancy {
    Occupancy::reported(200_000, Some(16_632)).expect("a reported window")
}

async fn collected(
    stream: impl Stream<Item = Result<ChatStreamChunk, Error>>,
) -> (Vec<ChatStreamChunk>, Option<Error>) {
    let mut stream = Box::pin(stream);
    let mut delivered = Vec::new();
    let mut failure = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => delivered.push(chunk),
            Err(error) => failure = Some(error),
        }
    }
    (delivered, failure)
}

fn spoken(chunks: &[ChatStreamChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| chunk.choices.first()?.delta.content.clone())
        .collect()
}

fn reasoned(chunks: &[ChatStreamChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| chunk.choices.first()?.delta.reasoning_content.clone())
        .collect()
}

fn reasons(chunks: &[ChatStreamChunk]) -> Vec<String> {
    chunks
        .iter()
        .filter_map(|chunk| chunk.choices.first()?.finish_reason.clone())
        .collect()
}

async fn stream_turn(
    client: &LlmClient,
    tools: Option<&[ToolDefinition]>,
) -> Result<ChatStream, Error> {
    client
        .chat_stream_with_options(
            "sonnet",
            &[Message::user("What does a.rs do?")],
            tools,
            RequestOptions::new(512),
        )
        .await
}

fn session_limit() -> Limit {
    Limit::new(SESSION_LIMIT).with_resets_at(
        chrono::DateTime::from_timestamp(1_790_208_000, 0).expect("a valid timestamp"),
    )
}

fn five_hours_at(used_percent: f64) -> Window {
    Window::new("five_hour")
        .with_used_percent(used_percent)
        .with_resets_at(
            chrono::DateTime::from_timestamp(1_790_208_000, 0).expect("a valid timestamp"),
        )
}

#[test]
fn a_client_exposes_the_config_it_was_built_from() {
    let config = LlmConfig::new(
        "https://custom.openai.com/v1",
        "gpt-4-turbo",
        "sk-custom-key",
    )
    .with_temperature(0.3)
    .with_maximum_tokens(8192);

    let client = LlmClient::new(config);
    let cloned = client.clone();

    for client in [client, cloned] {
        assert_eq!(client.config().base_url, "https://custom.openai.com/v1");
        assert_eq!(client.config().api_key.expose(), "sk-custom-key");
        assert_eq!(client.config().default_model, "gpt-4-turbo");
        assert!((client.config().temperature - 0.3).abs() < f32::EPSILON);
        assert_eq!(client.config().maximum_tokens, 8192);
    }
}

/// Every worker logs the client, and the config travels inside it.
#[test]
fn a_client_never_prints_its_key() {
    const KEY: &str = "sk-test-6b02da97e15c4f38";
    let client = LlmClient::new(LlmConfig::new("https://api.openai.com/v1", "gpt-4", KEY));
    let debug = format!("{client:?}");

    assert!(debug.contains("LlmClient"));
    assert!(debug.contains("config"));
    assert!(
        !debug.contains(KEY),
        "the provider key reached a debug line through the client: {debug}"
    );

    const CREDENTIAL: &str = "sk-ant-test-0f9e3c71d2a84b56";
    const TOKEN: &str = "zone-turn-5c1d7a90e3b24f68";
    let client = client
        .with_backend(LlmBackend::cli(
            AgentKind::Claude,
            settings().with_credential(Credential::key("ANTHROPIC_API_KEY", CREDENTIAL)),
        ))
        .with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", TOKEN, ["read_file"]),
            BuiltinTools::Withheld,
        );
    let debug = format!("{client:?}");

    assert!(debug.contains("Cli"));
    assert!(debug.contains("ANTHROPIC_API_KEY"));
    for secret in [KEY, CREDENTIAL, TOKEN] {
        assert!(
            !debug.contains(secret),
            "a secret reached a debug line through a CLI-backed client: {debug}"
        );
    }
}

#[test]
fn completions_come_from_the_endpoint_unless_a_backend_says_otherwise() {
    let client = LlmClient::new(LlmConfig::default());
    assert!(matches!(client.backend(), LlmBackend::Http));
    assert!(matches!(
        client
            .with_backend(LlmBackend::cli(AgentKind::Codex, settings()))
            .backend(),
        LlmBackend::Cli {
            agent: AgentKind::Codex,
            ..
        }
    ));
}

#[test]
fn a_turns_toolset_reaches_the_agent_the_backend_spawns() {
    let toolset = Toolset::new(
        "http://127.0.0.1:8421/mcp",
        "zone-turn-notarealtoken",
        ["read_file"],
    );

    let client = LlmClient::new(LlmConfig::default())
        .with_backend(LlmBackend::cli(AgentKind::Claude, settings()))
        .with_toolset(toolset, BuiltinTools::Granted);

    let attached = client.toolset().expect("the turn's toolset");
    assert_eq!(attached.endpoint, "http://127.0.0.1:8421/mcp");
    assert_eq!(attached.tools, ["read_file"]);
    assert_eq!(client.builtin_tools(), BuiltinTools::Granted);
}

/// An HTTP backend spawns nothing to configure and carries its tools in
/// the request itself, so there is nothing here for it to lose.
#[test]
fn a_toolset_leaves_an_http_backend_exactly_as_it_was() {
    let client = LlmClient::new(LlmConfig::default()).with_toolset(
        Toolset::new("http://127.0.0.1:8421/mcp", "token", ["read_file"]),
        BuiltinTools::Granted,
    );

    assert!(matches!(client.backend(), LlmBackend::Http));
    assert!(client.toolset().is_none());
    assert_eq!(client.builtin_tools(), BuiltinTools::Withheld);
}

#[derive(Debug)]
struct Watching;

impl Observer for Watching {
    fn observe(&self, _output: &Output) {}
}

/// The builder replaces what a turn decides and nothing else: the executable
/// the operator configured is still the one that runs.
#[test]
fn with_backend_replaces_the_backend_and_drops_its_toolset() {
    let client = LlmClient::new(LlmConfig::default())
        .with_backend(LlmBackend::cli(AgentKind::Claude, settings()))
        .with_toolset(
            Toolset::new(
                "http://127.0.0.1:8421/mcp",
                "zone-turn-notarealtoken",
                ["read_file"],
            ),
            BuiltinTools::Granted,
        )
        .with_observer(Arc::new(Watching))
        .with_temperature(0.25);
    assert!(client.observer().is_some());

    let moved = client.with_backend(LlmBackend::cli(
        AgentKind::Codex,
        settings().with_timeout(Duration::from_secs(42)),
    ));

    let LlmBackend::Cli {
        agent, settings, ..
    } = moved.backend()
    else {
        panic!("the backend stayed on the endpoint");
    };
    assert_eq!(*agent, AgentKind::Codex);
    assert!(
        moved.toolset().is_none(),
        "the old agent's toolset reached the new one"
    );
    assert!(moved.observer().is_none());
    assert_eq!(moved.builtin_tools(), BuiltinTools::Withheld);
    assert_eq!(settings.timeout, Duration::from_secs(42));
    assert_eq!(moved.config().temperature, 0.25);
}

#[test]
fn attaching_a_toolset_keeps_the_backends_settings() {
    let client = LlmClient::new(LlmConfig::default())
        .with_backend(LlmBackend::cli(
            AgentKind::Claude,
            settings().with_executable("/opt/bin/claude"),
        ))
        .with_toolset(
            Toolset::new("http://127.0.0.1:8421/mcp", "token", ["read_file"]),
            BuiltinTools::Granted,
        );

    let LlmBackend::Cli { settings, .. } = client.backend() else {
        panic!("the backend stayed on the endpoint");
    };
    assert_eq!(
        settings.executable,
        Some(std::path::PathBuf::from("/opt/bin/claude"))
    );
    assert_eq!(client.builtin_tools(), BuiltinTools::Granted);
}

#[test]
fn a_backend_runs_on_the_servers_sign_in_until_told_whose() {
    let backend = LlmBackend::cli(AgentKind::Claude, settings());
    assert_eq!(backend.sign_in(), Some(SignIn::Instance));
    assert_eq!(LlmBackend::Http.sign_in(), None);

    for sign_in in [SignIn::Organization, SignIn::Host] {
        assert_eq!(
            backend.clone().with_sign_in(sign_in).sign_in(),
            Some(sign_in)
        );
    }
    assert_eq!(LlmBackend::Http.with_sign_in(SignIn::Host).sign_in(), None);
}

/// Bounding a backend, moving it into a directory or setting its effort
/// changes its settings alone, so whose sign-in it runs under survives.
#[test]
fn configuring_a_backend_keeps_its_agent_and_its_sign_in() {
    let configured = LlmBackend::cli(AgentKind::Codex, settings())
        .with_sign_in(SignIn::Organization)
        .configured(|settings| settings.with_working_directory("/w"));

    let LlmBackend::Cli {
        agent,
        settings,
        sign_in,
    } = &configured
    else {
        panic!("the backend stayed on the endpoint");
    };
    assert_eq!(*agent, AgentKind::Codex);
    assert_eq!(*sign_in, SignIn::Organization);
    assert_eq!(
        settings.working_directory,
        Some(std::path::PathBuf::from("/w"))
    );
    assert!(matches!(
        LlmBackend::Http.configured(|settings| settings.with_executable("/x")),
        LlmBackend::Http
    ));
}

#[tokio::test]
async fn an_agents_successful_ending_becomes_the_one_the_loop_accepts() {
    let events = agent_events(vec![
        Ok(AgentEvent::Text("The suite passes.".to_string())),
        Ok(AgentEvent::Usage(Usage::new(40, 8))),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a finished turn failed: {failure:?}");
    assert_eq!(spoken(&delivered), "The suite passes.");
    assert_eq!(
        delivered
            .iter()
            .find_map(|chunk| chunk.usage.as_ref())
            .map(|usage| usage.total_tokens),
        Some(48)
    );
    assert_eq!(
        reasons(&delivered),
        [STOP],
        "the agent's own word would spin the loop to its iteration limit"
    );
}

#[tokio::test]
async fn an_agents_tool_use_is_shown_as_reasoning_rather_than_replayed() {
    let events = agent_events(vec![
        Ok(AgentEvent::Tool(ToolCall::function(
            "toolu_01",
            "Read",
            r#"{"file_path":"/w/a.rs"}"#,
        ))),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a finished turn failed: {failure:?}");
    assert!(
        reasoned(&delivered).contains("Read"),
        "the reader was shown nothing for the work the agent did"
    );
    assert!(
        delivered.iter().all(|chunk| chunk
            .choices
            .iter()
            .all(|choice| choice.delta.tool_calls.is_none())),
        "zone would run the agent's own tool a second time"
    );
}

#[tokio::test]
async fn an_agents_own_tools_are_listed_and_zones_are_left_to_their_cards() {
    let events = agent_events(vec![
        Ok(AgentEvent::Tool(ToolCall::function(
            "toolu_01",
            "Bash",
            r#"{"command":"git status"}"#,
        ))),
        Ok(AgentEvent::Tool(ToolCall::function(
            "toolu_02",
            "mcp__zone__list_sources",
            "{}",
        ))),
        Ok(AgentEvent::Tool(ToolCall::function(
            "toolu_03",
            "ToolSearch",
            r#"{"query":"select:Read"}"#,
        ))),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a finished turn failed: {failure:?}");
    assert_eq!(
        reasoned(&delivered),
        "- Bash `git status`\n- ToolSearch `select:Read`\n"
    );
}

#[tokio::test]
async fn a_host_agents_answer_keeps_its_blocks_apart() {
    let directory = TempDir::new().expect("a temporary directory");
    let script = [
        r#"echo '{"type":"assistant","message":{"content":[{"type":"text","text":"I will look first."}]}}'"#,
        r#"echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"mcp__zone__list_files","input":{"path":"."}}]}}'"#,
        r#"echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Found it."}]}}'"#,
        r#"echo '{"type":"result","subtype":"success","is_error":false}'"#,
    ]
    .join("\n");
    let client = agent_client(fake(&directory, &script));

    let stream = stream_turn(&client, None).await.expect("a stream");
    let (delivered, failure) = collected(stream).await;

    assert!(failure.is_none(), "a finished turn failed: {failure:?}");
    assert_eq!(spoken(&delivered), "I will look first.\n\nFound it.");
    assert_eq!(reasoned(&delivered), "");
}

#[tokio::test]
async fn a_refusing_agent_ends_the_stream_in_its_own_words() {
    let events = agent_events(vec![Err(ProviderError::agent("claude", REFUSAL))]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    let failure = failure.expect("a refused turn to fail");
    assert!(
        failure.to_string().contains(REFUSAL),
        "lost the agent's wording: {failure}"
    );
    assert!(
        delivered.is_empty(),
        "a refusal was delivered as an answer: {delivered:?}"
    );
}

#[tokio::test]
async fn a_limited_agent_stream_ends_in_a_limited_error_not_an_agent_one() {
    let events = agent_events(vec![
        Ok(AgentEvent::Text("Halfway there.".to_string())),
        Err(ProviderError::limited("claude", session_limit())),
        Ok(AgentEvent::Text("never read".to_string())),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert_eq!(spoken(&delivered), "Halfway there.");
    let failure = failure.expect("a limited turn to fail");
    let Error::Limited {
        provider, limit, ..
    } = &failure
    else {
        panic!("expected a limit, got {failure:?}");
    };
    assert_eq!(provider, "claude");
    assert_eq!(**limit, session_limit());
}

#[tokio::test]
async fn a_window_rides_the_chunk_it_arrived_on() {
    let events = agent_events(vec![
        Ok(AgentEvent::Window(five_hours_at(43.0))),
        Ok(AgentEvent::Text("Done.".to_string())),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a window failed the turn: {failure:?}");
    let windows: Vec<Option<&Window>> = delivered
        .iter()
        .map(|chunk| chunk.window.as_ref())
        .collect();
    assert_eq!(windows, [Some(&five_hours_at(43.0)), None, None]);
    assert_eq!(spoken(&delivered), "Done.");
    assert_eq!(reasons(&delivered), [STOP]);
}

#[tokio::test]
async fn the_context_an_agent_reports_is_kept_on_the_client_not_chunked() {
    let events = agent_events(vec![
        Ok(AgentEvent::Text("Done.".to_string())),
        Ok(AgentEvent::finished(Some("success".to_string()))),
        Ok(AgentEvent::Occupancy(occupancy())),
    ]);
    let reported = Reported::default();

    let (delivered, failure) = collected(turn(reported.clone()).chunks(events)).await;

    assert!(failure.is_none(), "a report failed the turn: {failure:?}");
    assert_eq!(delivered.len(), 2, "no chunk carries the report");
    assert_eq!(reported.latest(), Some((AgentKind::Claude, occupancy())));
}

#[tokio::test]
async fn news_of_a_subagent_never_reaches_the_reader_as_a_chunk() {
    let events = agent_events(vec![
        Ok(AgentEvent::Subagent(
            Subagent::new("toolu_01Agent")
                .with_description("Ask for a review")
                .with_status(SubagentStatus::Running),
        )),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a subagent failed the turn: {failure:?}");
    assert_eq!(
        delivered.len(),
        1,
        "only the finish is chunked: {delivered:?}"
    );
    assert_eq!(spoken(&delivered), "");
    assert_eq!(reasoned(&delivered), "");
    assert_eq!(reasons(&delivered), [STOP]);
}

fn step(name: &str, subject: Option<&str>, status: StepStatus) -> AgentEvent {
    let step = SubagentStep::new(format!("toolu_{name}"), name, status);
    let step = match subject {
        Some(subject) => step.with_subject(subject),
        None => step,
    };
    AgentEvent::Subagent(Subagent::new("toolu_01Agent").with_step(step))
}

#[tokio::test]
async fn a_subagents_call_just_made_is_reasoned_as_the_main_agents_are() {
    let events = agent_events(vec![
        Ok(step("Grep", Some("pub mod"), StepStatus::Running)),
        Ok(step("Grep", Some("pub mod"), StepStatus::Succeeded)),
        Ok(step("TodoWrite", None, StepStatus::Running)),
        Ok(step("Read", Some("/w/a.rs"), StepStatus::Failed)),
        Ok(step(
            "mcp__zone__search_code",
            Some("x"),
            StepStatus::Running,
        )),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a subagent failed the turn: {failure:?}");
    assert_eq!(delivered.len(), 3, "{delivered:?}");
    assert_eq!(reasoned(&delivered), "- Grep `pub mod`\n- TodoWrite\n");
    assert_eq!(spoken(&delivered), "");
    assert_eq!(reasons(&delivered), [STOP]);
}

/// A task run sets no observer, so a subagent's own calls are listed in the
/// turn's reasoning as the main agent's are, rather than lost.
#[tokio::test]
async fn a_subagents_calls_are_reasoned_when_nothing_observes_the_turn() {
    const STREAM: &str = include_str!("fixtures/claude/subagents.jsonl");
    let directory = TempDir::new().expect("a temporary directory");
    let replay = directory.path().join("stream.jsonl");
    std::fs::write(&replay, STREAM).expect("the replayed stream");
    let client = agent_client(fake(&directory, &format!("cat '{}'", replay.display())));

    let stream = stream_turn(&client, None).await.expect("a running agent");
    let (delivered, failure) = collected(stream).await;

    assert!(failure.is_none(), "the turn failed: {failure:?}");
    assert_eq!(
        reasoned(&delivered),
        [
            "- Agent `Survey the crate`\n",
            "- Grep `pub mod`\n",
            "- Read `/w/src/missing.rs`\n",
            "- Agent `Review the change`\n",
            "- Bash `cargo build`\n",
            "- Bash `cargo test -p zone_core`\n",
        ]
        .concat()
    );
}

#[test]
fn a_client_reports_only_what_the_agent_it_now_runs_on_reported() {
    let occupancy = occupancy();
    let client = LlmClient::new(LlmConfig::new("http://localhost:4000", "sonnet", "key"))
        .with_backend(LlmBackend::cli(AgentKind::Claude, settings()));
    client.reported.record(AgentKind::Claude, occupancy);
    assert_eq!(client.occupancy(), Some(occupancy));

    let handed = client.with_backend(LlmBackend::cli(AgentKind::Codex, settings()));
    assert_eq!(
        handed.occupancy(),
        None,
        "claude's report was kept as codex's after the turn handed over"
    );

    let http = handed.with_backend(LlmBackend::Http);
    assert_eq!(http.occupancy(), None);
}

#[tokio::test]
async fn the_session_an_agent_announces_rides_the_chunk_it_arrived_on() {
    let events = agent_events(vec![
        Ok(AgentEvent::Session("6f1".to_string())),
        Ok(AgentEvent::Text("Done.".to_string())),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "a session failed the turn: {failure:?}");
    let sessions: Vec<Option<&str>> = delivered
        .iter()
        .map(|chunk| chunk.session.as_deref())
        .collect();
    assert_eq!(sessions, [Some("6f1"), None, None]);
    assert_eq!(spoken(&delivered), "Done.");
    assert_eq!(reasons(&delivered), [STOP]);
}

#[tokio::test]
async fn a_host_agent_answers_a_streaming_turn() {
    let directory = TempDir::new().expect("a temporary directory");
    let script = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Looking now. "}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/w/a.rs"}}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"It is empty."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":9,"cache_read_input_tokens":1600,"output_tokens":24}}'
"#;
    let client = agent_client(fake(&directory, script));

    let stream = stream_turn(&client, None).await.expect("a running agent");
    let (delivered, failure) = collected(stream).await;

    assert!(failure.is_none(), "the turn failed: {failure:?}");
    assert_eq!(spoken(&delivered), "Looking now. \n\nIt is empty.");
    assert!(reasoned(&delivered).contains("Read"));
    assert_eq!(reasons(&delivered), [STOP]);
    assert_eq!(
        delivered
            .iter()
            .find_map(|chunk| chunk.usage.as_ref())
            .map(|usage| usage.prompt_tokens),
        Some(9 + 1600)
    );
}

#[tokio::test]
async fn a_signed_out_host_agent_fails_the_turn_rather_than_answering_it() {
    let directory = TempDir::new().expect("a temporary directory");
    let client = agent_client(fake(&directory, &format!("printf '%s\\n' '{SIGNED_OUT}'")));

    let stream = stream_turn(&client, None).await.expect("a running agent");
    let (delivered, failure) = collected(stream).await;

    let failure = failure.expect("a signed-out agent to fail the turn");
    assert!(
        failure.to_string().contains(REFUSAL),
        "lost the agent's wording: {failure}"
    );
    assert!(
        !spoken(&delivered).contains("Not logged in"),
        "the refusal was streamed as the answer"
    );
    assert!(
        reasons(&delivered).is_empty(),
        "a refused turn was ended as a finished one"
    );
}

#[tokio::test]
async fn a_refusal_spoken_before_it_is_declared_still_fails_the_turn() {
    let directory = TempDir::new().expect("a temporary directory");
    let client = agent_client(fake(
        &directory,
        &format!("printf '%s\\n' '{SIGNED_OUT_ASSISTANT}' '{SIGNED_OUT}'"),
    ));

    let stream = stream_turn(&client, None).await.expect("a running agent");
    let (delivered, failure) = collected(stream).await;

    let failure = failure.expect("a signed-out agent to fail the turn");
    assert!(
        failure.to_string().contains(REFUSAL),
        "lost the agent's wording: {failure}"
    );
    assert!(
        reasons(&delivered).is_empty(),
        "a turn that spoke before it failed was still ended as a finished one"
    );
}

#[tokio::test]
async fn tools_offered_to_a_cli_backend_are_refused_rather_than_dropped() {
    let directory = TempDir::new().expect("a temporary directory");
    let client = agent_client(fake(&directory, "exit 1"));
    let tools = [ToolDefinition::function(
        "read_file",
        "Read a file",
        serde_json::json!({}),
    )];

    let Err(streaming) = stream_turn(&client, Some(&tools)).await else {
        panic!("a streaming turn offered zone's tools to an agent that cannot call them");
    };
    let buffered = client
        .chat_with_options(
            "sonnet",
            &[Message::user("hi")],
            Some(&tools),
            RequestOptions::new(512),
        )
        .await
        .expect_err("a refusal");

    for error in [streaming, buffered] {
        assert!(
            matches!(error, Error::InvalidConfig(_)),
            "expected a rejected request, got {error:?}"
        );
        assert!(
            error.to_string().contains("read_file"),
            "the refusal has to name the tools it could not serve: {error}"
        );
    }
}

#[tokio::test]
async fn a_buffered_turn_reaches_the_host_agent_too() {
    let directory = TempDir::new().expect("a temporary directory");
    let script = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Two."}]}}'
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
    let client = agent_client(fake(&directory, script));

    let response = client
        .chat_with_options(
            "sonnet",
            &[Message::user("one plus one?")],
            None,
            RequestOptions::new(512),
        )
        .await
        .expect("an answer");

    let choice = response.choices.first().expect("one choice");
    assert_eq!(choice.message.content.as_deref(), Some("Two."));
    assert_eq!(choice.finish_reason.as_deref(), Some(STOP));
}

#[tokio::test]
async fn an_answer_after_streamed_prose_is_not_spoken_twice() {
    let events = agent_events(vec![
        Ok(AgentEvent::Text("It is empty.".to_string())),
        Ok(AgentEvent::Answer("It is empty.".to_string())),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "It is empty.");
}

#[tokio::test]
async fn an_answer_without_streamed_prose_is_spoken() {
    let events = agent_events(vec![
        Ok(AgentEvent::Answer("It is empty.".to_string())),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);

    let (delivered, failure) = collected(turn(Reported::default()).chunks(events)).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(spoken(&delivered), "It is empty.");
}

#[derive(Debug, Default)]
struct Recording {
    outputs: Mutex<Vec<String>>,
    subagents: Mutex<Vec<String>>,
}

impl Observer for Recording {
    fn observe(&self, output: &Output) {
        self.outputs
            .lock()
            .expect("outputs")
            .push(output.text.clone());
    }

    fn track(&self, subagent: &Subagent) {
        self.subagents
            .lock()
            .expect("subagents")
            .push(subagent.id.clone());
    }
}

#[tokio::test]
async fn an_observer_is_told_of_outputs_and_subagents_rather_than_the_reader() {
    let recording = Arc::new(Recording::default());
    let events = agent_events(vec![
        Ok(AgentEvent::Output(Output::new(
            ToolCall::function("toolu_01", "Bash", r#"{"command":"ls"}"#),
            "a.rs",
            true,
        ))),
        Ok(step("Grep", Some("pub mod"), StepStatus::Running)),
        Ok(AgentEvent::finished(Some("success".to_string()))),
    ]);
    let observed = Turn::new(
        AgentKind::Claude,
        SignIn::Instance,
        None,
        Some(recording.clone()),
        Reported::default(),
    );

    let (delivered, failure) = collected(observed.chunks(events)).await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(reasoned(&delivered), "");
    assert_eq!(*recording.outputs.lock().expect("outputs"), ["a.rs"]);
    assert_eq!(
        *recording.subagents.lock().expect("subagents"),
        ["toolu_01Agent"]
    );
}

#[tokio::test]
async fn a_limit_ends_the_stream_on_zones_name_for_its_window() {
    let refused = Window::new(WindowKind::FiveHours.streamed()).with_kind(WindowKind::FiveHours);
    let events = agent_events(vec![Err(ProviderError::limited(
        "claude",
        session_limit().with_window(refused),
    ))]);

    let (_, failure) = collected(turn(Reported::default()).chunks(events)).await;

    let Some(Error::Limited { limit, .. }) = failure else {
        panic!("expected a limit, got {failure:?}");
    };
    assert_eq!(
        limit.window.map(|window| window.name).as_deref(),
        Some(FIVE_HOURS)
    );
}

/// How long a stand-in's turn may run before the test gives up on it.
const PATIENCE: Duration = Duration::from_secs(20);

/// Set on the copy of this binary a test re-runs itself in, naming the test.
const COPY: &str = "ZONE_CORE_CLIENT_COPY";

/// A Claude stand-in's reply to a turn that goes as planned.
const CLAUDE_ANSWER: &str = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}'
echo '{"type":"result","subtype":"success","is_error":false}'
"#;

/// A Codex stand-in's reply to a turn that goes as planned.
const CODEX_ANSWER: &str = r#"
echo '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Done."}}'
echo '{"type":"turn.completed","usage":{"input_tokens":4,"output_tokens":1}}'
"#;

/// The model each agent is asked for, one it knows.
fn model(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => "gpt-5.5",
        _ => "sonnet",
    }
}

/// A stand-in for `agent` that answers its turn as planned.
fn answering(agent: AgentKind) -> StandIn {
    StandIn::new(match agent {
        AgentKind::Codex => CODEX_ANSWER,
        _ => CLAUDE_ANSWER,
    })
}

/// Zone's settings for a run of `stand_in`, which may take [`PATIENCE`].
fn standing_in(stand_in: &StandIn) -> CliSettings {
    settings()
        .with_executable(stand_in.executable())
        .with_timeout(PATIENCE)
}

/// A client that runs `agent` under `settings`.
fn running(agent: AgentKind, settings: CliSettings) -> LlmClient {
    LlmClient::new(LlmConfig::default()).with_backend(LlmBackend::cli(agent, settings))
}

/// Every chunk of a turn on `model` asking `messages`, and the error it
/// ended in, whether the stream failed to open or failed on the way.
async fn streamed(
    client: &LlmClient,
    model: &str,
    messages: &[Message],
) -> (Vec<ChatStreamChunk>, Option<Error>) {
    match client
        .chat_stream_with_options(model, messages, None, RequestOptions::new(512))
        .await
    {
        Ok(stream) => collected(stream).await,
        Err(error) => (Vec::new(), Some(error)),
    }
}

/// A turn on the agent's own [`model`] asking what a file does.
async fn asked(client: &LlmClient, agent: AgentKind) -> (Vec<ChatStreamChunk>, Option<Error>) {
    streamed(client, model(agent), &[Message::user("What does a.rs do?")]).await
}

/// Re-run `test` of `module` in a copy of this binary whose environment
/// also holds `environment`, unless this is that copy. Returns whether this
/// call was the original, which has nothing left to do once the copy passed.
///
/// Setting the variables in this process would race every other test that
/// starts a child.
async fn delegated(module: &str, test: &str, environment: &[(&str, &str)]) -> bool {
    let module = module.split_once("::").map_or(module, |(_, path)| path);
    let test = format!("{module}::{test}");
    if std::env::var(COPY).as_deref() == Ok(test.as_str()) {
        return false;
    }
    let output = tokio::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", &test, "--nocapture"])
        .env(COPY, &test)
        .env_remove(crate::llm::cli::PASSTHROUGH)
        .envs(environment.iter().copied())
        .output()
        .await
        .expect("a copy of this test binary");
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(output.status.success(), "{printed}");
    assert!(
        printed.contains("1 passed"),
        "the copy ran no test: {printed}"
    );
    true
}

/// The `-c` override attaching zone's MCP server among Codex's `arguments`.
fn zones_server(arguments: &[String]) -> String {
    let attached = format!("mcp_servers.{}=", Toolset::SERVER);
    arguments
        .windows(2)
        .find(|pair| pair[0] == "-c" && pair[1].starts_with(&attached))
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| panic!("zone's server among codex's overrides: {arguments:?}"))
}
