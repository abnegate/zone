//! Flattening a conversation into the single prompt a CLI agent accepts.

mod part;

use abnegate_llm::{Message, Role};

pub use part::Part;

const SYSTEM: &str = "System";
const USER: &str = "User";
const ASSISTANT: &str = "Assistant";
const TOOL: &str = "Tool result";

pub const SEPARATOR: &str = "\n\n";

/// A conversation rendered for an agent: its text, with each message's images
/// following the message they were attached to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    parts: Vec<Part>,
}

impl Transcript {
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The text alone, as one prompt.
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                Part::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join(SEPARATOR)
    }

    fn write(&mut self, text: &str) {
        if let Some(Part::Text(last)) = self.parts.last_mut() {
            last.push_str(SEPARATOR);
            last.push_str(text);
        } else {
            self.parts.push(Part::Text(text.to_string()));
        }
    }
}

/// Render a conversation as one prompt.
///
/// A coding agent takes a prompt, not a message array, so the roles have to
/// survive as text. A bare concatenation loses who said what, and an agent
/// that cannot tell its own earlier reply from the user's instruction will
/// answer the wrong one.
pub fn render(messages: &[Message]) -> Transcript {
    render_tail(messages, 0)
}

/// Render only what follows the first `from` messages, the part of a
/// conversation a resumed session has not yet seen, in the format [`render`]
/// uses for the whole of it.
pub fn render_tail(messages: &[Message], from: usize) -> Transcript {
    let mut transcript = Transcript::default();
    let current = messages
        .iter()
        .rposition(|message| message.role == Role::User);

    for (index, message) in messages.iter().enumerate().skip(from) {
        let content = message.content.as_deref().map_or("", str::trim);
        if content.is_empty() && message.images.is_empty() {
            continue;
        }
        let label = match message.role {
            Role::System => SYSTEM,
            Role::Assistant => ASSISTANT,
            Role::Tool => TOOL,
            Role::User => USER,
            _ => USER,
        };
        transcript.write(format!("{label}:\n{content}").trim_end());
        transcript
            .parts
            .extend(message.images.iter().map(|url| Part::Image {
                url: url.clone(),
                current: Some(index) == current,
            }));
    }

    transcript
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_llm::ToolCall;

    #[test]
    fn every_role_is_named_in_the_prompt() {
        let prompt = render(&[
            Message::system("Be terse."),
            Message::user("What does main do?"),
            Message::assistant("Let me look."),
            Message::tool_result("toolu_01", "fn main() {}"),
            Message::user("Thanks."),
        ])
        .text();

        assert_eq!(
            prompt,
            "System:\nBe terse.\n\nUser:\nWhat does main do?\n\nAssistant:\nLet me look.\n\nTool result:\nfn main() {}\n\nUser:\nThanks."
        );
    }

    #[test]
    fn a_message_carrying_only_tool_calls_contributes_nothing() {
        let prompt = render(&[
            Message::user("Read it."),
            Message::assistant_with_tools(vec![ToolCall::function("toolu_01", "read", "{}")]),
        ])
        .text();

        assert_eq!(prompt, "User:\nRead it.");
    }

    #[test]
    fn blank_content_is_skipped_rather_than_padded() {
        let prompt = render(&[
            Message::system("   "),
            Message::user("  Only this.  "),
            Message::assistant(""),
        ])
        .text();

        assert_eq!(prompt, "User:\nOnly this.");
    }

    #[test]
    fn an_empty_conversation_renders_an_empty_prompt() {
        assert!(render(&[]).parts().is_empty());
    }

    #[test]
    fn the_tail_renders_only_entries_after_the_position() {
        let conversation = [
            Message::system("Be terse."),
            Message::user("What does main do?"),
            Message::assistant("Nothing yet."),
            Message::system("Web search is unavailable this turn."),
            Message::user("Make it print hello."),
        ];

        assert_eq!(
            render_tail(&conversation, 3).text(),
            "System:\nWeb search is unavailable this turn.\n\nUser:\nMake it print hello."
        );
        assert_eq!(
            render_tail(&conversation, 3),
            render(&conversation[3..]),
            "a tail reads as the same conversation rendered on its own"
        );
        assert_eq!(render_tail(&conversation, 0), render(&conversation));
        assert_eq!(
            render_tail(&conversation, 4).text(),
            "User:\nMake it print hello."
        );
        assert!(
            render_tail(&conversation, conversation.len())
                .parts()
                .is_empty()
        );
        assert!(
            render_tail(&conversation, conversation.len() + 1)
                .parts()
                .is_empty()
        );
    }

    fn with_images(mut message: Message, images: &[&str]) -> Message {
        message.images = images.iter().map(|url| (*url).to_string()).collect();
        message
    }

    #[test]
    fn each_image_follows_the_message_it_was_attached_to() {
        let transcript = render(&[
            Message::system("Be terse."),
            with_images(Message::user("Draw this."), &["data:image/png;base64,AAAA"]),
            with_images(Message::assistant("Done."), &["/api/artifacts/w/c/m/a.png"]),
            with_images(
                Message::user("here's the comments"),
                &["data:image/png;base64,BBBB", "data:image/png;base64,CCCC"],
            ),
        ]);

        assert_eq!(
            transcript.parts(),
            [
                Part::Text("System:\nBe terse.\n\nUser:\nDraw this.".into()),
                Part::Image {
                    url: "data:image/png;base64,AAAA".into(),
                    current: false,
                },
                Part::Text("Assistant:\nDone.".into()),
                Part::Image {
                    url: "/api/artifacts/w/c/m/a.png".into(),
                    current: false,
                },
                Part::Text("User:\nhere's the comments".into()),
                Part::Image {
                    url: "data:image/png;base64,BBBB".into(),
                    current: true,
                },
                Part::Image {
                    url: "data:image/png;base64,CCCC".into(),
                    current: true,
                },
            ]
        );
        assert_eq!(
            transcript.text(),
            "System:\nBe terse.\n\nUser:\nDraw this.\n\nAssistant:\nDone.\n\nUser:\nhere's the comments"
        );
    }

    #[test]
    fn a_message_of_images_alone_is_still_rendered() {
        let transcript = render(&[with_images(
            Message::user(""),
            &["data:image/png;base64,AAAA"],
        )]);

        assert_eq!(
            transcript.parts(),
            [
                Part::Text("User:".into()),
                Part::Image {
                    url: "data:image/png;base64,AAAA".into(),
                    current: true,
                },
            ]
        );
    }
}
