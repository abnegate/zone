//! Telling a pull request an agent opened from one it only mentioned.

use std::sync::LazyLock;

use abnegate_agent_cli::Output;
use regex::Regex;
use serde_json::Value;
use zone_core::llm::Toolset;

/// The agents' names for the tool that runs a shell command.
const SHELL_TOOLS: [&str; 2] = ["Bash", "command_execution"];

/// What another MCP server's pull-request-opening tool is called, after its
/// server's prefix.
const CREATE_TOOL: &str = "create_pull_request";

static PULL_REQUEST_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://[A-Za-z0-9.-]+(?::[0-9]+)?/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/pull/[0-9]+")
        .expect("a valid pull request pattern")
});

static CREATES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s;&|(`'\x22])gh\s+pr\s+create\b").expect("a valid command pattern")
});

/// The pull request `output` opened, if its call opened one.
///
/// Only a call that succeeded and whose job is opening one counts: `gh pr
/// create` in a shell, or another MCP server's `create_pull_request`. A link
/// printed by anything else, such as `gh pr view` or a search, was only
/// mentioned. A shell prints its new pull request's link last, so a command
/// that printed others before it is read for the last one.
pub fn opened(output: &Output) -> Option<String> {
    if !output.succeeded {
        return None;
    }
    let name = output.call.function.name.as_str();
    let mut links = PULL_REQUEST_URL
        .find_iter(&output.text)
        .map(|link| link.as_str().to_string());
    if SHELL_TOOLS.contains(&name) {
        return command(&output.call.function.arguments)
            .filter(|command| CREATES.is_match(command))
            .and_then(|_| links.last());
    }
    let zone = format!("mcp__{}__", Toolset::SERVER);
    if name.starts_with("mcp__") && !name.starts_with(&zone) && name.ends_with(CREATE_TOOL) {
        return links.next();
    }
    None
}

/// The command a shell call ran, which codex may report as a list of words.
fn command(arguments: &str) -> Option<String> {
    let arguments: Value = serde_json::from_str(arguments).ok()?;
    match arguments.get("command")? {
        Value::String(command) => Some(command.clone()),
        Value::Array(words) => Some(
            words
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_llm::ToolCall;
    use serde_json::json;

    const LINK: &str = "https://github.com/acme/widgets/pull/7";

    fn shell(name: &str, command: Value, text: &str, succeeded: bool) -> Output {
        Output::new(
            ToolCall::function("call_1", name, json!({ "command": command }).to_string()),
            text,
            succeeded,
        )
    }

    #[test]
    fn a_shell_that_ran_gh_pr_create_opened_the_link_it_printed() {
        for (name, command) in [
            (
                "Bash",
                json!("git push -u origin feature && gh pr create --fill"),
            ),
            (
                "command_execution",
                json!("/bin/zsh -lc 'gh pr create --title Add'"),
            ),
            (
                "command_execution",
                json!(["bash", "-lc", "gh pr create --fill"]),
            ),
        ] {
            let output = shell(
                name,
                command,
                &format!("Creating pull request for feature into main in acme/widgets\n\n{LINK}\n"),
                true,
            );
            assert_eq!(opened(&output).as_deref(), Some(LINK), "{name}");
        }
    }

    #[test]
    fn a_link_another_command_printed_was_only_mentioned() {
        for command in [
            "gh pr view 7",
            "gh pr list --json url",
            "echo 'gh pr created yesterday'",
            "git push",
        ] {
            let output = shell("Bash", json!(command), LINK, true);
            assert_eq!(opened(&output), None, "{command}");
        }
    }

    #[test]
    fn a_create_that_failed_opened_nothing_even_when_it_names_a_link() {
        let output = shell(
            "Bash",
            json!("gh pr create --fill"),
            &format!(
                "a pull request for branch \"feature\" into branch \"main\" already exists:\n{LINK}"
            ),
            false,
        );
        assert_eq!(opened(&output), None);
    }

    #[test]
    fn the_last_link_a_creating_command_printed_is_the_new_one() {
        let output = shell(
            "Bash",
            json!("gh pr view 3 --json url; gh pr create --fill"),
            &format!("https://github.com/acme/widgets/pull/3\n{LINK}"),
            true,
        );
        assert_eq!(opened(&output).as_deref(), Some(LINK));
    }

    #[test]
    fn another_servers_create_tool_opened_its_link_but_zones_own_is_recorded_elsewhere() {
        let created = json!({"html_url": LINK, "number": 7}).to_string();
        let github = Output::new(
            ToolCall::function("call_2", "mcp__github__create_pull_request", "{}"),
            created.clone(),
            true,
        );
        assert_eq!(opened(&github).as_deref(), Some(LINK));

        let zone = Output::new(
            ToolCall::function("call_3", "mcp__zone__create_pull_request", "{}"),
            created.clone(),
            true,
        );
        assert_eq!(opened(&zone), None);

        let read = Output::new(
            ToolCall::function("call_4", "mcp__github__get_pull_request", "{}"),
            created,
            true,
        );
        assert_eq!(opened(&read), None);
    }

    #[test]
    fn a_create_that_printed_no_link_opened_nothing() {
        let output = shell(
            "Bash",
            json!("gh pr create --fill"),
            "https://github.com/acme/widgets/pull/new/feature",
            true,
        );
        assert_eq!(opened(&output), None);
    }
}
