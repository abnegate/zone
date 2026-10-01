//! A chat whose history holds an image keeps working after switching to a model
//! the engine declares text-only: the request carries a note, not the image.
mod common;

use common::context::{Harness, answer, ordinary, successful};
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};
use zone_server::db::chats;
use zone_server::services::chat::session::WITHHELD_IMAGE;

const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";
const IMAGE_PART: &str = "image_url";

async fn illustrated(capabilities: &[&str]) -> Harness {
    let harness = Harness::new(Some(32768), false, vec![answer("Hi there")]).await;
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "capabilities": capabilities,
            "model_info": {"general.architecture":"test","test.context_length":32768}
        })))
        .with_priority(1)
        .mount(&harness.provider)
        .await;
    chats::create_message(&harness.pool, harness.chat, "user", "Draw me a cat", None)
        .await
        .unwrap();
    chats::create_message(
        &harness.pool,
        harness.chat,
        "assistant",
        "Here is your cat.",
        Some(json!({"attachments":[{"name":"cat.png","mime":"image/png","url":PNG}]})),
    )
    .await
    .unwrap();
    harness
}

async fn completion(harness: &Harness) -> Value {
    let frames = harness.turn("Hello").await;
    successful(&frames);
    let requests = harness.requests().await;
    let turns = ordinary(&requests);
    assert_eq!(turns.len(), 1, "{requests:?}");
    turns[0].clone()
}

fn image_parts(request: &Value) -> usize {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .filter(|part| part["type"] == IMAGE_PART)
        .count()
}

#[tokio::test]
async fn a_text_only_model_gets_a_note_instead_of_a_historical_image() {
    let harness = illustrated(&["completion", "tools"]).await;
    let request = completion(&harness).await;

    assert_eq!(image_parts(&request), 0, "{request}");
    let body = request.to_string();
    assert!(!body.contains(PNG), "{body}");
    let illustrated = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.starts_with("Here is your cat."))
        })
        .unwrap_or_else(|| panic!("the illustrated reply is still sent: {request}"));
    assert_eq!(
        illustrated["content"],
        format!("Here is your cat.\n\n{WITHHELD_IMAGE}")
    );
    assert!(
        !illustrated["content"]
            .as_str()
            .is_some_and(|content| content.contains("attached")),
        "the assistant's own image must not be described as an attachment: {illustrated}"
    );
    assert_eq!(
        harness
            .history()
            .await
            .entries
            .iter()
            .filter(|entry| entry.message.images == [PNG])
            .count(),
        1,
        "canonical history keeps the image for a later vision model"
    );
}

#[tokio::test]
async fn a_vision_model_still_sees_a_historical_image() {
    let harness = illustrated(&["completion", "vision"]).await;
    let request = completion(&harness).await;

    assert_eq!(image_parts(&request), 1, "{request}");
    assert!(request.to_string().contains(PNG), "{request}");
    assert!(!request.to_string().contains(WITHHELD_IMAGE), "{request}");
}
