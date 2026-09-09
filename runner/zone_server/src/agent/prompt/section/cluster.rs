//! Reading Zone's own monitoring stack instead of guessing at it.

use crate::agent::prompt::Context;

const CLUSTER: &str = "Cluster:\n\
     - query_prometheus and list_grafana_dashboards read Zone's live monitoring stack. Use them for on-call questions instead of guessing from chat history.\n\
     - Prefer a bounded PromQL range (start/end) over an unbounded instant query.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    context
        .tools
        .has("query_prometheus")
        .then(|| CLUSTER.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn monitoring_tools_bring_the_on_call_and_bounded_range_rules() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["query_prometheus"], None);
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered.contains("instead of guessing from chat history"),
            "{rendered}"
        );
        assert!(rendered.contains("bounded PromQL range"), "{rendered}");
    }

    #[test]
    fn a_catalog_without_monitoring_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
