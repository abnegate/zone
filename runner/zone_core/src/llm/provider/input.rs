//! What an agent is handed for a turn: what goes to its stdin, and the image
//! files its command line names.

mod limits;
mod undeliverable;

use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::agent::AgentKind;
use super::image::{Image, Rejection};
use super::transcript::{Part, SEPARATOR, Transcript};

pub use limits::Limits;
pub use undeliverable::Undeliverable;

/// Stands in for an earlier image the agent cannot be shown, so the transcript
/// still says one was there.
const OMITTED: &str = "[An image was attached here, but it cannot be shown to you.]";

#[derive(Debug)]
pub struct Input {
    pub stdin: String,
    pub images: Vec<PathBuf>,
    pub directory: Option<TempDir>,
}

impl Input {
    /// Claude reads one stream-json user message whose content blocks carry
    /// the images; codex reads the text on stdin and opens each image from a
    /// file its command line names.
    pub fn new(agent: AgentKind, transcript: &Transcript) -> Result<Self, Undeliverable> {
        match agent {
            AgentKind::Claude => Ok(Self {
                stdin: claude(transcript, Limits::CLAUDE)?,
                images: Vec::new(),
                directory: None,
            }),
            AgentKind::Codex => codex(transcript),
        }
    }
}

/// An earlier image the agent cannot take is noted rather than sent: one the
/// API would refuse fails every turn that resends the history, and a link may
/// have died since it was attached. The newest images are the ones kept within
/// a request's limits.
fn claude(transcript: &Transcript, limits: Limits) -> Result<String, Undeliverable> {
    let parts = transcript.parts();
    let text: usize = parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => text.len(),
            Part::Image { .. } => 0,
        })
        .sum();
    let mut room = limits.request_bytes.saturating_sub(text);
    let mut allowed = limits.images;
    let mut images: Vec<Option<Result<Value, Rejection>>> = vec![None; parts.len()];

    for (index, part) in parts.iter().enumerate().rev() {
        let Part::Image { url, current } = part else {
            continue;
        };
        images[index] = Some(
            Image::parse(url)
                .and_then(|image| claude_block(image, *current, limits.image_bytes))
                .and_then(|(block, size)| {
                    if allowed == 0 {
                        return Err(Rejection::Excess);
                    }
                    if size > room {
                        return Err(Rejection::Crowded);
                    }
                    allowed -= 1;
                    room -= size;
                    Ok(block)
                }),
        );
    }

    let mut content = Vec::with_capacity(parts.len());
    for (part, image) in parts.iter().zip(images) {
        match (part, image) {
            (Part::Text(text), _) => content.push(text_block(text)),
            (_, Some(Ok(block))) => content.push(block),
            (Part::Image { current: true, .. }, Some(Err(rejection))) => {
                return Err(Undeliverable::Refused {
                    agent: AgentKind::Claude,
                    rejection,
                });
            }
            (Part::Image { .. }, _) => content.push(text_block(OMITTED)),
        }
    }

    let message = json!({
        "type": "user",
        "message": {"role": "user", "content": content},
    });
    Ok(format!("{message}\n"))
}

/// The content block showing `image`, with the bytes it adds to a request.
fn claude_block(
    image: Image<'_>,
    current: bool,
    image_bytes: usize,
) -> Result<(Value, usize), Rejection> {
    match image {
        Image::Inline {
            media_type, bytes, ..
        } => {
            let data = BASE64.encode(bytes);
            if data.len() > image_bytes {
                return Err(Rejection::Oversized);
            }
            let size = data.len();
            Ok((
                json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": media_type, "data": data},
                }),
                size,
            ))
        }
        Image::Linked(url) if current => Ok((
            json!({
                "type": "image",
                "source": {"type": "url", "url": url},
            }),
            url.len(),
        )),
        Image::Linked(_) => Err(Rejection::Linked),
    }
}

