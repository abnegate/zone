//! The system prompt, assembled from named sections in one fixed order.
//!
//! Zone used to build this by appending strings in four places, which drifts as
//! it grows: a later fragment can contradict an earlier one and nothing catches
//! it. Here every rule lives in exactly one section, each section decides on its
//! own whether it applies, and `ORDER` fixes the sequence once for both
//! surfaces. `session` renders last so its live timestamp cannot invalidate the
//! prompt cache for everything above it.

mod context;
mod environment;
mod section;
mod surface;
mod vcs;
mod verbosity;

pub(crate) use context::Context;
pub use environment::Environment;
pub use surface::Surface;
pub use vcs::Vcs;
pub use verbosity::Verbosity;

use crate::agent::ChatTools;

/// A section's name, used only by the ordering tests, and its renderer.
type Section = (&'static str, fn(&Context<'_>) -> Option<String>);

/// Every section, in the order an assembled prompt reads them.
const ORDER: &[Section] = &[
    ("identity", section::identity::render),
    ("boundary", section::boundary::render),
    ("tiers", section::tiers::render),
    ("elicitation", section::elicitation::render),
    ("conduct", section::conduct::render),
    ("reply", section::reply::render),
    ("refusal", section::refusal::render),
    ("workspace", section::workspace::render),
    ("retrieval", section::retrieval::render),
    ("files", section::files::render),
    ("images", section::images::render),
    ("cluster", section::cluster::render),
    ("web", section::web::render),
    ("citation", section::citation::render),
    ("task", section::task::render),
    ("mcp", section::mcp::render),
    ("session", section::session::render),
];

/// A chat with no tools, whose workspace evidence arrives from the server.
const PLAIN: &[Section] = &[
    ("identity", section::identity::render),
    ("boundary", section::boundary::render),
    ("reply", section::reply::render),
    ("refusal", section::refusal::render),
    ("citation", section::citation::render),
    ("session", section::session::render),
];

pub const CHAT_MAX_CHARS: usize = 19_000;
pub const PLAIN_MAX_CHARS: usize = 6_000;
pub const TASK_MAX_CHARS: usize = 12_000;

