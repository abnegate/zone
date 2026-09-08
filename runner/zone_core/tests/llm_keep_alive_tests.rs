use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::{LlmClient, LlmConfig, Message};

const RACERS: usize = 8;
const TURNS: usize = 12;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// One completion on its own runtime, as a chat turn gets in the server.
fn turn(base_url: &str) {
    runtime().block_on(async {
        LlmClient::new(LlmConfig {
            base_url: base_url.to_string(),
            default_model: "test".to_string(),
            ..LlmConfig::default()
        })
        .chat(&[Message::user("hi")], None)
        .await
        .expect("a turn reaches the server")
    });
}

/// Completions share one process-wide connection pool, but every pooled
/// connection is driven by a task belonging to the runtime that opened it.
/// Once that runtime is gone the pool can still hand the connection out, and
/// the send fails with "runtime dropped the dispatch task" without ever
/// reaching the server.
#[test]
fn a_pooled_connection_outliving_its_runtime_does_not_fail_the_next_turn() {
    let host = runtime();
    let server = host.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": "test",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hi"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        server
    });

    let racers: Vec<_> = (0..RACERS)
        .map(|_| {
            let base_url = server.uri();
            std::thread::spawn(move || {
                for _ in 0..TURNS {
                    turn(&base_url);
                }
            })
        })
        .collect();

    for racer in racers {
        racer
            .join()
            .expect("no turn is handed a dead pooled connection");
    }
}
