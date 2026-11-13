//! The tool context every chat and task run on the server works in.

mod config;

use std::sync::Arc;

use abnegate_agent::ToolContext;
use zone_core::application::application;
use zone_core::vpn::Offline;
use zone_core::walk;

pub use config::Config;

use super::environment;

/// A context built from `config`. Every child it starts is given the
/// [`environment::policy`], routed through Zone's proxy and started at the
/// configured niceness, its files are kept under `.zone/`, its walks have
/// Zone's [`walk::BUDGET`], and `curl` and `wget` are held to the VPN gate.
pub fn build(config: Config) -> ToolContext {
    let mut context = ToolContext::default()
        .within(config.working_directory)
        .with_session(config.session)
        .with_denied(config.denied)
        .with_application(application())
        .with_environment(environment::policy())
        .with_proxy(zone_routing::proxy())
        .with_public_web(Arc::new(Offline::new(config.chat_offline)))
        .with_search_timeout(walk::BUDGET)
        .with_niceness(config.niceness);
    context.unrestricted = config.unrestricted;
    context.maximum_file_size = config.maximum_file_size;
    context.command_timeout = config.command_timeout;
    context
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use abnegate_agent::tool::job::{JobCommand, Jobs};
    use abnegate_agent::tool::{
        OFF_LIMITS, ReadFileTool, RunCommandTool, RunShellTool, SearchCodeTool, Session,
    };
    use abnegate_agent::{Tool, ToolError, ToolResult};
    use abnegate_exec::Niceness;
    use serde_json::{Value, json};
    use tempfile::tempdir;
    use uuid::Uuid;
    use zone_core::variables::Variables;
    use zone_core::vpn::{CHAT_OFFLINE, Hold, OFFLINE};

    use super::*;
    use crate::agent::tools::{MAX_TOOL_FILE_BYTES, TOOL_COMMAND_TIMEOUT_SECS};

    const PROXY: &str = "http://127.0.0.1:28888";
    const PROXY_VARIABLES: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ];

    /// The file a stand-in for `rg` records its arguments in.
    const ARGUMENTS: &str = "arguments";

    fn chat_config(working_directory: &Path, denied: Vec<PathBuf>, chat_offline: bool) -> Config {
        Config {
            working_directory: working_directory.to_path_buf(),
            session: Session::Chat(Uuid::new_v4()),
            denied,
            unrestricted: true,
            maximum_file_size: MAX_TOOL_FILE_BYTES,
            command_timeout: Duration::from_secs(TOOL_COMMAND_TIMEOUT_SECS),
            chat_offline,
            niceness: Niceness::DEFAULT,
        }
    }

    fn chat(working_directory: &Path, denied: Vec<PathBuf>, chat_offline: bool) -> ToolContext {
        build(chat_config(working_directory, denied, chat_offline))
    }

    /// A chat context whose commands start at the lowest priority.
    fn lowered(working_directory: &Path) -> ToolContext {
        build(Config {
            niceness: Niceness::LOWEST,
            ..chat_config(working_directory, Vec::new(), false)
        })
    }

    async fn run(
        program: &str,
        arguments: Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        RunCommandTool::with_commands(&[program])
            .execute(json!({"command": program, "args": arguments}), context)
            .await
    }

    #[test]
    fn a_chat_context_sets_every_field_it_is_given() {
        let directory = tempdir().unwrap();
        let session = Session::Task(Uuid::new_v4());
        let denied = vec![directory.path().join("state")];

        let context = build(Config {
            working_directory: directory.path().to_path_buf(),
            session,
            denied: denied.clone(),
            unrestricted: false,
            maximum_file_size: 42,
            command_timeout: Duration::from_secs(7),
            chat_offline: false,
            niceness: Niceness::LOWEST,
        });

        assert_eq!(context.working_directory, directory.path());
        assert_eq!(context.session, session);
        assert_eq!(context.denied, denied);
        assert!(!context.unrestricted);
        assert_eq!(context.maximum_file_size, 42);
        assert_eq!(context.command_timeout, Duration::from_secs(7));
        assert!(!context.environment.inherits());
        assert_eq!(context.niceness, Some(Niceness::LOWEST));
    }

    #[tokio::test]
    async fn a_chat_context_routes_through_zones_proxy() {
        let directory = tempdir().unwrap();
        let mut variables = Variables::lock();
        variables.set(zone_routing::PROXY_URL_VARIABLE, PROXY);

        let context = chat(directory.path(), Vec::new(), false);

        assert!(context.proxy.is_some());
        for name in PROXY_VARIABLES {
            let printed = run("printenv", json!([name]), &context)
                .await
                .expect("printenv runs");
            assert_eq!(
                printed.output,
                Some(format!("stdout:\n{PROXY}\n")),
                "{name}: {printed:?}"
            );
        }
    }

    /// Zone's own agent ran its tool commands at the server's priority, so
    /// chats building and testing at once starved the server's API.
    #[tokio::test]
    async fn a_command_the_agent_runs_starts_at_the_configured_niceness() {
        let directory = tempdir().unwrap();
        let context = lowered(directory.path());

        let reported = RunShellTool
            .execute(json!({"command": "ps -o nice= -p $$"}), &context)
            .await
            .expect("the shell runs");

        let nice = reported
            .output
            .as_deref()
            .and_then(|output| output.strip_prefix("stdout:"))
            .map(str::trim);
        assert!(reported.success, "{reported:?}");
        assert_eq!(nice, Some("19"), "{reported:?}");
    }

    #[tokio::test]
    async fn a_job_the_agent_starts_runs_at_the_configured_niceness() {
        let directory = tempdir().unwrap();
        let context = lowered(directory.path());

        let started = Jobs::spawn(
            &JobCommand::shell("ps -o nice= -p $$").within(directory.path()),
            &context,
        )
        .await
        .expect("a job starts");
        let ended = Jobs::settled(context.session, &started.id)
            .expect("its own session claims it")
            .await;
        let log = std::fs::read_to_string(&started.log_path).expect("the job's log");
        Jobs::kill_session(context.session).await;

        assert_eq!(log.trim(), "19", "{ended:?}");
    }

    #[tokio::test]
    async fn a_chat_context_keeps_jobs_under_dot_zone() {
        let directory = tempdir().unwrap();
        let context = chat(directory.path(), Vec::new(), false);

        let started = Jobs::spawn(
            &JobCommand::new("true", Vec::new()).within(directory.path()),
            &context,
        )
        .await
        .expect("a job starts");
        Jobs::kill_session(context.session).await;

        assert_eq!(context.application, application());
        assert_eq!(
            Path::new(&started.log_path),
            directory
                .path()
                .join(".zone/jobs")
                .join(format!("{}.log", started.id))
        );
    }

    #[tokio::test]
    async fn a_chat_context_withholds_agent_state() {
        let directory = tempdir().unwrap();
        let state = directory.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::write(state.join("auth.json"), "notreal-token").unwrap();
        std::fs::write(directory.path().join("notes.md"), "shared").unwrap();
        std::os::unix::fs::symlink(state.join("auth.json"), directory.path().join("link")).unwrap();
        let context = chat(directory.path(), vec![state.clone()], false);

        let readable = ReadFileTool
            .execute(json!({"path": "notes.md"}), &context)
            .await
            .expect("a file outside the state reads");
        assert!(readable.success, "{readable:?}");
        for path in [state.join("auth.json"), directory.path().join("link")] {
            let error = ReadFileTool
                .execute(json!({"path": path}), &context)
                .await
                .expect_err("agent state is withheld");
            assert!(
                error.to_string().contains(OFF_LIMITS),
                "{}: {error}",
                path.display()
            );
        }
    }

    #[tokio::test]
    async fn a_search_out_of_time_says_so_in_the_crates_words() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("main.rs"), "fn main() {}").unwrap();
        let context = chat(directory.path(), Vec::new(), false).with_search_timeout(Duration::ZERO);

        let found = SearchCodeTool
            .execute(json!({"pattern": "absent"}), &context)
            .await
            .expect("a search out of time still answers")
            .output
            .unwrap();

        assert_eq!(
            found,
            "No matches found so far\n[search stopped early: out of time]"
        );
    }

    #[test]
    fn a_zone_context_walks_for_zones_twenty_five_seconds() {
        let directory = tempdir().unwrap();

        let context = chat(directory.path(), Vec::new(), false);

        assert_eq!(context.search_timeout, Duration::from_secs(25));
    }

    /// A stand-in for `rg` under `directory` that records the arguments it
    /// was started with, and the context that finds it first on its `PATH`.
    fn recording_rg(directory: &Path, checkout: &Path) -> ToolContext {
        let bin = directory.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("rg");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\0' \"$@\" > '{}'\n",
                directory.join(ARGUMENTS).display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let mut context = chat(checkout, Vec::new(), false);
        context
            .environment
            .set("PATH", path.into_string().expect("a UTF-8 PATH"));
        context
    }

    #[tokio::test]
    async fn search_hands_rg_the_crates_flags() {
        let directory = tempdir().unwrap();
        let checkout = directory.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        let context = recording_rg(directory.path(), &checkout);

        SearchCodeTool
            .execute(json!({"pattern": "needle"}), &context)
            .await
            .expect("a search");

        let recorded = std::fs::read_to_string(directory.path().join(ARGUMENTS))
            .expect("the stand-in for rg ran");
        let arguments: Vec<&str> = recorded.split_terminator('\0').collect();
        for flag in [
            "--no-config",
            "--no-ignore-parent",
            "--no-ignore-global",
            "--max-columns-preview",
        ] {
            assert!(arguments.contains(&flag), "{flag}: {arguments:?}");
        }
        for pair in [["--max-columns", "400"], ["--glob", "!node_modules/**"]] {
            assert!(
                arguments.windows(2).any(|window| window == pair),
                "{pair:?}: {arguments:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_file_only_a_parent_ignore_file_names_is_searched() {
        let directory = tempdir().unwrap();
        let checkout = directory.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(directory.path().join(".ignore"), "ignored.rs\n").unwrap();
        std::fs::write(checkout.join("ignored.rs"), "let needle = 1;\n").unwrap();
        let context = chat(&checkout, Vec::new(), false);

        let found = SearchCodeTool
            .execute(json!({"pattern": "needle"}), &context)
            .await
            .expect("a search")
            .output
            .unwrap();

        assert!(found.contains("ignored.rs:1: let needle = 1;"), "{found}");
    }

    #[tokio::test]
    async fn an_offline_chat_refuses_curl() {
        let directory = tempdir().unwrap();
        let arguments = json!(["https://example.com"]);

        let vpn = Hold::on();
        let offline = run(
            "curl",
            arguments.clone(),
            &chat(directory.path(), Vec::new(), true),
        )
        .await;
        drop(vpn);
        assert!(
            matches!(&offline, Err(ToolError::Execution(message)) if message == CHAT_OFFLINE),
            "{offline:?}"
        );

        let _vpn = Hold::required_off();
        let live = run(
            "curl",
            arguments,
            &chat(directory.path(), Vec::new(), false),
        )
        .await;
        assert!(
            matches!(&live, Err(ToolError::Execution(message)) if message == OFFLINE),
            "{live:?}"
        );
    }

    /// Filling a tree past the cap is cheap on Linux and slow on APFS, so the
    /// cap is pinned on Linux.
    #[cfg(target_os = "linux")]
    mod entry_cap {
        use abnegate_agent::tool::ListFilesTool;

        use super::*;

        /// Most entries one crate walk looks at before it stops, as
        /// `abnegate_agent`'s private `MAXIMUM_WALK_ENTRIES` sets it.
        const MAXIMUM_WALK_ENTRIES: usize = 100_000;

        /// How many directories the tree past [`MAXIMUM_WALK_ENTRIES`] is
        /// spread over, so they can be filled side by side.
        const BRANCHES: usize = 16;

        fn sprawl(root: &Path) {
            let leaves = MAXIMUM_WALK_ENTRIES / BRANCHES + 1;
            std::thread::scope(|scope| {
                for branch in 0..BRANCHES {
                    scope.spawn(move || {
                        let directory = root.join(branch.to_string());
                        std::fs::create_dir(&directory).unwrap();
                        for leaf in 0..leaves {
                            std::fs::File::create(directory.join(leaf.to_string())).unwrap();
                        }
                    });
                }
            });
        }

        fn prune(root: &Path) {
            std::thread::scope(|scope| {
                for branch in 0..BRANCHES {
                    scope.spawn(move || std::fs::remove_dir_all(root.join(branch.to_string())));
                }
            });
        }

        #[tokio::test]
        async fn a_listing_past_the_crates_entry_cap_stops_early() {
            let directory = tempdir().unwrap();
            let root = directory.path().to_path_buf();
            tokio::task::spawn_blocking(move || sprawl(&root))
                .await
                .expect("the sprawling tree");
            let context = chat(directory.path(), Vec::new(), false);

            let listed = ListFilesTool
                .execute(json!({"path": ".", "recursive": true}), &context)
                .await
                .expect("a listing past the cap still answers")
                .output
                .unwrap();
            let root = directory.path().to_path_buf();
            tokio::task::spawn_blocking(move || prune(&root))
                .await
                .expect("the sprawling tree is pruned");

            let tail = &listed[listed.len().saturating_sub(200)..];
            assert!(
                listed.ends_with("\n[listing stopped early: too many entries]"),
                "{tail}"
            );
        }
    }
}
