//! What an agent is handed for a turn: what goes to its stdin, and the image
//! files its command line names.

use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::agent::AgentKind;
use super::image::{Image, extension};
use super::transcript::{Part, Transcript};

/// Stands in for an earlier image the agent cannot be shown, so the transcript
/// still says one was there.
const OMITTED: &str = "[An image was attached here, but it cannot be shown to you.]";

const UNSUPPORTED: &str = "only PNG, JPEG, GIF and WebP images can be given to";
const LINKED: &str = "codex can only be given images as files, not links";
const UNREADABLE: &str = "the attached image could not be decoded for";

/// Why an image on the message being answered cannot reach the agent.
#[derive(Debug, thiserror::Error)]
pub enum Undeliverable {
    #[error("{UNSUPPORTED} {0}")]
    Unsupported(AgentKind),
    #[error("{LINKED}")]
    Linked,
    #[error("{UNREADABLE} {0}")]
    Unreadable(AgentKind),
    #[error("the attached image could not be saved for codex: {0}")]
    Unsaved(#[from] std::io::Error),
}

/// A turn's input, ready for the agent.
#[derive(Debug)]
pub struct Input {
    pub stdin: String,
    pub images: Vec<PathBuf>,
    /// Holds the image files until the agent has read them.
    pub directory: Option<TempDir>,
}

impl Input {
    /// Claude reads one stream-json user message whose content blocks carry
    /// the images; codex reads the text on stdin and opens each image from a
    /// file its command line names.
    pub fn new(agent: AgentKind, transcript: &Transcript) -> Result<Self, Undeliverable> {
        match agent {
            AgentKind::Claude => Ok(Self {
                stdin: claude(transcript)?,
                images: Vec::new(),
                directory: None,
            }),
            AgentKind::Codex => codex(transcript),
        }
    }
}

fn claude(transcript: &Transcript) -> Result<String, Undeliverable> {
    let mut content = Vec::new();
    for part in transcript.parts() {
        match part {
            Part::Text(text) => content.push(text_block(text)),
            Part::Image { url, current } => match Image::parse(url) {
                Some(Image::Inline { media_type, data }) => content.push(json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": media_type, "data": data},
                })),
                Some(Image::Linked(url)) => content.push(json!({
                    "type": "image",
                    "source": {"type": "url", "url": url},
                })),
                None if *current => return Err(Undeliverable::Unsupported(AgentKind::Claude)),
                None => content.push(text_block(OMITTED)),
            },
        }
    }
    let message = json!({
        "type": "user",
        "message": {"role": "user", "content": content},
    });
    Ok(format!("{message}\n"))
}