fn text_block(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

/// Codex attaches every image to the prompt as a whole, so the text marks
/// where each one was sent.
fn codex(transcript: &Transcript) -> Result<Input, Undeliverable> {
    let mut texts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    let mut directory: Option<TempDir> = None;

    for part in transcript.parts() {
        match part {
            Part::Text(text) => texts.push(text.clone()),
            Part::Image { url, current } => match saved(url, &mut directory, images.len()) {
                Ok(path) => {
                    images.push(path);
                    texts.push(format!("[Image {}]", images.len()));
                }
                Err(error) if *current => return Err(error),
                Err(_) => texts.push(OMITTED.to_string()),
            },
        }
    }

    Ok(Input {
        stdin: texts.join(SEPARATOR),
        images,
        directory,
    })
}

fn saved(
    url: &str,
    directory: &mut Option<TempDir>,
    index: usize,
) -> Result<PathBuf, Undeliverable> {
    let refused = |rejection| Undeliverable::Refused {
        agent: AgentKind::Codex,
        rejection,
    };
    let (extension, bytes) = match Image::parse(url).map_err(refused)? {
        Image::Inline {
            extension, bytes, ..
        } => (extension, bytes),
        Image::Linked(_) => return Err(refused(Rejection::Linked)),
    };
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

    const PNG_DATA: &str = "iVBORw0KGgo=";
    const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";
    const ARTIFACT: &str = "/api/artifacts/w/c/m/generated.png";
    const LINK: &str = "https://example.test/review.webp";

    fn with_images(mut message: Message, images: &[&str]) -> Message {
        message.images = images.iter().map(|url| (*url).to_string()).collect();
        message
    }

    fn claude_content(transcript: &Transcript) -> Value {
        let input = Input::new(AgentKind::Claude, transcript).expect("deliverable");
        assert!(
            input.stdin.ends_with('\n'),
            "stream-json is one message a line"
        );
        assert_eq!(input.stdin.trim_end().lines().count(), 1);
        let message: Value =
            serde_json::from_str(input.stdin.trim_end()).expect("one json message");
        assert_eq!(message["type"], "user");
        assert_eq!(message["message"]["role"], "user");
        message["message"]["content"].clone()
    }

    fn png_block() -> Value {
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}})
    }

    fn rejection(result: Result<Input, Undeliverable>) -> Rejection {
        match result {
            Err(Undeliverable::Refused { rejection, .. }) => rejection,
            other => panic!("expected a refused image: {other:?}"),
        }
    }

    #[test]
    fn claude_is_shown_the_images_a_message_was_sent_with() {
        let transcript = render(&[
            Message::system("Be terse."),
            with_images(Message::user("here's the comments"), &[PNG, LINK]),
        ]);

        assert_eq!(
            claude_content(&transcript),
            json!([
                {"type": "text", "text": "System:\nBe terse.\n\nUser:\nhere's the comments"},
                png_block(),
                {"type": "image", "source": {"type": "url", "url": LINK}},
            ])
        );
    }

    #[test]
    fn claude_reads_a_conversation_without_images_as_one_text_block() {
        assert_eq!(
            claude_content(&render(&[Message::user("ping")])),
            json!([{"type": "text", "text": "User:\nping"}])
        );
    }

    #[test]
    fn claude_is_given_the_lowercase_media_type_and_canonical_base64() {
        let transcript = render(&[with_images(
            Message::user("Look."),
            &["data:IMAGE/PNG;base64,iVBORw0K\nGgo"],
        )]);

        assert_eq!(
            claude_content(&transcript),
            json!([{"type": "text", "text": "User:\nLook."}, png_block()])
        );
    }

    #[test]
    fn an_earlier_image_claude_cannot_take_is_noted_rather_than_failing_every_turn() {
        let transcript = render(&[
            with_images(Message::assistant("Here it is."), &[ARTIFACT]),
            with_images(
                Message::user("And these?"),
                &["data:image/png;base64,!!!!", LINK],
            ),
            Message::assistant("Seen."),
            with_images(Message::user("Thanks."), &[PNG]),
        ]);

        assert_eq!(
            claude_content(&transcript),
            json!([
                {"type": "text", "text": "Assistant:\nHere it is."},
                {"type": "text", "text": OMITTED},
                {"type": "text", "text": "User:\nAnd these?"},
                {"type": "text", "text": OMITTED},
                {"type": "text", "text": OMITTED},
                {"type": "text", "text": "Assistant:\nSeen.\n\nUser:\nThanks."},
                png_block(),
            ])
        );
    }

    #[test]
    fn a_conversation_with_no_user_message_notes_every_image_it_cannot_send() {
        let transcript = render(&[with_images(Message::assistant("Drawn."), &[ARTIFACT])]);

        assert_eq!(
            claude_content(&transcript),
            json!([
                {"type": "text", "text": "Assistant:\nDrawn."},
                {"type": "text", "text": OMITTED},
            ])
        );
    }

    fn claude_within(transcript: &Transcript, limits: Limits) -> Result<Value, Rejection> {
        match claude(transcript, limits) {
            Ok(stdin) => {
                let message: Value = serde_json::from_str(stdin.trim_end()).expect("json");
                Ok(message["message"]["content"].clone())
            }
            Err(Undeliverable::Refused { rejection, .. }) => Err(rejection),
            Err(error) => panic!("unexpected: {error}"),
        }
    }

    fn three_turns_of_images() -> Transcript {
        render(&[
            with_images(Message::user("First."), &[PNG]),
            Message::assistant("Seen."),
            with_images(Message::user("Second."), &[PNG]),
            Message::assistant("Seen."),
            with_images(Message::user("Third."), &[PNG]),
        ])
    }

    /// For each image in `content`, in order, whether it was sent rather than
    /// noted.
    fn sent_images(content: &Value) -> Vec<bool> {
        content
            .as_array()
            .expect("blocks")
            .iter()
            .filter(|block| block["type"] == "image" || block["text"] == OMITTED)
            .map(|block| block["type"] == "image")
            .collect()
    }

    #[test]
    fn claude_is_sent_no_more_images_than_a_request_may_carry_keeping_the_newest() {
        let limits = Limits {
            images: 2,
            ..Limits::CLAUDE
        };

        let content = claude_within(&three_turns_of_images(), limits).expect("deliverable");

        assert_eq!(sent_images(&content), [false, true, true]);
    }

    #[test]
    fn claude_is_sent_no_more_image_bytes_than_a_request_may_carry_keeping_the_newest() {
        let text = three_turns_of_images().text().len();
        let limits = Limits {
            request_bytes: text + 2 * PNG_DATA.len(),
            ..Limits::CLAUDE
        };

        let content = claude_within(&three_turns_of_images(), limits).expect("deliverable");

        assert_eq!(sent_images(&content), [false, true, true]);
    }

    #[test]
    fn images_this_turn_was_sent_with_that_overflow_the_request_fail_it() {
        let transcript = render(&[with_images(Message::user("Look."), &[PNG, PNG])]);

        let fewer = Limits {
            images: 1,
            ..Limits::CLAUDE
        };
        assert_eq!(claude_within(&transcript, fewer), Err(Rejection::Excess));

        let smaller = Limits {
            request_bytes: transcript.text().len() + PNG_DATA.len(),
            ..Limits::CLAUDE
        };
        assert_eq!(claude_within(&transcript, smaller), Err(Rejection::Crowded));
    }

    #[test]
    fn an_image_larger_than_claude_takes_fails_the_turn_that_sent_it() {
        let transcript = render(&[with_images(Message::user("Look."), &[PNG])]);
        let limits = Limits {
            image_bytes: PNG_DATA.len() - 1,
            ..Limits::CLAUDE
        };

        assert_eq!(
            claude_within(&transcript, limits),
            Err(Rejection::Oversized)
        );
    }

    #[test]
    fn an_earlier_file_that_is_not_the_image_it_claims_is_noted() {
        let transcript = render(&[
            with_images(Message::user("Old."), &["data:image/png;base64,aGVsbG8="]),
            Message::user("New."),
        ]);

        assert_eq!(
            claude_content(&transcript),
            json!([
                {"type": "text", "text": "User:\nOld."},
                {"type": "text", "text": OMITTED},
                {"type": "text", "text": "User:\nNew."},
            ])
        );
    }

    #[test]
    fn an_image_this_turn_was_sent_with_that_cannot_be_shown_fails_the_turn() {
        for (url, expected) in [
            (ARTIFACT, Rejection::Unreachable),
            ("data:image/png;base64,!!!!", Rejection::Malformed),
            ("data:image/svg+xml;base64,PHN2Zz4=", Rejection::Unsupported),
        ] {
            let transcript = render(&[with_images(Message::user("Look."), &[url])]);
            for agent in AgentKind::ALL {
                let result = Input::new(agent, &transcript);
                let message = result.as_ref().expect_err("undeliverable").to_string();
                assert!(
                    message.contains(agent.as_str()),
                    "names the agent: {message}"
                );
                assert!(
                    message.contains(&expected.to_string()),
                    "says why: {message}"
                );
                assert_eq!(rejection(result), expected, "{agent} {url}");
            }
        }
    }

    #[test]
    fn codex_is_handed_each_image_as_a_file_marked_where_it_was_sent() {
        let transcript = render(&[with_images(
            Message::user("here's the comments"),
            &[PNG, "data:image/jpeg;base64,/9j/4AAQ"],
        )]);

        let input = Input::new(AgentKind::Codex, &transcript).expect("deliverable");

        assert_eq!(
            input.stdin,
            "User:\nhere's the comments\n\n[Image 1]\n\n[Image 2]"
        );
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
    fn an_earlier_image_codex_cannot_open_is_noted() {
        let transcript = render(&[
            with_images(Message::assistant("Here it is."), &[ARTIFACT, LINK]),
            Message::user("Thanks."),
        ]);

        let input = Input::new(AgentKind::Codex, &transcript).expect("deliverable");

        assert!(input.images.is_empty());
        assert_eq!(
            input.stdin,
            format!("Assistant:\nHere it is.\n\n{OMITTED}\n\n{OMITTED}\n\nUser:\nThanks.")
        );
    }

    #[test]
    fn codex_cannot_be_given_a_linked_image() {
        let transcript = render(&[with_images(Message::user("Look."), &[LINK])]);

        assert_eq!(
            rejection(Input::new(AgentKind::Codex, &transcript)),
            Rejection::Linked
        );
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
