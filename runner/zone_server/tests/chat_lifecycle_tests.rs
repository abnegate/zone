//! Generation ownership and deadline acceptance through the real authenticated router.
mod common;

use common::context::{Harness, Socket, answer, finish, next, send, successful};
use serde_json::json;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zone_server::services::chat::session::Settings;

static MEDIA: Mutex<()> = Mutex::const_new(());

#[test]
fn settings_validate_deadline_range() {
    const CHILD: &str = "ZONE_DEADLINE_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let seconds: u64 = std::env::var("ZONE_CHAT_TIMEOUT_SECONDS")
            .unwrap()
            .parse()
            .unwrap();
        let timeout = Duration::from_secs(seconds);
        let settings = Settings::from_env();
        if Instant::now().checked_add(timeout).is_some() {
            assert_eq!(settings.unwrap().timeout, timeout);
        } else {
            let error = settings.expect_err("an unrepresentable deadline must fail configuration");
            assert!(error.contains("ZONE_CHAT_TIMEOUT_SECONDS"), "{error}");
        }
        return;
    }
    for seconds in [1, 1800, 31_536_000, u64::MAX] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "settings_validate_deadline_range", "--nocapture"]);
        for name in [
            "ZONE_CHAT_CONTEXT_TOKENS",
            "ZONE_CHAT_ROUNDS",
            "ZONE_CHAT_CALLS",
            "ZONE_CHAT_OUTPUT_TOKENS",
        ] {
            command.env_remove(name);
        }
        let output = command
            .env(CHILD, "1")
            .env("ZONE_CHAT_TIMEOUT_SECONDS", seconds.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "timeout {seconds}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn unrepresentable_runtime_deadline_terminates_without_inference() {
    let mut harness = Harness::new(Some(32768), false, vec![]).await;
    harness.config.chat.timeout = Duration::from_secs(u64::MAX);
    harness.restart().await;
    let mut socket = harness.connect().await;
    send(
        &mut socket,
        json!({"type":"send","content":"A short ordinary question"}),
    )
    .await;
    let frames = tokio::time::timeout(Duration::from_secs(3), finish(&mut socket))
        .await
        .expect("invalid runtime configuration must terminate without panicking or hanging");
    let terminal = frames.last().unwrap();
    assert_eq!(terminal["type"], "error", "{frames:?}");
    assert!(terminal["message"].as_str().unwrap().contains("deadline"));
    assert!(harness.requests().await.is_empty());
    let lease = harness
        .store()
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .expect("terminal error must release persistent ownership");
    harness.store().release(&lease).await.unwrap();
    harness.config.chat.timeout = Duration::from_secs(1);
    harness.script.push(answer("Recovered"));
    harness.restart().await;
    let frames = tokio::time::timeout(Duration::from_secs(3), harness.turn("Please continue"))
        .await
        .expect("terminal error must also release the per-chat semaphore");
    successful(&frames);
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
    Mock::given(method("GET"))
        .and(path("/history/occupied"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&occupied)
        .await;
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
    let stopped = tokio::time::timeout(Duration::from_secs(15), finish(&mut waiting)).await;

    let followup = if let Ok(frames) = &stopped {
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
        Some(tokio::time::timeout(Duration::from_secs(3), finish(&mut waiting)).await)
    } else {
        None
    };
    let submitted = unused.received_requests().await.unwrap();
    send(&mut holding, json!({"type":"cancel"})).await;
    let cleanup = finish(&mut holding).await;
    assert_eq!(cleanup.last().unwrap()["type"], "cancelled");
    assert!(
        stopped.is_ok(),
        "{lane} wait retained ownership after lease loss while media capacity remained occupied"
    );
    assert!(
        submitted.is_empty(),
        "the fenced {lane} request must never reach ComfyUI: {submitted:?}"
    );
    successful(
        &followup.unwrap().expect(
            "lost generation must release the per-chat permit before media capacity returns",
        ),
    );
}