fn text_block(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn codex(transcript: &Transcript) -> Result<Input, Undeliverable> {
    let mut texts: Vec<&str> = Vec::new();
    let mut images = Vec::new();
    let mut directory: Option<TempDir> = None;

    for part in transcript.parts() {
        match part {
            Part::Text(text) => texts.push(text),
            Part::Image { url, current } => match saved(url, &mut directory, images.len()) {
                Ok(path) => images.push(path),
                Err(error) if *current => return Err(error),
                Err(_) => texts.push(OMITTED),
            },
        }
    }

    Ok(Input {
        stdin: texts.join("\n\n"),
        images,
        directory,
    })
}

/// Write the image `url` carries to a file of its own in `directory`.
fn saved(
    url: &str,
    directory: &mut Option<TempDir>,
    index: usize,
) -> Result<PathBuf, Undeliverable> {
    let (media_type, data) = match Image::parse(url) {
        Some(Image::Inline { media_type, data }) => (media_type, data),
        Some(Image::Linked(_)) => return Err(Undeliverable::Linked),
        None => return Err(Undeliverable::Unsupported(AgentKind::Codex)),
    };
    let extension = extension(media_type).ok_or(Undeliverable::Unsupported(AgentKind::Codex))?;
    let bytes = BASE64
        .decode(data)
        .map_err(|_| Undeliverable::Unreadable(AgentKind::Codex))?;
    let directory = match directory {
        Some(directory) => directory,
        None => directory.insert(tempfile::Builder::new().prefix("zone-images-").tempdir()?),
    };
    let path = directory.path().join(format!("{}.{extension}", index + 1));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::transcript::render;
    use abnegate_llm::Message;

    const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";
    const ARTIFACT: &str = "/api/artifacts/w/c/m/generated.png";

    fn with_images(mut message: Message, images: &[&str]) -> Message {
        message.images = images.iter().map(|url| (*url).to_string()).collect();
        message
    }

    fn claude_message(transcript: &Transcript) -> Value {
        let input = Input::new(AgentKind::Claude, transcript).expect("deliverable");
        assert!(
            input.stdin.ends_with('\n'),
            "stream-json is one message a line"
        );
        assert_eq!(input.stdin.trim_end().lines().count(), 1);
        serde_json::from_str(input.stdin.trim_end()).expect("one json message")
    }

    #[test]
    fn claude_is_shown_the_images_a_message_was_sent_with() {
        let transcript = render(&[
            Message::system("Be terse."),
            with_images(
                Message::user("here's the comments"),
                &[PNG, "https://example.test/review.webp"],
            ),
        ]);

        let message = claude_message(&transcript);

        assert_eq!(message["type"], "user");
        assert_eq!(message["message"]["role"], "user");
        assert_eq!(
            message["message"]["content"],
            json!([
                {"type": "text", "text": "System:\nBe terse.\n\nUser:\nhere's the comments"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}},
                {"type": "image", "source": {"type": "url", "url": "https://example.test/review.webp"}},
            ])
        );
    }

    #[test]
    fn claude_reads_a_conversation_without_images_as_one_text_block() {
        let message = claude_message(&render(&[Message::user("ping")]));

        assert_eq!(
            message["message"]["content"],
            json!([{"type": "text", "text": "User:\nping"}])
        );
    }

    #[test]
    fn an_earlier_image_the_agent_cannot_be_shown_is_noted_rather_than_failing_the_turn() {
        let transcript = render(&[
            with_images(Message::assistant("Here it is."), &[ARTIFACT]),
            Message::user("Thanks."),
        ]);

        let message = claude_message(&transcript);
        assert_eq!(
            message["message"]["content"],
            json!([
                {"type": "text", "text": "Assistant:\nHere it is."},
                {"type": "text", "text": OMITTED},
                {"type": "text", "text": "User:\nThanks."},
            ])
        );

        let input = Input::new(AgentKind::Codex, &transcript).expect("deliverable");
        assert!(input.images.is_empty());
        assert_eq!(
            input.stdin,
            format!("Assistant:\nHere it is.\n\n{OMITTED}\n\nUser:\nThanks.")
        );
    }

    #[test]
    fn an_image_this_turn_was_sent_with_that_cannot_be_shown_fails_the_turn() {
        let transcript = render(&[with_images(Message::user("Look."), &[ARTIFACT])]);

        for agent in AgentKind::ALL {
            let error = Input::new(agent, &transcript).expect_err("undeliverable");
            assert!(
                error.to_string().contains(agent.as_str()),
                "the error names the agent: {error}"
            );
        }
    }

    #[test]
    fn codex_is_handed_each_image_as_a_file_and_the_text_on_stdin() {
        let transcript = render(&[with_images(
            Message::user("here's the comments"),
            &[PNG, "data:image/jpeg;base64,/9j/4AAQ"],
        )]);

        let input = Input::new(AgentKind::Codex, &transcript).expect("deliverable");

        assert_eq!(input.stdin, "User:\nhere's the comments");
        assert_eq!(input.images.len(), 2);
        assert!(input.images[0].extension().is_some_and(|e| e == "png"));
        assert!(input.images[1].extension().is_some_and(|e| e == "jpg"));
        assert_eq!(
            std::fs::read(&input.images[0]).expect("saved"),
            BASE64.decode("iVBORw0KGgo=").expect("base64")
        );
        let directory = input.directory.expect("the files are kept");
        assert!(
            input
                .images
                .iter()
                .all(|path| path.starts_with(directory.path()))
        );
    }

    #[test]
    fn codex_cannot_be_given_a_linked_image() {
        let transcript = render(&[with_images(
            Message::user("Look."),
            &["https://example.test/a.png"],
        )]);

        assert!(matches!(
            Input::new(AgentKind::Codex, &transcript),
            Err(Undeliverable::Linked)
        ));
    }

    #[test]
    fn a_conversation_without_images_writes_no_files() {
        let input =
            Input::new(AgentKind::Codex, &render(&[Message::user("ping")])).expect("deliverable");

        assert_eq!(input.stdin, "User:\nping");
        assert!(input.images.is_empty());
        assert!(input.directory.is_none());
    }
}
