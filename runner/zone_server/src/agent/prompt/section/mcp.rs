//! Guidance for the tools an attached MCP server contributed.

use crate::agent::prompt::Context;

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    context.tools.mcp_guidance()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn attached_guidance_is_rendered_as_its_own_section() {
        let tools = ChatTools::with_names(
            ToolProfile::Chat,
            &["magents_spawn_session"],
            Some("Server guidance.".to_string()),
        );
        let environment = environment();
        assert_eq!(
            render(&chat_context(&tools, false, &environment)).as_deref(),
            Some("Server guidance.")
        );
    }

    #[test]
    fn a_catalog_without_mcp_servers_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
