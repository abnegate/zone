//! The tool context a run on the operator's own machine works in, such as the
//! CLI's: the whole environment, as the operator's own shell has it.

use std::path::PathBuf;
use std::sync::Arc;

use abnegate_agent::ToolContext;

use crate::application::application;
use crate::vpn::Offline;

/// A context rooted at `working_directory` that hands every child this
/// process's whole environment, routes it through Zone's proxy, keeps its
/// files under `.zone/`, and holds `curl` and `wget` to the VPN gate.
pub fn context(working_directory: impl Into<PathBuf>) -> ToolContext {
    ToolContext::default()
        .within(working_directory)
        .with_application(application())
        .inherit_environment()
        .with_proxy(zone_routing::proxy())
        .with_public_web(Arc::new(Offline::new(false)))
}

#[cfg(test)]
mod tests {
    use abnegate_agent::tool::RunCommandTool;
    use abnegate_agent::tool::job::log_path;
    use abnegate_agent::{Tool, ToolError, ToolResult};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::variables::Variables;
    use crate::vpn::{Hold, OFFLINE};

    const MARKER: &str = "ZONE_LOCAL_TEST_MARKER";
    const MARKED: &str = "the-whole-environment";
    const PROXY: &str = "http://127.0.0.1:28888";
    const PROXY_VARIABLES: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ];

    async fn printed(names: &[&str], context: &ToolContext) -> ToolResult {
        RunCommandTool::with_commands(&["printenv"])
            .execute(json!({"command": "printenv", "args": names}), context)
            .await
            .expect("printenv runs")
    }

    #[tokio::test]
    async fn a_local_run_hands_commands_the_whole_environment() {
        let directory = tempdir().unwrap();
        let mut variables = Variables::lock();
        variables.set(MARKER, MARKED);

        let local = printed(&[MARKER], &context(directory.path())).await;
        let allowlisted =
            printed(&[MARKER], &ToolContext::default().within(directory.path())).await;

        assert!(local.success, "{local:?}");
        assert_eq!(
            local.output.as_deref(),
            Some(format!("stdout:\n{MARKED}\n").as_str())
        );
        assert!(!allowlisted.success, "{allowlisted:?}");
    }

    #[tokio::test]
    async fn a_local_run_routes_through_zones_proxy() {
        let directory = tempdir().unwrap();
        let mut variables = Variables::lock();
        variables.set(zone_routing::PROXY_URL_VARIABLE, PROXY);

        let context = context(directory.path());

        assert!(context.proxy.is_some());
        for name in PROXY_VARIABLES {
            let result = printed(&[name], &context).await;
            assert_eq!(
                result.output,
                Some(format!("stdout:\n{PROXY}\n")),
                "{name}: {result:?}"
            );
        }
        let bypass = printed(&["NO_PROXY"], &context).await;
        assert_eq!(
            bypass.output,
            Some(format!("stdout:\n{}\n", zone_routing::BYPASS.join(",")))
        );
    }

    #[test]
    fn a_local_run_keeps_its_files_under_dot_zone() {
        let directory = tempdir().unwrap();
        let context = context(directory.path());

        assert_eq!(context.application, application());
        assert_eq!(
            log_path(&context.working_directory, &context.application, "job_x"),
            directory.path().join(".zone/jobs/job_x.log")
        );
    }

    #[tokio::test]
    async fn a_local_run_has_the_public_web_gate() {
        let directory = tempdir().unwrap();
        let _vpn = Hold::required_off();
        let context = context(directory.path());

        let gate = context.public_web.as_ref().expect("a public-web gate");
        assert_eq!(gate.refusal().as_deref(), Some(OFFLINE));
        let error = RunCommandTool::with_commands(&["curl"])
            .execute(
                json!({"command": "curl", "args": ["https://example.com"]}),
                &context,
            )
            .await
            .expect_err("curl is refused while the VPN is required and off");
        assert!(
            matches!(&error, ToolError::Execution(message) if message == OFFLINE),
            "{error}"
        );
    }
}
