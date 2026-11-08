//! How a turn's images reach the agent, and what happens when one cannot.

use serde_json::Value;

use super::*;
use crate::llm::cli::prompt;

const PNG: &str = "iVBORw0KGgo=";

/// An image no agent can be shown: a path on zone's own server.
const UNREACHABLE: &str = "/api/artifacts/w/c/m/generated.png";

fn with_images(message: Message, images: &[&str]) -> Message {
    let mut message = message;
    message.images = images.iter().map(|url| (*url).to_string()).collect();
    message
}

fn reads_stream_json(arguments: &[String]) -> bool {
    arguments
        .windows(2)
        .any(|pair| pair == ["--input-format", "stream-json"])
}

#[tokio::test]
async fn a_text_only_claude_turn_is_sent_as_plain_text() {
    let stand_in = answering(AgentKind::Claude);

    let (_, failure) = asked(
        &running(AgentKind::Claude, standing_in(&stand_in)),
        AgentKind::Claude,
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    let arguments = stand_in.arguments();
    assert!(
        !arguments
            .iter()
            .any(|argument| argument == "--input-format"),
        "{arguments:?}"
    );
    let stdin = stand_in.stdin();
    assert!(serde_json::from_str::<Value>(&stdin).is_err(), "{stdin}");
    assert_eq!(stdin, "User:\nWhat does a.rs do?");
}

#[tokio::test]
async fn an_image_turn_is_sent_as_stream_json() {
    let stand_in = answering(AgentKind::Claude);
    let png = format!("data:image/png;base64,{PNG}");

    let (_, failure) = streamed(
        &running(AgentKind::Claude, standing_in(&stand_in)),
        "sonnet",
        &[with_images(Message::user("Look."), &[&png])],
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert!(reads_stream_json(&stand_in.arguments()));
    let stdin = stand_in.stdin();
    let message: Value = serde_json::from_str(&stdin).expect("a stream-json message");
    assert_eq!(
        message["message"]["content"][1],
        serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": PNG}})
    );
    assert_eq!(prompt(&stdin), "User:\nLook.");
}

#[tokio::test]
async fn an_image_that_cannot_be_sent_fails_the_turn() {
    for agent in AgentKind::ALL.iter().copied() {
        let stand_in = answering(agent);
        let client = running(agent, standing_in(&stand_in));
        let messages = [with_images(Message::user("Look."), &[UNREACHABLE])];

        let (_, streaming) = streamed(&client, model(agent), &messages).await;
        let buffered = client
            .chat_with_options(model(agent), &messages, None, RequestOptions::new(512))
            .await
            .err();

        for failure in [streaming, buffered] {
            let failure = failure.expect("the image to fail the turn");
            assert!(matches!(failure, Error::Agent(_)), "{agent}: {failure:?}");
            let rendered = failure.to_string();
            assert!(rendered.contains("image"), "{agent}: {rendered}");
            assert!(rendered.contains(agent.as_str()), "{agent}: {rendered}");
        }
        assert!(!stand_in.started(), "{agent} was started without its image");
    }
}
