//! How the final message reads: post-tool replies, formatting and narration.
//!
//! The reply is the only text the user reliably sees, so the rules that shape it
//! render on both surfaces and in the plain prompt. Narration and the post-tool
//! reply describe work the user cannot watch, so they render only when there is
//! a catalog to work with. How a refusal is written belongs to `refusal.rs`,
//! where declining is the subject, so these formatting rules stay silent on it.

use crate::agent::prompt::Context;

/// Wording the reply rules ban, iterated by the section's own tests.
pub(in crate::agent::prompt) const BANNED_PHRASES: &[&str] = &[
    "genuinely",
    "honestly",
    "straightforward",
    "My honest recommendation",
    "Honestly?",
    "delve",
    "leverage",
    "it's worth noting",
];

const HEADING: &str = "Writing the reply:";

const NARRATION: &str = "- Only your final message reliably reaches the user. Tool calls and the text between them \
     may not be shown, so it has to stand on its own.\n\
     - Say in one line what you are about to do before you start, and keep updates brief while you \
     work. On a long run, one short sentence every couple of tool calls is enough.\n\
     - Close with a recap that stands on its own for someone who did not watch: what you found, \
     what you changed, and what is left.";

const POST_TOOL: &str = "- After your last tool call, state the answer in one or two sentences. A sign-off alone, such \
     as \"Done.\", is not a reply.\n\
     - Do not repeat what you already wrote before the call. Your first sentence answers the \
     question, and anything about how you got there comes after it.\n\
     - Never open by announcing that no tool was needed. Open with the answer.";

const FORMATTING: &str = "- Use the minimum formatting that carries the meaning. In a personal or emotional exchange \
     write plain prose, because formatting there reads as clinical.\n\
     - Skip headers in a reply under about 500 words, and use at most three above that.\n\
     - Leave a blank line before a list and after a header, which most renderers need.";

const PATTERNS: &str = "- Do not use contrastive framing of the form \"X, not Y\" that raises an alternative the user \
     never asked about.\n\
     - Never praise your own plan by contrasting it with a worse one you invented.";

fn banned() -> String {
    format!(
        "- Do not write {}. Be direct rather than announcing that you are being direct.",
        BANNED_PHRASES
            .iter()
            .map(|phrase| format!("\"{phrase}\""))
            .collect::<Vec<String>>()
            .join(", ")
    )
}

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    let cadence = if context.tools.is_empty() {
        String::new()
    } else {
        format!("{NARRATION}\n{POST_TOOL}\n")
    };
    Some(format!(
        "{HEADING}\n{cadence}{FORMATTING}\n{}\n{PATTERNS}",
        banned()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};

    fn tools() -> ChatTools {
        ChatTools::with_names(ToolProfile::Chat, &["read_file", "web_search"], None)
    }

    fn rendered() -> String {
        let tools = tools();
        let environment = environment();
        render(&chat_context(&tools, false, &environment)).unwrap()
    }

    #[test]
    fn every_banned_phrase_is_named_in_the_section() {
        let rendered = rendered();

        assert!(!BANNED_PHRASES.is_empty());
        for phrase in BANNED_PHRASES {
            assert!(
                rendered.contains(&format!("\"{phrase}\"")),
                "{phrase} is banned but never named: {rendered}"
            );
        }
    }

    #[test]
    fn a_sign_off_alone_is_not_a_reply() {
        let rendered = rendered();

        assert!(
            rendered.contains("state the answer in one or two sentences"),
            "{rendered}"
        );
        assert!(
            rendered.contains("A sign-off alone, such as \"Done.\", is not a reply."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Your first sentence answers the question"),
            "{rendered}"
        );
        assert!(
            rendered.contains("announcing that no tool was needed"),
            "{rendered}"
        );
    }

    #[test]
    fn headers_are_reserved_for_long_replies() {
        let rendered = rendered();

        assert!(
            rendered.contains("Skip headers in a reply under about 500 words"),
            "{rendered}"
        );
        assert!(
            rendered.contains("blank line before a list and after a header"),
            "{rendered}"
        );
    }

    #[test]
    fn a_long_run_gets_one_short_update_every_couple_of_calls() {
        let rendered = rendered();

        assert!(
            rendered.contains("Say in one line what you are about to do before you start"),
            "{rendered}"
        );
        assert!(
            rendered.contains("one short sentence every couple of tool calls"),
            "{rendered}"
        );
        assert!(
            rendered.contains("recap that stands on its own for someone who did not watch"),
            "{rendered}"
        );
    }

    /// How a refusal is written is `refusal.rs`'s rule, and both sections render
    /// on the chat surface, so restating it here would state it twice.
    #[test]
    fn a_personal_exchange_stays_in_prose_and_declining_is_left_to_the_refusal_section() {
        let rendered = rendered();

        assert!(
            rendered.contains("minimum formatting that carries the meaning"),
            "{rendered}"
        );
        assert!(
            rendered.contains("personal or emotional exchange"),
            "{rendered}"
        );
        assert!(!rendered.contains("bullet points"), "{rendered}");
    }

    #[test]
    fn an_invented_alternative_is_never_the_reason_a_plan_sounds_good() {
        let rendered = rendered();

        assert!(
            rendered.contains("contrastive framing of the form \"X, not Y\""),
            "{rendered}"
        );
        assert!(
            rendered.contains("contrasting it with a worse one you invented"),
            "{rendered}"
        );
    }

    #[test]
    fn an_empty_catalog_drops_the_cadence_and_keeps_the_wording_rules() {
        let tools = ChatTools::empty();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(!rendered.contains("Done."), "{rendered}");
        assert!(
            !rendered.contains("every couple of tool calls"),
            "{rendered}"
        );
        assert!(!rendered.contains("last tool call"), "{rendered}");
        assert!(!rendered.contains("no tool was needed"), "{rendered}");
        assert!(!rendered.contains("recap"), "{rendered}");

        assert!(rendered.starts_with(HEADING), "{rendered}");
        assert!(rendered.contains("\"it's worth noting\""), "{rendered}");
        assert!(rendered.contains("about 500 words"), "{rendered}");
    }

    #[test]
    fn the_section_renders_on_both_surfaces() {
        let tools = tools();
        let environment = environment();
        let chat = render(&chat_context(&tools, false, &environment)).unwrap();
        let task = render(&task_context(&tools, &environment)).unwrap();

        assert_eq!(chat, task);
        assert!(chat.starts_with(HEADING), "{chat}");
        assert!(!chat.ends_with('\n'), "{chat}");
        assert!(!chat.contains("\n\n"), "{chat}");
    }
}
