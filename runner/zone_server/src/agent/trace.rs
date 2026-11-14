//! How a tool's output reads as lines in the console's trace.

pub(crate) const RUN_COMMAND: &str = "run_command";
pub(crate) const RUN_SHELL: &str = "run_shell";
pub(crate) const STDOUT_LABEL: &str = "stdout:";
pub(crate) const STDERR_LABEL: &str = "stderr:";

/// The lines of a tool's output that carry something: never blank, and for a
/// command, never the label its report heads each captured stream with. Any
/// other tool's `stdout:` line is its own output.
pub(crate) fn content_lines<'a>(
    tool: &str,
    output: &'a str,
) -> impl Iterator<Item = &'a str> + use<'a> {
    let labelled = tool == RUN_COMMAND || tool == RUN_SHELL;
    output.lines().filter(move |line| {
        let line = line.trim();
        !line.is_empty() && !(labelled && (line == STDOUT_LABEL || line == STDERR_LABEL))
    })
}

#[cfg(test)]
mod tests {
    use abnegate_agent::ToolContext;
    use abnegate_agent::tool::{RunCommandTool, RunShellTool, Tool};
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn a_command_report_reads_as_its_content_without_the_stream_labels() {
        let directory = TempDir::new().expect("a working directory");
        let context = ToolContext::default().within(directory.path());
        let mut shell_context = ToolContext::default().within(directory.path());
        shell_context.unrestricted = true;

        let shell = RunShellTool
            .execute(
                json!({"command": "echo hello; echo careful >&2"}),
                &shell_context,
            )
            .await
            .expect("the shell runs");
        let command = RunCommandTool
            .execute(json!({"command": "echo", "args": ["hello"]}), &context)
            .await
            .expect("the command runs");

        let shell = shell.output.expect("the shell reports its output");
        let command = command.output.expect("the command reports its output");
        assert!(shell.contains(STDOUT_LABEL), "{shell}");
        assert!(shell.contains(STDERR_LABEL), "{shell}");
        assert_eq!(
            content_lines(RUN_SHELL, &shell).collect::<Vec<_>>(),
            ["hello", "careful"]
        );
        assert_eq!(
            content_lines(RUN_COMMAND, &command).collect::<Vec<_>>(),
            ["hello"]
        );
    }

    #[test]
    fn a_stream_label_another_tool_returned_is_its_own_content() {
        assert_eq!(
            content_lines("read_file", "stdout:\nreal").collect::<Vec<_>>(),
            ["stdout:", "real"]
        );
    }

    #[test]
    fn the_command_tool_names_are_the_crates() {
        assert_eq!(RunCommandTool.name(), RUN_COMMAND);
        assert_eq!(RunShellTool.name(), RUN_SHELL);
    }
}
