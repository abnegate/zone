//! Who the model is, what it may call, and how it decides to call anything.
//!
//! A chat with an empty catalog gets neither the catalog sentence nor the rules
//! for spending a tool call, because it has none to spend and describing one
//! only invites an announcement that it is about to search. The evidence rules
//! stay: that chat still receives retrieved knowledge, sources and a server-side
//! web search in its prompt, so what to cite and when to admit a gap still bind.

use crate::agent::prompt::Context;

const IDENTITY: &str = "You are Zone's assistant, answering inside one of the user's workspaces.";

const TOOL_FIRST: &str = "- Anything about this workspace's own documents, sources, projects or tasks must come \
     from a tool call, never from memory. Search first, answer second.";

const CALLING: &str = "- Greetings, small talk and general knowledge questions need no tools; answer them directly. \
     Call a tool only when its result is needed for the user's request.\n\
     - Prefer one well-phrased search over several near-identical ones, and stop searching \
     once you can answer. Do not repeat an unchanged tool call after receiving its result.";

const EVIDENCE: &str = "- Name the sources you drew on. Structured citations keep the source URL, immutable \
     ref or document revision, and observation time. Incomplete evidence is never a passing result.\n\
     - If the tools return nothing useful, say so plainly instead of guessing. A wrong answer \
     about the user's own data is worse than an admission that you could not find it.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if context.tools.is_empty() {
        return Some(format!("{IDENTITY}\n\nHow to work:\n{EVIDENCE}"));
    }
    Some(format!(
        "{IDENTITY}\n\nYou can call these tools: {}.\n\nHow to work:\n{TOOL_FIRST}\n{CALLING}\n{EVIDENCE}",
        context.tools.names().join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn a_catalog_is_listed_and_workspace_facts_must_come_from_it() {
        let tools =
            ChatTools::with_names(ToolProfile::Chat, &["read_file", "list_documents"], None);
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(rendered.starts_with(IDENTITY), "{rendered}");
        assert!(
            rendered.contains("You can call these tools: list_documents, read_file."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Search first, answer second."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Name the sources you drew on."),
            "{rendered}"
        );
    }

    #[test]
    fn an_empty_catalog_keeps_the_identity_line_without_tool_instructions() {
        let tools = ChatTools::empty();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(rendered.starts_with(IDENTITY), "{rendered}");
        assert!(!rendered.contains("You can call these tools"), "{rendered}");
        assert!(
            !rendered.contains("must come from a tool call"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Name the sources you drew on."),
            "{rendered}"
        );
    }

    /// Nothing here can be called, so a rule for deciding to call it is an
    /// instruction the chat cannot follow and a prompt for a search it cannot
    /// run. The catalog decides, not the surface.
    #[test]
    fn an_empty_catalog_is_never_told_how_to_spend_a_tool_call() {
        let tools = ChatTools::empty();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        for instruction in [
            "Call a tool only when",
            "Do not repeat an unchanged tool call",
            "stop searching",
            "Prefer one well-phrased search",
            "need no tools",
        ] {
            assert!(!rendered.contains(instruction), "{instruction}: {rendered}");
        }
    }

    /// The with-tools path is what today's callers already send, so splitting
    /// the block for the empty catalog must not move a byte of it.
    #[test]
    fn a_catalog_still_reads_every_rule_in_its_original_order() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert_eq!(
            rendered,
            format!(
                "{IDENTITY}\n\nYou can call these tools: read_file.\n\nHow to work:\n\
                 {TOOL_FIRST}\n{CALLING}\n{EVIDENCE}"
            )
        );
    }
}
