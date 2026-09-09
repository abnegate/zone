//! Generating and editing images inside the same turn.

use crate::agent::prompt::Context;

const IMAGES: &str = "Images:\n\
     - generate_image and edit_image stay in this loop. After an image is generated you can inspect it and edit it in the same turn.\n\
     - Do not claim an image was created unless the tool returned a URL.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    context
        .tools
        .has("generate_image")
        .then(|| IMAGES.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn image_tools_bring_the_same_turn_and_no_claim_rules() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["generate_image"], None);
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(rendered.contains("edit it in the same turn"), "{rendered}");
        assert!(
            rendered.contains("unless the tool returned a URL"),
            "{rendered}"
        );
    }

    #[test]
    fn a_catalog_without_image_tools_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
