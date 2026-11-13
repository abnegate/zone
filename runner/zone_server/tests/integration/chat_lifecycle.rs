//! Generation ownership through the real authenticated router.

use crate::common::MEDIA;
use crate::common::comfy;
use crate::common::context::{Harness, Socket, answer, finish, next, send, successful};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
use zone_server::services::stages::AUTO;

/// A turn that fails before it reaches inference, here on a saved endpoint
/// with no model to run, ends with an error frame and gives the chat back:
/// another owner can take the chat, and the next turn, once a model is set,
/// answers.
#[tokio::test]
async fn an_error_before_inference_releases_the_chat_for_the_next_turn() {
    const UNANSWERED: &str = "http://127.0.0.1:9";

    let harness = Harness::new(Some(32768), false, vec![]).await;
    let organization: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id = $1")
            .bind(harness.workspace)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO organization_ai_settings \
         (organization_id, provider, litellm_host, completions_routed) \
         VALUES ($1, $2, $3, true)",
    )
    .bind(organization)
    .bind(PROVIDER_SELF_HOSTED)
    .bind(UNANSWERED)
    .execute(&harness.pool)
    .await
    .unwrap();
    let requested: String = sqlx::query_scalar("SELECT model_name FROM chats WHERE id = $1")
        .bind(harness.chat)
        .fetch_one(&harness.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chats SET model_name = $2 WHERE id = $1")
        .bind(harness.chat)
        .bind(AUTO)
        .execute(&harness.pool)
        .await
        .unwrap();

    let frames = harness.turn("A short ordinary question").await;
    let terminal = frames.last().unwrap();
    assert_eq!(terminal["type"], "error", "{frames:?}");
    assert!(
        terminal["message"]
            .as_str()
            .unwrap()
            .contains("No model is set"),
        "{terminal}"
    );
    assert!(harness.requests().await.is_empty());
    let lease = harness
        .store()
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .expect("a terminal error must release persistent ownership");
    harness.store().release(&lease).await.unwrap();

    sqlx::query("UPDATE chats SET model_name = $2 WHERE id = $1")
        .bind(harness.chat)
        .bind(requested)
        .execute(&harness.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM organization_ai_settings WHERE organization_id = $1")
        .bind(organization)
        .execute(&harness.pool)
        .await
        .unwrap();
    harness.script.push(answer("Recovered"));
    successful(&harness.turn("Please continue").await);
}

#[tokio::test]
async fn image_wait_releases_ownership_before_media_capacity_returns() {
    lost_media_wait("image").await;
}

#[tokio::test]
async fn video_wait_releases_ownership_before_media_capacity_returns() {
    lost_media_wait("video").await;
}

#[tokio::test]
async fn audio_wait_releases_ownership_before_media_capacity_returns() {
    lost_media_wait("audio").await;
}

async fn status(socket: &mut Socket, expected: &str) {
    loop {
        let frame = next(socket).await;
        if frame["type"] == "status" && frame["message"] == expected {
            return;
        }
        assert!(
            !matches!(
                frame["type"].as_str(),
                Some("error" | "cancelled" | "message_end")
            ),
            "expected {expected}: {frame}"
        );
    }
}

async fn lost_media_wait(lane: &str) {
    let _serial = MEDIA.lock().await;
    let occupied = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":"occupied"})))
        .expect(1)
        .mount(&occupied)
        .await;
    comfy::queued(&occupied, "occupied").await;
    Mock::given(method("POST"))
        .and(path("/queue"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&occupied)
        .await;
    let mut holder = Harness::new(Some(32768), false, vec![]).await;
    holder.config.comfyui.enabled = true;
    holder.config.comfyui.base_url = occupied.uri();
    holder.restart().await;
    let mut holding = holder.connect().await;
    send(
        &mut holding,
        json!({"type":"send","content":"Hold media capacity", "metadata":{"image_generation":true}}),
    )
    .await;
    status(&mut holding, "Image queued...").await;

    let unused = MockServer::start().await;
    let mut waiter =
        Harness::new(Some(32768), false, vec![answer("The next response works")]).await;
    waiter.config.comfyui.enabled = true;
    waiter.config.comfyui.base_url = unused.uri();
    waiter.restart().await;
    let mut waiting = waiter.connect().await;
    send(
        &mut waiting,
        json!({"type":"send","content":"Waiting generation", "metadata":{format!("{lane}_generation"):true}}),
    )
    .await;
    status(&mut waiting, &format!("Preparing {lane} generation...")).await;

    sqlx::query(
        "UPDATE chat_leases SET expires_at = clock_timestamp() - interval '1 second' WHERE chat_id = $1",
    )
    .bind(waiter.chat)
    .execute(&waiter.pool)
    .await
    .unwrap();
    let replacement = waiter
        .store()
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap();
    let frames = finish(&mut waiting).await;
    let terminal = frames.last().unwrap();
    assert_eq!(terminal["type"], "error", "{frames:?}");
    assert!(terminal["message"].as_str().unwrap().contains("ownership"));
    waiter.store().assert_current(&replacement).await.unwrap();
    waiter.store().release(&replacement).await.unwrap();
    send(
        &mut waiting,
        json!({"type":"send","content":"Please continue with text", "metadata":{"image_generation":false,"video_generation":false,"audio_generation":false}}),
    )
    .await;
    let followup = finish(&mut waiting).await;
    let submitted = unused.received_requests().await.unwrap();
    send(&mut holding, json!({"type":"cancel"})).await;
    let cleanup = finish(&mut holding).await;
    assert_eq!(cleanup.last().unwrap()["type"], "cancelled", "{cleanup:?}");
    assert!(
        submitted.is_empty(),
        "the fenced {lane} request must never reach ComfyUI: {submitted:?}"
    );
    successful(&followup);
}
