//! A ComfyUI stand-in whose prompts never leave its queue.

use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// Keeps `prompt` running for as long as the test lasts: its history stays
/// empty, and the queue the client confirms an empty history against still
/// lists it.
pub async fn queued(server: &MockServer, prompt: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/history/{prompt}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/queue"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "queue_running": [[0, prompt, {}, {}, []]],
            "queue_pending": [],
        })))
        .mount(server)
        .await;
}