fn assemble(order: &[Section], context: &Context<'_>) -> String {
    order
        .iter()
        .filter_map(|(_, render)| render(context))
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// The prompt for an agentic chat turn.
pub fn chat(tools: &ChatTools, auto_approve: bool, environment: &Environment) -> String {
    assemble(
        ORDER,
        &Context {
            surface: Surface::Chat,
            auto_approve,
            environment,
            tools,
        },
    )
}

/// The prompt for a chat that offers no tools and has no character card.
pub fn plain(environment: &Environment) -> String {
    let tools = ChatTools::empty();
    assemble(
        PLAIN,
        &Context {
            surface: Surface::Chat,
            auto_approve: false,
            environment,
            tools: &tools,
        },
    )
}

/// The boundary section alone, appended after a character card.
pub fn boundary() -> String {
    section::boundary::BOUNDARY.to_string()
}

/// The prompt for a background task run.
pub fn task(tools: &ChatTools, environment: &Environment) -> String {
    assemble(
        ORDER,
        &Context {
            surface: Surface::Task,
            auto_approve: true,
            environment,
            tools,
        },
    )
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{Context, Environment, Surface};
    use crate::agent::ChatTools;
    use chrono::DateTime;
    use std::path::PathBuf;

    /// A fixed environment, so every section test renders the same bytes twice.
    pub(crate) fn environment() -> Environment {
        Environment::at(
            DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
            "Pacific/Auckland",
            PathBuf::from("/srv/zone"),
        )
    }

    pub(crate) fn chat_context<'a>(
        tools: &'a ChatTools,
        auto_approve: bool,
        environment: &'a Environment,
    ) -> Context<'a> {
        Context {
            surface: Surface::Chat,
            auto_approve,
            environment,
            tools,
        }
    }

    pub(crate) fn task_context<'a>(
        tools: &'a ChatTools,
        environment: &'a Environment,
    ) -> Context<'a> {
        Context {
            surface: Surface::Task,
            auto_approve: true,
            environment,
            tools,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ToolProfile;
    use test_support::environment;
    use zone_core::tools::{Tier, ToolRegistry};

    /// Every chat tool loaded today, plus one MCP tool, as a worst-case catalog.
    const CHAT_CATALOG: &[&str] = &[
        "apply_patch",
        "ask_user",
        "assess_pull_requests",
        "assess_release_pipelines",
        "cancel_reminder",
        "comment_on_issue",
        "create_document",
        "create_pull_request",
        "create_reminder",
        "create_task",
        "edit_image",
        "fetch_url",
        "generate_audio",
        "generate_image",
        "get_build_status",
        "get_task_run",
        "list_chats",
        "list_deployments",
        "list_documents",
        "list_files",
        "list_grafana_dashboards",
        "list_issues",
        "list_members",
        "list_projects",
        "list_reminders",
        "list_sources",
        "list_tasks",
        "magents_spawn_session",
        "query_prometheus",
        "read_chat_evidence",
        "read_check_logs",
        "read_document",
        "read_file",
        "read_repository_file",
        "run_command",
        "run_shell",
        "search_chat_history",
        "search_code",
        "search_knowledge",
        "send_message",
        "start_task",
        "tail_task_log",
        "update_document",
        "update_task",
        "web_search",
        "write_file",
    ];

    /// The sandboxed host tools a task always gets, and the question it can
    /// put to the user while it runs.
    const TASK_HOST: &[&str] = &[
        "apply_patch",
        "ask_user",
        "list_files",
        "read_file",
        "run_command",
        "search_code",
        "write_file",
    ];

    /// What an authorized writer adds on top of `TASK_HOST`.
    const TASK_DOCUMENTS: &[&str] = &[
        "create_document",
        "list_documents",
        "read_document",
        "update_document",
    ];

    fn names(order: &[Section]) -> Vec<&'static str> {
        order.iter().map(|(name, _)| *name).collect()
    }

    fn occurrences(prompt: &str, phrase: &str) -> usize {
        prompt.matches(phrase).count()
    }

    /// Whole word only: "sol" sits inside "resolve" and "console", and a
    /// substring match would report a leak that is not there.
    fn names_vendor(prompt: &str, vendor: &str) -> bool {
        let lowered = prompt.to_lowercase();
        lowered.match_indices(vendor).any(|(offset, _)| {
            let before = lowered[..offset].chars().next_back();
            let after = lowered[offset + vendor.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    }

    /// Every catalog name with the tier it declares, `Tier::Write` standing in
    /// for the names no host registry knows, which is the fallback
    /// `ChatTools::tier` applies to them in production. Stating the tiers is
    /// what keeps `ask_user` a read: left to that fallback it would be
    /// described to the model as a call an approval gates.
    fn tiered(profile: ToolProfile, catalog: &[&'static str]) -> Vec<(&'static str, Tier)> {
        let host = match profile {
            ToolProfile::Chat => ToolRegistry::with_host_tools(),
            ToolProfile::Task => ToolRegistry::with_defaults(),
        };
        catalog
            .iter()
            .map(|name| {
                let tier = if *name == section::elicitation::ASK_USER {
                    Tier::Read
                } else {
                    host.tier(name).unwrap_or(Tier::Write)
                };
                (*name, tier)
            })
            .collect()
    }

    fn chat_tools() -> ChatTools {
        ChatTools::with_tiers(
            ToolProfile::Chat,
            &tiered(ToolProfile::Chat, CHAT_CATALOG),
            zone_core::mcp::guidance_for_tools(&["magents_spawn_session"]),
        )
    }

    fn task_tools() -> ChatTools {
        let mut catalog = TASK_HOST.to_vec();
        catalog.extend_from_slice(TASK_DOCUMENTS);
        ChatTools::with_tiers(
            ToolProfile::Task,
            &tiered(ToolProfile::Task, &catalog),
            None,
        )
    }

    #[test]
    fn the_order_is_fixed_and_the_session_block_renders_last() {
        assert_eq!(
            names(ORDER),
            [
                "identity",
                "boundary",
                "tiers",
                "elicitation",
                "conduct",
                "reply",
                "refusal",
                "workspace",
                "retrieval",
                "files",
                "images",
                "cluster",
                "web",
                "citation",
                "task",
                "mcp",
                "session",
            ]
        );
        assert_eq!(
            names(PLAIN),
            [
                "identity", "boundary", "reply", "refusal", "citation", "session"
            ]
        );
        assert_eq!(names(ORDER).last(), Some(&"session"));
        assert_eq!(names(PLAIN).last(), Some(&"session"));
    }

    #[test]
    fn every_plain_section_is_also_an_ordered_section() {
        let ordered = names(ORDER);
        for name in names(PLAIN) {
            assert!(ordered.contains(&name), "{name} is not in ORDER");
        }
    }

    #[test]
    fn a_chat_prompt_states_whether_mutating_tools_wait_for_approval() {
        let tools = chat_tools();
        let environment = environment();

        let required = chat(&tools, false, &environment);
        assert!(
            required.contains("wait for the user to approve"),
            "{required}"
        );
        assert!(!required.contains("without waiting for confirmation"));

        let automatic = chat(&tools, true, &environment);
        assert!(
            automatic.contains("without waiting for confirmation"),
            "{automatic}"
        );
        assert!(!automatic.contains("wait for the user to approve"));
    }

    #[test]
    fn a_chat_prompt_carries_every_catalog_section() {
        let rendered = chat(&chat_tools(), false, &environment());

        assert!(rendered.contains("You can call these tools:"), "{rendered}");
        assert!(rendered.contains("Workspace actions:"), "{rendered}");
        assert!(rendered.contains("Images:"), "{rendered}");
        assert!(rendered.contains("Cluster:"), "{rendered}");
        assert!(rendered.contains("Web tools:"), "{rendered}");
        assert!(rendered.contains("Citing sources:"), "{rendered}");
        assert!(rendered.contains("prefixed with the server"), "{rendered}");
    }

    #[test]
    fn a_task_prompt_keeps_the_sandbox_rules_and_drops_the_chat_only_ones() {
        let rendered = task(&task_tools(), &environment());

        assert!(rendered.contains("stay inside the"), "{rendered}");
        assert!(
            rendered.contains("There is no unrestricted shell."),
            "{rendered}"
        );
        assert!(
            !rendered.contains("act in the server runtime"),
            "{rendered}"
        );
        assert!(!rendered.contains("Images:"), "{rendered}");
        assert!(!rendered.contains("Cluster:"), "{rendered}");
        assert!(!rendered.contains("Web tools:"), "{rendered}");
        assert!(!rendered.contains("Citing sources:"), "{rendered}");
    }

    /// The six host tools are what a run without an authorized actor receives.
    #[test]
    fn a_task_without_an_authorized_actor_omits_the_workspace_sections() {
        let tools = ChatTools::with_names(ToolProfile::Task, TASK_HOST, None);
        let rendered = task(&tools, &environment());

        assert!(!rendered.contains("Workspace actions:"), "{rendered}");
        assert!(!rendered.contains("Images:"), "{rendered}");
        assert!(!rendered.contains("Cluster:"), "{rendered}");
        assert!(!rendered.contains("Web tools:"), "{rendered}");
    }

    /// The tool rules migrated into `identity` came from a prompt that always
    /// had a catalog. This chat has none, so every rule for spending a call is
    /// an instruction it cannot follow. What the server injects instead, the
    /// retrieved knowledge and sources block, is still evidence, so the rules
    /// for citing it and for admitting a gap stay.
    #[test]
    fn a_plain_prompt_keeps_the_identity_line_and_never_orders_a_tool_call() {
        let rendered = plain(&environment());

        assert!(
            rendered.starts_with(
                "You are Zone's assistant, answering inside one of the user's workspaces."
            ),
            "{rendered}"
        );
        for instruction in [
            "You can call these tools",
            "must come from a tool call",
            "Call a tool only when",
            "Do not repeat an unchanged tool call",
            "stop searching",
            "Prefer one well-phrased search",
        ] {
            assert!(!rendered.contains(instruction), "{instruction}: {rendered}");
        }
        assert!(
            rendered.contains("Name the sources you drew on."),
            "{rendered}"
        );
        assert!(
            rendered.contains("A wrong answer about the user's own data"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Cite only identifiers the sources in this prompt arrived with."),
            "{rendered}"
        );
    }

    /// A vendor name in one of these files is almost always a section's own
    /// exclusion list, so grepping the module proves nothing. Only the text the
    /// model reads counts. The single exemption is the magents roster naming the
    /// agents Zone drives, which is a product roster; subtracting exactly that
    /// block is what stops the exemption quietly widening to the rest.
    #[test]
    fn no_rendered_prompt_names_a_model_vendor() {
        const VENDORS: &[&str] = &[
            "claude",
            "anthropic",
            "openai",
            "gpt",
            "codex",
            "grok",
            "xai",
            "fable",
            "astra",
        ];

        let environment = environment();
        let tools = chat_tools();
        let roster = section::mcp::render(&test_support::chat_context(&tools, false, &environment))
            .expect("the magents catalog contributes MCP guidance");
        let outside_the_roster = chat(&tools, false, &environment).replace(&roster, "");

        for rendered in [
            chat(
                &ChatTools::with_names(ToolProfile::Chat, CHAT_CATALOG, None),
                false,
                &environment,
            ),
            plain(&environment),
            task(&task_tools(), &environment),
            outside_the_roster,
        ] {
            for vendor in VENDORS {
                assert!(
                    !names_vendor(&rendered, vendor),
                    "{vendor} appears in {rendered}"
                );
            }
        }

        assert!(names_vendor(&roster, "claude"), "{roster}");
    }

    #[test]
    fn the_boundary_entry_point_is_the_boundary_section_alone() {
        let tools = ChatTools::empty();
        let environment = environment();
        let section =
            section::boundary::render(&test_support::chat_context(&tools, false, &environment))
                .unwrap();

        assert_eq!(boundary(), section);
        assert!(!boundary().is_empty());
    }

    #[test]
    fn sections_are_separated_by_a_blank_line_and_never_run_together() {
        let rendered = chat(&chat_tools(), false, &environment());

        assert!(!rendered.contains("\n\n\n"), "{rendered}");
        assert!(!rendered.starts_with('\n'), "{rendered}");
        assert!(!rendered.ends_with('\n'), "{rendered}");
    }

    /// `reply` and `refusal` both render on the chat surface and were written
    /// without sight of each other, so each independently banned bullet points
    /// in a refusal and the assembled prompt said it twice. `refusal` owns it,
    /// and counting the phrase is what catches the next author restating it.
    #[test]
    fn the_rule_against_a_bulleted_refusal_is_stated_once_and_only_by_refusal() {
        let chat_prompt = chat(&chat_tools(), false, &environment());
        let plain_prompt = plain(&environment());
        let tools = ChatTools::empty();
        let environment = environment();
        let refusal =
            section::refusal::render(&test_support::chat_context(&tools, false, &environment))
                .unwrap();

        assert_eq!(
            occurrences(&chat_prompt, "bullet points"),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&plain_prompt, "bullet points"),
            1,
            "{plain_prompt}"
        );
        assert_eq!(occurrences(&refusal, "bullet points"), 1, "{refusal}");
    }

    /// `conduct` told a chat turn to take a draft pull request rather than ask,
    /// while `workspace` lets `create_pull_request` run only when the user asked
    /// for a pull request and `files` lets the push it needs run only on the
    /// same ask, so the assembled chat prompt both ordered and forbade the same
    /// action. On the task surface `task` says Zone opens the run's pull request
    /// itself. Counting on each surface is what keeps one section deciding.
    #[test]
    fn one_section_decides_who_opens_a_pull_request_on_each_surface() {
        let task_prompt = task(&task_tools(), &environment());
        let chat_prompt = chat(&chat_tools(), false, &environment());

        assert_eq!(
            occurrences(&task_prompt, "opens the pull request"),
            1,
            "{task_prompt}"
        );
        assert_eq!(
            occurrences(&chat_prompt, "opens the pull request"),
            0,
            "{chat_prompt}"
        );

        for prompt in [&chat_prompt, &task_prompt] {
            assert_eq!(occurrences(prompt, "a draft pull request"), 0, "{prompt}");
        }

        assert_eq!(
            occurrences(
                &chat_prompt,
                "Only use them when the user asked to open a PR"
            ),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&chat_prompt, "Commit or push only when the user asks"),
            1,
            "{chat_prompt}"
        );
    }

    /// `reason` is in seven schemas' `required` arrays and nothing validates
    /// that array at dispatch, so this sentence is the whole lever. It is worth
    /// its bytes exactly once: `files` states it on both surfaces, and the
    /// sections holding the outward-writing tools are the obvious place for the
    /// next author to say it again. Counting is what stops that.
    #[test]
    fn the_rule_asking_why_a_changing_call_is_needed_is_stated_once_and_only_by_files() {
        let chat_prompt = chat(&chat_tools(), false, &environment());
        let task_prompt = task(&task_tools(), &environment());
        let plain_prompt = plain(&environment());

        for prompt in [&chat_prompt, &task_prompt] {
            assert_eq!(
                occurrences(prompt, "Where a tool takes a reason, give one"),
                1,
                "{prompt}"
            );
            assert_eq!(
                occurrences(prompt, "bound a long log with max_output_chars"),
                1,
                "{prompt}"
            );
        }

        assert_eq!(
            occurrences(&plain_prompt, "Where a tool takes a reason, give one"),
            0,
            "{plain_prompt}"
        );
    }

    /// A marker rule reads as advice about writing, so the next author to touch
    /// `reply` or `web` has every reason to restate one there, and the chat
    /// prompt has no room to say anything twice. `citation` owns the mechanism
    /// on both of the surfaces it renders on, and the exemption for a build or
    /// status result is the one rule that needs a tool to be worth stating.
    #[test]
    fn the_rules_for_citing_by_identifier_are_stated_once_and_only_by_citation() {
        let chat_prompt = chat(&chat_tools(), false, &environment());
        let plain_prompt = plain(&environment());
        let task_prompt = task(&task_tools(), &environment());

        for phrase in [
            "the identifier is the link",
            "licence to quote at length",
            "final punctuation of the sentence or table cell",
            "summarised conversation unchanged",
        ] {
            assert_eq!(
                occurrences(&chat_prompt, phrase),
                1,
                "{phrase}: {chat_prompt}"
            );
            assert_eq!(
                occurrences(&plain_prompt, phrase),
                1,
                "{phrase}: {plain_prompt}"
            );
            assert_eq!(
                occurrences(&task_prompt, phrase),
                0,
                "{phrase}: {task_prompt}"
            );
        }

        assert_eq!(
            occurrences(&chat_prompt, "need no identifier"),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&plain_prompt, "need no identifier"),
            0,
            "{plain_prompt}"
        );
    }

    /// `with_tiers` states every name outright, where `with_names` keeps only
    /// the few a host registry knows. Building the worst case out of those
    /// would leave every budget and ownership test below measuring a prompt no
    /// user is ever served.
    #[test]
    fn the_test_catalogs_carry_every_name_they_list() {
        assert_eq!(chat_tools().names().len(), CHAT_CATALOG.len());
        assert_eq!(
            task_tools().names().len(),
            TASK_HOST.len() + TASK_DOCUMENTS.len()
        );
        assert!(chat_tools().has(section::elicitation::ASK_USER));
        assert!(task_tools().has(section::elicitation::ASK_USER));
    }

    /// The section renders off the catalog, so the prompt only carries the
    /// rules for a question when the tool that asks one is there to call.
    #[test]
    fn the_elicitation_rules_arrive_with_the_tool_and_not_before() {
        let environment = environment();
        let without: Vec<(&str, Tier)> = tiered(ToolProfile::Chat, CHAT_CATALOG)
            .into_iter()
            .filter(|(name, _)| *name != section::elicitation::ASK_USER)
            .collect();
        let without = ChatTools::with_tiers(ToolProfile::Chat, &without, None);

        assert!(!chat(&without, false, &environment).contains("Asking the user:"));
        assert!(chat(&chat_tools(), false, &environment).contains("Asking the user:"));
    }

    #[test]
    fn every_prompt_stays_inside_its_budget() {
        let chat_prompt = chat(&chat_tools(), false, &environment());
        let plain_prompt = plain(&environment());
        let task_prompt = task(&task_tools(), &environment());

        for prompt in [&chat_prompt, &task_prompt] {
            assert!(prompt.contains("Asking the user:"), "{prompt}");
        }

        assert!(
            chat_prompt.len() <= CHAT_MAX_CHARS,
            "chat prompt is {} chars",
            chat_prompt.len()
        );
        assert!(
            plain_prompt.len() <= PLAIN_MAX_CHARS,
            "plain prompt is {} chars",
            plain_prompt.len()
        );
        assert!(
            task_prompt.len() <= TASK_MAX_CHARS,
            "task prompt is {} chars",
            task_prompt.len()
        );
    }

    #[test]
    fn a_fixed_environment_renders_the_same_prompt_twice() {
        let tools = chat_tools();
        let environment = environment();
        assert_eq!(
            chat(&tools, false, &environment),
            chat(&tools, false, &environment)
        );
    }
}
