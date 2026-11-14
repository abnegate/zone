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
pub(crate) use section::session::NOW;
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
    ("memory", section::memory::render),
    ("files", section::files::render),
    ("waiting", section::waiting::render),
    ("images", section::images::render),
    ("models", section::models::render),
    ("cluster", section::cluster::render),
    ("web", section::web::render),
    ("citation", section::citation::render),
    ("task", section::task::render),
    ("planner", section::planner::render),
    ("mcp", section::mcp::render),
    ("toolbox", section::toolbox::render),
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

/// Bounds the prompt of a member's chat over everything production assembles
/// for one: every optional tool group on, an MCP server attached, the tools
/// whose schemas are held back listed, a GitHub source, a stated effort and
/// the server's own zone. It measured 29,803 characters auto-approved in
/// zone's own loop, and 27,902 offered to a coding agent, which is offered no
/// tool that ends zone's turn. Telling that chat to open the pull request its
/// work needs rather than ask for one adds 257 to each, counted over the
/// sentences that changed; `every_prompt_stays_inside_its_budget` is what
/// measures it again.
pub const CHAT_MAX_CHARS: usize = 30_100;
/// Bounds the prompt of a planner chat, which adds the interview section and
/// its two tools to the chat prompt: 32,163 characters, and 30,170 offered to
/// a coding agent, each carrying the same 257.
pub const PLANNER_MAX_CHARS: usize = 32_500;
/// Bounds the prompt of a chat that offers no tools: 4,740 characters.
pub const PLAIN_MAX_CHARS: usize = 4_800;
/// Bounds the task prompt over everything production assembles for a run its
/// writer started: the same tool groups, server and listing, the checkout's
/// branch and head, and a stated effort. It measured 23,678 characters for an
/// unattended run that requires its plan approved, 584 of them added by the
/// plan paragraph, and 20,756 offered to a coding agent. Splitting the rule
/// about the pull request from the rule about the comment adds at most 48 to
/// each, less whatever the outward tier drops by no longer naming one.
pub const TASK_MAX_CHARS: usize = 23_800;

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
    use crate::agent::discovery::RECORD_DISCOVERY;
    use crate::agent::plan::SUBMIT_PLAN;
    use crate::agent::question::ASK_USER;
    use crate::agent::{ToolProfile, WorkspaceScope};
    use crate::state::AppState;
    use abnegate_agent::mcp;
    use abnegate_agent::mcp::McpConfig;
    use abnegate_agent::mcp::McpHub;
    use abnegate_agent::mcp::McpServer;
    use abnegate_agent::tool::Tier;
    use abnegate_agent_cli::AgentKind;
    use abnegate_agent_cli::CliSettings;
    use abnegate_llm::Effort;
    use std::collections::HashMap;
    use std::sync::LazyLock;
    use test_support::{chat_context, environment, task_context};
    use uuid::Uuid;
    use zone_core::commands;
    use zone_core::commands::Shell;
    use zone_core::llm::LlmBackend;
    use zone_core::servers;

    /// The one tool of the stand-in server [`fully_configured`] attaches under
    /// magents' name.
    const MCP_TOOL: &str = "magents__delegate";

    /// Every tool a member's chat assembles, the stand-in MCP tool included.
    const CHAT_CATALOG: &[&str] = &[
        "apply_patch",
        "ask_user",
        "assess_pull_requests",
        "assess_release_pipelines",
        "cancel_model_install",
        "cancel_reminder",
        "comment_on_issue",
        "create_document",
        "create_pull_request",
        "create_reminder",
        "create_task",
        "delete_model",
        "dismiss_train",
        "edit_image",
        "fetch_url",
        "generate_audio",
        "generate_image",
        "get_build_status",
        "get_issue",
        "get_model",
        "get_model_install",
        "get_task_run",
        "get_train_job",
        "install_model",
        "list_chats",
        "list_deployments",
        "list_documents",
        "list_files",
        "list_grafana_dashboards",
        "list_issues",
        "list_members",
        "list_models",
        "list_projects",
        "list_reminders",
        "list_sources",
        "list_tasks",
        "list_train_bases",
        "load_tools",
        MCP_TOOL,
        "memory_append",
        "memory_delete",
        "memory_list",
        "memory_read",
        "memory_write",
        "query_prometheus",
        "read_chat_evidence",
        "read_check_logs",
        "read_document",
        "read_file",
        "read_repository_file",
        "read_review_threads",
        RECORD_DISCOVERY,
        "run_command",
        "run_shell",
        "search_chat_history",
        "search_code",
        "search_knowledge",
        "search_tools",
        "send_message",
        "start_task",
        "start_train",
        "tail_job",
        "tail_task_log",
        "update_document",
        "update_task",
        "wait_for",
        "web_search",
        "write_file",
    ];

    /// Every tool a run assembles for the writer who started it, the stand-in
    /// MCP tool included.
    const TASK_CATALOG: &[&str] = &[
        "apply_patch",
        "ask_user",
        "assess_pull_requests",
        "assess_release_pipelines",
        "comment_on_issue",
        "create_document",
        "create_pull_request",
        "create_task",
        "fetch_url",
        "get_build_status",
        "get_issue",
        "get_task_run",
        "list_chats",
        "list_deployments",
        "list_documents",
        "list_files",
        "list_grafana_dashboards",
        "list_issues",
        "list_members",
        "list_projects",
        "list_sources",
        "list_tasks",
        "load_tools",
        MCP_TOOL,
        "query_prometheus",
        "read_check_logs",
        "read_document",
        "read_file",
        "read_repository_file",
        "read_review_threads",
        RECORD_DISCOVERY,
        "run_command",
        "search_chat_history",
        "search_code",
        "search_knowledge",
        "search_tools",
        "tail_job",
        "tail_task_log",
        "update_document",
        "update_task",
        "wait_for",
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
        "tail_job",
        "wait_for",
        "write_file",
    ];

    /// What an authorized writer adds on top of `TASK_HOST`.
    const TASK_DOCUMENTS: &[&str] = &[
        "create_document",
        "list_documents",
        "read_document",
        RECORD_DISCOVERY,
        "update_document",
    ];

    /// The worst case for every chat measurement: a workspace with a GitHub
    /// source, which adds the bullets sending a pull request to zone's tool.
    fn github() -> Environment {
        environment().with_github()
    }

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

    /// The tier every tool a chat assembles declares for itself.
    ///
    /// A catalog built from names has no tool to ask its tier. Defaulting the
    /// answer to `Tier::Write` put all six outward tools at a tier none of them
    /// declares, so the prompts built from one lost the paragraph saying an
    /// outward action cannot be recalled and nothing failed. Nothing an
    /// assembly registers reaches the database, so the declarations are
    /// readable from here and a retiered or newly registered tool arrives
    /// without anyone editing a table.
    static DECLARED: LazyLock<HashMap<String, Tier>> = LazyLock::new(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a catalog assembles on a bare runtime")
            .block_on(async {
                let catalog = ChatTools::build(member(&fully_configured().await)).await;
                catalog
                    .names()
                    .iter()
                    .map(|name| (name.clone(), catalog.tier(name)))
                    .collect()
            })
    });

    /// Server state as the largest prompt finds it: every optional tool group
    /// switched on and one MCP server attached.
    ///
    /// Images, audio, monitoring and web search each register nothing unless
    /// their config is enabled, so a group gated on a setting this leaves off
    /// is neither tiered nor measured. The server is the stub fixture under
    /// magents' name, which earns the guidance zone writes for magents and
    /// the half of the tool listing that says a server wrote it; the tools a
    /// real server lists are that server's text and no ceiling here counts
    /// them. The pool is lazy and no tool is called, so nothing dials it.
    async fn fully_configured() -> AppState {
        let mut config = crate::state::test_config();
        config.comfyui.enabled = true;
        config.monitoring.enabled = true;
        config.web_search.enabled = true;
        let database = config.database_url.clone();
        let state = AppState::new(
            config,
            sqlx::PgPool::connect_lazy(&database).expect("a lazy pool needs no server"),
            None,
        );
        let stub = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stub_mcp_server.py");
        let hub = McpHub::connect(&McpConfig::default().with_server(
            servers::MAGENTS,
            McpServer::command("python3", [stub.to_string_lossy().into_owned()]),
        ))
        .await;
        assert_eq!(hub.server_count(), 1, "the stub server has to connect");
        state.install_mcp(hub);
        state
    }

    /// A member's chat in a workspace of `state`.
    fn member(state: &AppState) -> WorkspaceScope {
        WorkspaceScope {
            state: state.clone(),
            workspace_id: Uuid::nil(),
            chat_id: Some(Uuid::nil()),
            user_id: Uuid::nil(),
            offline: false,
            origin: crate::db::tasks::Origin::Member,
        }
    }

    /// One prompt a surface sends, the catalog it was rendered from and what
    /// set it apart from the surface's others.
    struct Sent {
        variant: String,
        tools: ChatTools,
        prompt: String,
    }

    /// Zone's own loop, then every coding agent a turn can be handed to.
    fn backends() -> Vec<(String, LlmBackend)> {
        let mut backends = vec![("in zone's own loop".to_string(), LlmBackend::Http)];
        backends.extend(AgentKind::ALL.iter().map(|&agent| {
            (
                format!("offered to {agent}"),
                LlmBackend::cli(agent, CliSettings::default()),
            )
        }));
        backends
    }

    /// The longest session block a chat renders: the server's own zone, which
    /// the block qualifies, a stated effort and a GitHub source.
    fn fullest_chat() -> Environment {
        let mut environment = github().with_effort(Effort::Medium);
        environment.reported = false;
        environment
    }

    /// The longest session block a run renders: the server's own zone, the
    /// checkout's branch and head, and a stated effort.
    fn fullest_run() -> Environment {
        let mut environment = environment().with_effort(Effort::Medium).with_vcs(Vcs {
            branch: "main".to_string(),
            head: "0123456789abcdef0123456789abcdef01234567".to_string(),
        });
        environment.reported = false;
        environment
    }

    /// Every prompt a member's chat is sent, a planner chat's when `planner`.
    async fn chats(state: &AppState, planner: bool) -> Vec<Sent> {
        let environment = fullest_chat();
        let mut sent = Vec::new();
        for auto_approve in [false, true] {
            for (label, backend) in backends() {
                let tools = ChatTools::build(member(state)).await;
                let tools = if planner { tools.with_planner() } else { tools };
                let tools = crate::mcp::offered(&backend, tools);
                let approval = if auto_approve {
                    "auto-approved"
                } else {
                    "awaiting approval"
                };
                sent.push(Sent {
                    variant: format!("{approval} {label}"),
                    prompt: chat(&tools, auto_approve, &environment),
                    tools,
                });
            }
        }
        sent
    }

    /// Every prompt a run started by a writer is sent, one that requires its
    /// plan approved when `plan`.
    async fn runs(state: &AppState, plan: bool) -> Vec<Sent> {
        let environment = fullest_run();
        let mut sent = Vec::new();
        for unattended in [false, true] {
            for (label, backend) in backends() {
                let tools = ChatTools::for_task_writer(
                    state,
                    environment.directory.clone(),
                    Uuid::nil(),
                    Uuid::nil(),
                )
                .await;
                let tools = if unattended {
                    tools.unattended()
                } else {
                    tools
                };
                let tools = if plan {
                    tools.with_plan_approval()
                } else {
                    tools
                };
                let tools = crate::mcp::offered(&backend, tools);
                let watched = if unattended { "unattended" } else { "attended" };
                sent.push(Sent {
                    variant: format!("{watched} {label}"),
                    prompt: task(&tools, &environment),
                    tools,
                });
            }
        }
        sent
    }

    fn longest(sent: &[Sent]) -> &Sent {
        sent.iter()
            .max_by_key(|sent| sent.prompt.len())
            .expect("a surface sends at least one prompt")
    }

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
        names.sort();
        names
    }

    /// Every catalog name with the tier its tool declares, taken from the host
    /// registry the profile loads and then from the workspace catalog a chat
    /// assembles. Only a tool neither of them registers falls back to a write.
    fn tiered(profile: ToolProfile, catalog: &[&'static str]) -> Vec<(&'static str, Tier)> {
        let host = match profile {
            ToolProfile::Chat => commands::registry(Shell::Included),
            ToolProfile::Task => commands::registry(Shell::Excluded),
        };
        catalog
            .iter()
            .map(|name| {
                let tier = host
                    .tier(name)
                    .or_else(|| DECLARED.get(*name).copied())
                    .unwrap_or(Tier::Write);
                (*name, tier)
            })
            .collect()
    }

    fn chat_tools() -> ChatTools {
        ChatTools::with_tiers(
            ToolProfile::Chat,
            &tiered(ToolProfile::Chat, CHAT_CATALOG),
            mcp::guidance_for_tools(&[MCP_TOOL], &servers::guidance()),
        )
    }

    fn task_catalog() -> Vec<(&'static str, Tier)> {
        let mut catalog = TASK_HOST.to_vec();
        catalog.extend_from_slice(TASK_DOCUMENTS);
        tiered(ToolProfile::Task, &catalog)
    }

    fn task_tools() -> ChatTools {
        ChatTools::with_tiers(ToolProfile::Task, &task_catalog(), None)
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
                "memory",
                "files",
                "waiting",
                "images",
                "models",
                "cluster",
                "web",
                "citation",
                "task",
                "planner",
                "mcp",
                "toolbox",
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
        assert!(rendered.contains("Memory:"), "{rendered}");
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
        assert!(!rendered.contains("Memory:"), "{rendered}");
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

        let environment = github();
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
            chat(
                &ChatTools::with_names(ToolProfile::Chat, CHAT_CATALOG, None)
                    .served_to(abnegate_agent_cli::AgentKind::Codex),
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
    /// while `workspace` let `create_pull_request` run only when the user asked
    /// for a pull request and `files` let the commit it needs run only on the
    /// same ask, so the assembled chat prompt both ordered and forbade the same
    /// action. `workspace` now owns it on the chat surface and says work on a
    /// connected repository ends in a pull request; on the task surface `task`
    /// says Zone opens the run's pull request itself. Counting on each surface
    /// is what keeps one section deciding.
    #[test]
    fn one_section_decides_who_opens_a_pull_request_on_each_surface() {
        let task_prompt = task(&task_tools(), &github());
        let chat_prompt = chat(&chat_tools(), false, &github());

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
            occurrences(&chat_prompt, "ends in a pull request"),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&chat_prompt, "Only use it when the user asked to open a PR"),
            0,
            "a chat that opens the pull request its work needs is not also told to wait to be \
             asked for one: {chat_prompt}"
        );
        assert_eq!(
            occurrences(&chat_prompt, "push only when the user asks"),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&chat_prompt, "so Zone can track it"),
            1,
            "{chat_prompt}"
        );
        assert_eq!(
            occurrences(&task_prompt, "so Zone can track it"),
            0,
            "{task_prompt}"
        );
    }

    /// A pull request opened through zone's own tool is one Zone records; one
    /// a shell opened is only caught by reading the shell's output. So the chat
    /// prompt sends the request to the tool, by the name this turn's model
    /// calls it, and only while there is a GitHub source for it to act on.
    #[test]
    fn a_chat_with_a_github_source_opens_pull_requests_through_zone() {
        let source = github();
        let native = chat(&chat_tools(), false, &source);

        assert!(
            native.contains("then calling create_pull_request so Zone can track it"),
            "{native}"
        );
        assert!(native.contains("or use gh pr create, hub"), "{native}");

        for &agent in abnegate_agent_cli::AgentKind::ALL {
            let served = chat(&chat_tools().served_to(agent), false, &source);
            assert!(
                served.contains("then calling mcp__zone__create_pull_request so Zone can track it"),
                "{agent}: {served}"
            );
            assert!(
                served.contains("or use gh pr create, hub"),
                "{agent}: {served}"
            );
        }

        let without = chat(&chat_tools(), false, &environment());
        assert!(!without.contains("so Zone can track it"), "{without}");
        assert!(!without.contains("gh pr create"), "{without}");
    }

    /// Task runs open their pull request through Zone already, so a GitHub
    /// source changes nothing in what a run is told, even one whose authorized
    /// actor hands it the GitHub writers.
    #[test]
    fn a_github_source_leaves_the_task_prompt_as_it_was() {
        let mut catalog = TASK_HOST.to_vec();
        catalog.extend_from_slice(TASK_DOCUMENTS);
        catalog.extend(["comment_on_issue", "create_pull_request"]);
        let tools = ChatTools::with_tiers(
            ToolProfile::Task,
            &tiered(ToolProfile::Task, &catalog),
            None,
        );

        let with = task(&tools, &github());

        assert_eq!(with, task(&tools, &environment()));
        assert!(!with.contains("gh pr create"), "{with}");
        assert_eq!(occurrences(&with, "opens the pull request"), 1, "{with}");
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
    /// would leave every ownership test below reading a prompt no user is ever
    /// served.
    #[test]
    fn the_test_catalogs_carry_every_name_they_list() {
        assert_eq!(chat_tools().names().len(), CHAT_CATALOG.len());
        assert_eq!(
            task_tools().names().len(),
            TASK_HOST.len() + TASK_DOCUMENTS.len()
        );
        assert!(chat_tools().has(ASK_USER));
        assert!(task_tools().has(ASK_USER));
    }

    /// A catalog built from names is only worth measuring while it carries the
    /// tiers production declares, and the outward six are what that is really
    /// guarding: under the old `Tier::Write` default every one of them was
    /// described to the model as an ordinary write. Naming the whole outward
    /// set here is what fails the next tool registered at that tier and left
    /// out of the catalog below.
    #[test]
    fn the_test_catalogs_carry_the_tier_each_tool_declares_in_production() {
        assert!(
            DECLARED.values().any(|tier| *tier == Tier::Outward),
            "production registers no outward tool at all"
        );

        for name in DECLARED.keys() {
            assert!(
                CHAT_CATALOG.contains(&name.as_str()),
                "a chat registers {name} and the worst-case catalog omits it"
            );
        }

        for (name, tier) in tiered(ToolProfile::Chat, CHAT_CATALOG) {
            assert_eq!(Some(&tier), DECLARED.get(name), "{name}");
        }

        assert_eq!(DECLARED[ASK_USER], Tier::Read, "a question is a read");
    }

    /// The paragraph is the only place either prompt says an outward action is
    /// unrecallable and needs the user's own words. It renders off the declared
    /// tiers rather than off the names, so a catalog that lost them dropped it
    /// from the assembled prompt in silence.
    #[test]
    fn a_catalog_that_can_reach_outside_the_workspace_says_the_send_is_final() {
        let environment = environment();

        for rendered in [
            chat(&chat_tools(), false, &environment),
            task(&task_tools(), &environment),
        ] {
            assert!(
                rendered.contains(section::tiers::OUTWARD_HEAD),
                "{rendered}"
            );
            assert!(
                rendered.contains(section::tiers::OUTWARD_TAIL),
                "{rendered}"
            );
        }

        let inward = ChatTools::with_names(ToolProfile::Task, TASK_HOST, None);
        let rendered = task(&inward, &environment);
        assert!(
            !rendered.contains(section::tiers::OUTWARD_HEAD),
            "{rendered}"
        );
    }

    /// The section renders off the catalog, so the prompt only carries the
    /// rules for a question when the tool that asks one is there to call.
    #[test]
    fn the_elicitation_rules_arrive_with_the_tool_and_not_before() {
        let environment = environment();
        let without: Vec<(&str, Tier)> = tiered(ToolProfile::Chat, CHAT_CATALOG)
            .into_iter()
            .filter(|(name, _)| *name != ASK_USER)
            .collect();
        let without = ChatTools::with_tiers(ToolProfile::Chat, &without, None);

        assert!(!chat(&without, false, &environment).contains("Asking the user:"));
        assert!(chat(&chat_tools(), false, &environment).contains("Asking the user:"));
    }

    /// Both sections go quiet for a catalog without `wait_for` and neither
    /// failure is visible from anywhere else: the waiting rules render nothing,
    /// and the runner bullet, which names the tool among the ones it requires,
    /// gives way to one that does not name it.
    #[test]
    fn both_assembled_prompts_carry_the_waiting_rules_and_the_runner_bullet() {
        let environment = environment();
        let chat_tools = chat_tools();
        let task_tools = task_tools();

        let for_chat = section::waiting::render(&chat_context(&chat_tools, false, &environment))
            .expect("the chat catalog carries wait_for");
        let for_task = section::waiting::render(&task_context(&task_tools, &environment))
            .expect("the task catalog carries wait_for");
        let chat_prompt = chat(&chat_tools, false, &environment);

        assert!(chat_prompt.contains(&for_chat), "{chat_prompt}");
        assert!(
            chat_prompt.contains(section::workspace::START_TASK),
            "{chat_prompt}"
        );

        let task_prompt = task(&task_tools, &environment);

        assert!(task_prompt.contains(&for_task), "{task_prompt}");
    }

    /// The ceilings are measured over what production assembles, so a tool
    /// registered tomorrow is measured without anyone listing it. These lists
    /// are what make the registration visible here: a name a surface gains or
    /// loses fails until its list says so, which keeps the catalogs the other
    /// prompt tests state by name the ones a turn is handed.
    #[tokio::test]
    async fn every_tool_a_surface_registers_is_one_its_budget_measures() {
        let state = fully_configured().await;
        let chat = ChatTools::build(member(&state)).await;
        let run = || {
            ChatTools::for_task_writer(
                &state,
                std::path::PathBuf::from("/srv/zone"),
                Uuid::nil(),
                Uuid::nil(),
            )
        };

        assert_eq!(chat.names(), sorted(CHAT_CATALOG));
        assert_eq!(
            chat.with_planner().names(),
            sorted(
                &[
                    CHAT_CATALOG,
                    &[
                        crate::agent::planner::CREATE_REPOSITORY,
                        crate::agent::planner::FINALIZE_PROJECT,
                    ],
                ]
                .concat()
            )
        );
        assert_eq!(run().await.names(), sorted(TASK_CATALOG));
        assert_eq!(
            run().await.with_plan_approval().names(),
            sorted(&[TASK_CATALOG, &[SUBMIT_PLAN]].concat())
        );

        for name in [TASK_HOST, TASK_DOCUMENTS].concat() {
            assert!(
                TASK_CATALOG.contains(&name),
                "the task tests state {name} and no run registers it"
            );
        }
    }

    /// A deferred tool costs its line in the listing, and a catalog stated by
    /// name defers nothing, so the listing never rendered in a prompt a
    /// ceiling was measured over. Every tool a measured prompt's catalog holds
    /// is either sent as a schema or named on a line of that prompt.
    #[tokio::test]
    async fn every_measured_prompt_lists_the_tools_whose_schemas_it_holds_back() {
        let state = fully_configured().await;
        let mut sent = chats(&state, false).await;
        sent.extend(chats(&state, true).await);
        sent.extend(runs(&state, false).await);
        sent.extend(runs(&state, true).await);

        for Sent {
            variant,
            tools,
            prompt,
        } in &sent
        {
            let handed: Vec<String> = tools
                .definitions()
                .into_iter()
                .map(|definition| definition.function.name)
                .collect();
            assert!(
                handed.len() < tools.names().len(),
                "{variant} defers nothing, so its listing is not measured"
            );
            for name in tools.names() {
                assert!(
                    handed.contains(name) || prompt.contains(&format!("\n- {name}: ")),
                    "{name} is neither sent nor listed {variant}"
                );
            }
        }
    }

    #[tokio::test]
    async fn every_prompt_stays_inside_its_budget() {
        let state = fully_configured().await;
        let chats_sent = chats(&state, false).await;
        let planners_sent = chats(&state, true).await;
        let runs_sent = runs(&state, false).await;
        let approving_sent = runs(&state, true).await;
        let chat_prompt = longest(&chats_sent);
        let planner_prompt = longest(&planners_sent);
        let task_prompt = longest(&runs_sent);
        let approving_prompt = longest(&approving_sent);
        let plain_prompt = plain(&fullest_chat());

        for sent in [chat_prompt, planner_prompt, task_prompt, approving_prompt] {
            assert!(sent.prompt.contains("Asking the user:"), "{}", sent.prompt);
        }
        assert!(
            planner_prompt.prompt.contains("Planning a project:"),
            "{}",
            planner_prompt.prompt
        );
        assert!(
            approving_prompt.prompt.len() > task_prompt.prompt.len(),
            "the plan paragraph adds nothing: {} chars against {}",
            approving_prompt.prompt.len(),
            task_prompt.prompt.len()
        );

        for (surface, sent, ceiling) in [
            ("chat", chat_prompt, CHAT_MAX_CHARS),
            ("planner", planner_prompt, PLANNER_MAX_CHARS),
            ("task", task_prompt, TASK_MAX_CHARS),
            ("plan-approved task", approving_prompt, TASK_MAX_CHARS),
        ] {
            assert!(
                sent.prompt.len() <= ceiling,
                "the {surface} prompt {} is {} chars",
                sent.variant,
                sent.prompt.len()
            );
        }
        assert!(
            plain_prompt.len() <= PLAIN_MAX_CHARS,
            "plain prompt is {} chars",
            plain_prompt.len()
        );
    }

    /// A chat holds both rules that have it write without being asked, so the
    /// workspace's may not read as the only one.
    #[test]
    fn a_chat_told_to_write_memory_unasked_is_told_discovery_is_only_the_workspaces_exception() {
        let prompt = chat(&chat_tools(), false, &github());

        assert_eq!(occurrences(&prompt, "- Do not wait to be asked: "), 1);
        assert_eq!(
            occurrences(
                &prompt,
                "- record_discovery is the one workspace write you make unasked: "
            ),
            1
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
