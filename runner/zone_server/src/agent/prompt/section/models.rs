//! Catalog, install, and training jobs inside the same turn.

use crate::agent::prompt::Context;

const MODELS: &str = "Models:\n\
     - list_models, get_model, install_model and delete_model manage the catalog. Poll get_model_install for a pull; cancel_model_install stops one. Do not claim an install finished unless the tool says it did.\n\
     - list_train_bases then start_train. The call returns while the job runs; poll get_train_job. One job at a time. dismiss_train only a finished job.\n\
     - Person uses the SDXL people base and a trigger. Language dumps documents. Video needs clips.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    context.tools.has("list_models").then(|| MODELS.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn model_tools_bring_poll_and_one_job_rules() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["list_models"], None);
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(rendered.contains("poll get_train_job"), "{rendered}");
        assert!(rendered.contains("One job at a time"), "{rendered}");
        assert!(
            rendered.contains("Person uses the SDXL people base"),
            "{rendered}"
        );
        assert!(!rendered.contains("wait_for"), "{rendered}");
    }

    #[test]
    fn a_catalog_without_model_tools_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
