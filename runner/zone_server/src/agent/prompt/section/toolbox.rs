//! The tools that exist but whose schemas are not loaded.
//!
//! Deferring a schema saves context only if the model still knows the tool is
//! there. A tool nobody can name is a tool nobody can ask for, so the saving
//! would come out of the feature rather than out of the budget. This renders
//! the one line each deferred tool declares about itself, which is cheap enough
//! to carry every round and is the whole reason the rest can be left out.

use crate::agent::prompt::Context;

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    let deferred = context.tools.deferred();
    if deferred.is_empty() {
        return None;
    }
    let listed: Vec<String> = deferred
        .iter()
        .map(|tool| format!("- {}: {}", tool.name, tool.purpose))
        .collect();
    Some(format!(
        "These tools exist but their schemas are not loaded, to keep the ones you use most in \
         front of you. Call load_tools with the names you want and they arrive on your next \
         round; search_tools finds one by what it does when the name is not obvious. Do not \
         guess at arguments for a tool listed here — load it and read its schema.\n\n{}",
        listed.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    /// The list is what makes deferral safe rather than lossy, so it names
    /// every held-back tool and says how to get one.
    #[test]
    fn every_deferred_tool_is_named_with_the_line_it_declares() {
        let tools = ChatTools::with_deferred(
            ToolProfile::Chat,
            &["read_file", "search_knowledge"],
            &[
                ("cancel_reminder", "Stop a standing schedule."),
                ("list_chats", "List the chats in this workspace."),
            ],
        );
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment))
            .expect("a catalog holding tools back says so");

        assert!(rendered.contains("- cancel_reminder: Stop a standing schedule."));
        assert!(rendered.contains("- list_chats: List the chats in this workspace."));
        assert!(
            rendered.contains("load_tools"),
            "naming a tool without saying how to reach it is worse than not naming it: {rendered}"
        );
        assert!(
            !rendered.contains("read_file"),
            "a loaded tool is in the schemas already and listing it twice spends what deferral \
             saved: {rendered}"
        );
    }

    /// Nothing held back, nothing said. A profile whose tools all load — a
    /// task run, or a chat small enough not to need this — should not carry a
    /// paragraph explaining a mechanism it never uses.
    #[test]
    fn a_catalog_that_defers_nothing_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
