//! How to cite a retrieved source by the identifier it arrived with.
//!
//! Chat only: a task run reports to a pull request rather than to a reader, and
//! nothing on that surface mints an identifier. Within chat the catalog decides
//! the form. A chat with no tools still receives the pre-turn search block, so
//! its sources still carry identifiers and the rule still binds, but it has no
//! tool to call and must not be told that a tool returned anything.
//!
//! What a citation is for belongs to `identity.rs`, which names the sources and
//! the fields a structured citation keeps on both surfaces. This section is the
//! mechanism alone: which identifier is legitimate, and where the marker goes.

use crate::agent::prompt::{Context, Surface};

const HEADING: &str = "Citing sources:";

const MARKER: &str = "- A retrieved source arrives with a bracketed identifier such as [web:a3f21c]. Cite it by \
     writing that identifier verbatim.";

const RESOLVED: &str = "- Cite only identifiers a tool returned in this chat. The server resolves every marker \
     against what it actually retrieved and drops one it cannot match.";

const SUPPLIED: &str = "- Cite only identifiers the sources in this prompt arrived with.";

const PLACEMENT: &str = "- Put the marker after the final punctuation of the sentence or table cell it supports, \
     never in a list at the end.";

const LINK: &str =
    "- Never write a markdown link or a bare URL for a cited source; the identifier is the link.";

const EXTENT: &str = "- A marker is attribution, not licence to quote at length. Keep borrowed wording short and \
     put the rest in your own words.";

const CARRY: &str = "- Carry identifiers through a summarised conversation unchanged; summarising earlier turns \
     retires none of them.";

const EXEMPT: &str = "- Build, deployment, metric and status results need no identifier.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if context.surface != Surface::Chat {
        return None;
    }
    if context.tools.is_empty() {
        return Some(format!(
            "{HEADING}\n{MARKER}\n{SUPPLIED}\n{PLACEMENT}\n{LINK}\n{EXTENT}\n{CARRY}"
        ));
    }
    Some(format!(
        "{HEADING}\n{MARKER}\n{RESOLVED}\n{PLACEMENT}\n{LINK}\n{EXTENT}\n{CARRY}\n{EXEMPT}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};

    fn tools() -> ChatTools {
        ChatTools::with_names(
            ToolProfile::Chat,
            &["search_knowledge", "web_search", "get_build_status"],
            None,
        )
    }

    fn rendered() -> String {
        let tools = tools();
        let environment = environment();
        render(&chat_context(&tools, false, &environment)).unwrap()
    }

    fn plain() -> String {
        let tools = ChatTools::empty();
        let environment = environment();
        render(&chat_context(&tools, false, &environment)).unwrap()
    }

    #[test]
    fn a_source_is_cited_by_the_identifier_it_arrived_with() {
        let rendered = rendered();

        assert!(rendered.starts_with(HEADING), "{rendered}");
        assert!(
            rendered.contains("bracketed identifier such as [web:a3f21c]"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Cite it by writing that identifier verbatim."),
            "{rendered}"
        );
    }

    /// The server re-derives an identifier for everything it retrieved and
    /// refuses the rest, so a minted-looking marker buys nothing.
    #[test]
    fn only_an_identifier_a_tool_returned_can_be_cited() {
        let rendered = rendered();

        assert!(
            rendered.contains("Cite only identifiers a tool returned in this chat."),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "The server resolves every marker against what it actually retrieved and drops \
                 one it cannot match."
            ),
            "{rendered}"
        );
    }

    #[test]
    fn a_marker_follows_the_punctuation_of_what_it_supports() {
        let rendered = rendered();

        assert!(
            rendered.contains(
                "Put the marker after the final punctuation of the sentence or table cell it \
                 supports"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("never in a list at the end."),
            "{rendered}"
        );
    }

    #[test]
    fn a_cited_source_is_never_a_markdown_link_or_a_bare_url() {
        let rendered = rendered();

        assert!(
            rendered.contains("Never write a markdown link or a bare URL for a cited source"),
            "{rendered}"
        );
        assert!(
            rendered.contains("the identifier is the link"),
            "{rendered}"
        );
    }

    #[test]
    fn a_marker_is_attribution_rather_than_licence_to_quote_at_length() {
        let rendered = rendered();

        assert!(
            rendered.contains("A marker is attribution, not licence to quote at length."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Keep borrowed wording short and put the rest in your own words."),
            "{rendered}"
        );
    }

    /// Summarising rewrites the turns an identifier was minted in, and a rewrite
    /// that renames one leaves the server unable to resolve it.
    #[test]
    fn identifiers_survive_a_summarised_conversation_unchanged() {
        let rendered = rendered();

        assert!(
            rendered.contains("Carry identifiers through a summarised conversation unchanged"),
            "{rendered}"
        );
        assert!(
            rendered.contains("summarising earlier turns retires none of them."),
            "{rendered}"
        );
    }

    #[test]
    fn a_build_or_status_result_needs_no_identifier() {
        assert!(
            rendered().contains("Build, deployment, metric and status results need no identifier."),
            "{}",
            rendered()
        );
    }

    /// Nothing on a task run mints an identifier, so every rule here would be an
    /// instruction about markers the run can never be handed.
    #[test]
    fn a_task_run_renders_nothing() {
        let tools = tools();
        let environment = environment();

        assert!(render(&task_context(&tools, &environment)).is_none());
        assert!(render(&task_context(&ChatTools::empty(), &environment)).is_none());
    }

    /// The pre-turn search block reaches this chat too, so the marker rules
    /// stay; what goes is every word implying a call it cannot make.
    #[test]
    fn an_empty_catalog_keeps_the_marker_rules_and_never_names_a_tool() {
        let plain = plain();

        assert!(plain.starts_with(HEADING), "{plain}");
        assert!(
            plain.contains("Cite only identifiers the sources in this prompt arrived with."),
            "{plain}"
        );
        for instruction in [
            "a tool returned",
            "The server resolves every marker",
            "Build, deployment, metric and status",
        ] {
            assert!(!plain.contains(instruction), "{instruction}: {plain}");
        }

        assert!(plain.contains("[web:a3f21c]"), "{plain}");
        assert!(plain.contains("the identifier is the link"), "{plain}");
        assert!(plain.contains("licence to quote at length"), "{plain}");
        assert!(
            plain.contains("summarised conversation unchanged"),
            "{plain}"
        );
    }

    #[test]
    fn neither_form_runs_two_rules_together_or_trails_a_newline() {
        for rendered in [rendered(), plain()] {
            assert!(!rendered.contains("\n\n"), "{rendered}");
            assert!(!rendered.ends_with('\n'), "{rendered}");
        }
    }

    /// No section may name a model vendor or product.
    #[test]
    fn the_section_names_no_vendor() {
        for rendered in [rendered().to_lowercase(), plain().to_lowercase()] {
            for vendor in [
                "claude",
                "anthropic",
                "openai",
                "gpt",
                "codex",
                "grok",
                "xai",
                "fable",
            ] {
                assert!(!rendered.contains(vendor), "{vendor} in {rendered}");
            }
        }
    }
}
