//! The tool context every chat and task run on the server works in.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use abnegate_agent::ToolContext;
use abnegate_agent::tool::Session;
use zone_core::application::application;
use zone_core::vpn::Offline;

use super::environment;

/// A context rooted at `working_directory` for `session`, withholding
/// `denied` from the file tools and reaching outside `working_directory`
/// only when `unrestricted`. Every child it starts is given the
/// [`environment::policy`] and routed through Zone's proxy, its files are
/// kept under `.zone/`, and `curl` and `wget` are held to the VPN gate, and
/// refused outright when `chat_offline`.
pub fn build(
    working_directory: PathBuf,
    session: Session,
    denied: Vec<PathBuf>,
    unrestricted: bool,
    maximum_file_size: usize,
    command_timeout: Duration,
    chat_offline: bool,
) -> ToolContext {
    let mut context = ToolContext::default()
        .within(working_directory)
        .with_session(session)
        .with_denied(denied)
        .with_application(application())
        .with_environment(environment::policy())
        .with_proxy(zone_routing::proxy())
        .with_public_web(Arc::new(Offline::new(chat_offline)));
    context.unrestricted = unrestricted;
    context.maximum_file_size = maximum_file_size;
    context.command_timeout = command_timeout;
    context
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use abnegate_agent::tool::job::{JobCommand, Jobs};
    use abnegate_agent::tool::{OFF_LIMITS, ReadFileTool, RunCommandTool, SearchCodeTool};
    use abnegate_agent::{Tool, ToolError, ToolResult};
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

    fn chat(working_directory: &Path, denied: Vec<PathBuf>, offline: bool) -> ToolContext {
        build(
            working_directory.to_path_buf(),
            Session::Chat(Uuid::new_v4()),
            denied,
            true,
            MAX_TOOL_FILE_BYTES,
            Duration::from_secs(TOOL_COMMAND_TIMEOUT_SECS),
            offline,
        )
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

        let context = build(
            directory.path().to_path_buf(),
            session,
            denied.clone(),
            false,
            42,
            Duration::from_secs(7),
            false,
        );

        assert_eq!(context.working_directory, directory.path());
        assert_eq!(context.session, session);
        assert_eq!(context.denied, denied);
        assert!(!context.unrestricted);
        assert_eq!(context.maximum_file_size, 42);
        assert_eq!(context.command_timeout, Duration::from_secs(7));
        assert!(!context.environment.inherits());
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

        assert!(found.starts_with("No matches found so far\n"), "{found}");
        assert!(found.contains("[search stopped early: "), "{found}");
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
}
