//! Command execution tool

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tool_runner::Proxy;

use super::{
    ERROR_PREFIX, MAX_PREVIEW_CHARS, MAX_TOOL_OUTPUT_CHARS, REASON_PARAM, Tier, Tool, ToolContext,
    ToolError, ToolResult, excerpt, reason_property, trim_middle,
};

/// Programs [`RunCommandTool`] may spawn, resolved on the child's `PATH`.
///
/// Matched against the whole `command`, never its last path segment: an agent
/// may write a file into `cwd`, so a basename match would admit `./cargo` and
/// then run whatever that file is.
const ALLOWED_COMMANDS: &[&str] = &[
    "cargo", "rustc", "npm", "npx", "yarn", "pnpm", "node", "deno", "bun", "make", "cmake",
    "gradle", "mvn", "maven", "go", "python", "python3", "pip", "pip3", "poetry", "uv", "ruby",
    "gem", "bundle", "rake", "dotnet", "msbuild", "git", "gh", "hub", "ls", "cat", "head", "tail",
    "grep", "find", "wc", "sort", "uniq", "diff", "tree", "file", "stat", "pwd", "which",
    "whereis", "pytest", "jest", "mocha", "rspec", "phpunit", "echo", "printf", "date", "env",
    "true", "false", "test", "curl", "wget", "jq", "yq", "docker",
];

/// Run a shell command
pub struct RunCommandTool;

#[derive(Debug, Deserialize)]
struct RunCommandParams {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    max_output_chars: Option<u64>,
}

const MAX_OUTPUT_PARAM: &str = "max_output_chars";

/// Cap on returned output, so one noisy command cannot fill the context
/// window. Spends the shared tool budget, which the transcript cap sits above,
/// so what the tool keeps is what the model is given even once the exit-code
/// line and the `Error: ` prefix are wrapped around it.
const MAX_SHELL_OUTPUT_CHARS: usize = MAX_TOOL_OUTPUT_CHARS;

/// Floor for a caller-supplied cap, below which neither end of the output
/// holds enough to diagnose anything.
const MIN_SHELL_OUTPUT_CHARS: usize = 500;

/// Resolve `max_output_chars` against the built-in cap.
///
/// Reduce-only: a caller may spend fewer characters than the default, never
/// more, so the constant stays the ceiling on what one call can cost.
fn clamp_output_chars(requested: Option<u64>) -> usize {
    match requested {
        Some(chars) => {
            chars.clamp(MIN_SHELL_OUTPUT_CHARS as u64, MAX_SHELL_OUTPUT_CHARS as u64) as usize
        }
        None => MAX_SHELL_OUTPUT_CHARS,
    }
}

fn max_output_property() -> Value {
    json!({
        "type": "integer",
        // Parsed into a u64, so a negative fails the call instead of clamping.
        "minimum": 0,
        "description": format!(
            "Cap returned output at this many characters, keeping head and tail. Default \
             {MAX_SHELL_OUTPUT_CHARS}; larger values clamp down, values under \
             {MIN_SHELL_OUTPUT_CHARS} clamp up."
        )
    })
}

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Returns stdout/stderr output. Use for running tests, builds, git commands, etc."
    }

    fn tier(&self) -> Tier {
        Tier::Host
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let params: RunCommandParams = serde_json::from_value(params.clone()).ok()?;
        let line = std::iter::once(params.command)
            .chain(params.args)
            .collect::<Vec<String>>()
            .join(" ");
        Some(run_preview(&line, params.cwd.as_deref()))
    }

    fn timeout(&self, context: &ToolContext) -> Duration {
        // Loose enough never to pre-empt the per-call limit applied below.
        Duration::from_secs(context.command_timeout + 30)
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to run (e.g., 'cargo', 'npm', 'git')"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments to pass to the command"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the command (relative to project root)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 300)"
                },
                MAX_OUTPUT_PARAM: max_output_property(),
                REASON_PARAM: reason_property()
            },
            "required": ["command", REASON_PARAM]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: RunCommandParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        tracing::debug!(
            tool = self.name(),
            reason_given = params
                .reason
                .as_deref()
                .is_some_and(|why| !why.trim().is_empty()),
            "Running tool"
        );

        if !ALLOWED_COMMANDS.contains(&params.command.as_str()) {
            return Err(ToolError::Execution(format!(
                "Command '{}' is not in the allowed list. Name a program, not a path: cargo, npm, git, python, etc.",
                params.command
            )));
        }

        // Additional security: Check for shell metacharacters in arguments
        // that could allow command injection
        let dangerous_patterns = ["$(", "`", "&&", "||", ";", "|", ">", "<", "\n", "\r"];
        for arg in &params.args {
            for pattern in &dangerous_patterns {
                if arg.contains(pattern) {
                    return Err(ToolError::Execution(format!(
                        "Argument contains potentially dangerous pattern: '{}'",
                        pattern
                    )));
                }
            }
        }

        // Determine working directory
        let cwd = if let Some(dir) = &params.cwd {
            context.cwd.join(dir)
        } else {
            context.cwd.clone()
        };

        // Build command
        let mut cmd = Command::new(&params.command);
        cmd.args(&params.args)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        cmd.env_clear();
        for (key, value) in &context.env {
            cmd.env(key, value);
        }
        Proxy::from_env().apply(&mut cmd);

        // Execute with timeout
        let timeout_duration =
            Duration::from_secs(params.timeout_secs.unwrap_or(context.command_timeout));

        let output = match timeout(timeout_duration, cmd.output()).await {
            Ok(result) => {
                result.map_err(|e| ToolError::Execution(format!("Failed to execute: {}", e)))?
            }
            Err(_) => {
                return Err(ToolError::Execution(format!(
                    "Command timed out after {} seconds",
                    timeout_duration.as_secs()
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        if !stdout.is_empty() {
            result.push_str("stdout:\n");
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str("stderr:\n");
            result.push_str(&stderr);
        }

        if result.is_empty() {
            result = "(no output)".to_string();
        }

        let output_chars = clamp_output_chars(params.max_output_chars);

        if output.status.success() {
            Ok(ToolResult::success(trim_middle(&result, output_chars)))
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            // `to_message` prefixes a failure, so the cap the caller asked for
            // has to cover that too.
            Ok(ToolResult::error(trim_middle(
                &format!("Command exited with code {}\n\n{}", code, result),
                output_chars.saturating_sub(ERROR_PREFIX.chars().count()),
            )))
        }
    }
}

/// The command line as it will run, for an approval card.
///
/// The command is what the reader is deciding on, so it keeps the whole budget
/// and the directory is appended after it rather than put in front of it.
fn run_preview(line: &str, cwd: Option<&str>) -> String {
    let command = excerpt(line, MAX_PREVIEW_CHARS);
    match cwd {
        Some(cwd) => format!("Run `{command}` in {cwd}."),
        None => format!("Run `{command}`."),
    }
}

/// Run a command through a real shell, with no allow-list.
///
/// [`RunCommandTool`] spawns a binary from a fixed list and rejects shell
/// metacharacters, which rules out pipes, redirection and chaining. That is
/// the right trade for a task runner working in a checkout. This is the tool
/// for callers who have deliberately asked for an unrestricted agent: it runs
/// whatever it is given, as whoever runs the process. Register it only
/// alongside a [`ToolContext`] with `unrestricted` set.
pub struct RunShellTool;

#[derive(Debug, Deserialize)]
struct RunShellParams {
    command: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    max_output_chars: Option<u64>,
}

/// Longest a single shell command may run, whatever it asks for.
const MAX_SHELL_TIMEOUT_SECS: u64 = 900;

/// Longest a call may spend blocked on `sleep`.
///
/// A run that has produced nothing for this long is announced as stalled, so a
/// longer sleep reads as a wedged run rather than a waiting one. Waiting past
/// it belongs between calls, where the loop can still see what is happening.
pub const MAX_SLEEP_SECS: u64 = 60;

/// Where one command in a line ends and the next begins, plus the grouping
/// characters a `sleep` can sit behind.
const COMMAND_BOUNDARIES: [char; 9] = [';', '&', '|', '\n', '(', ')', '{', '}', '`'];

/// Seconds `command` blocks on `sleep` for, adding up every `sleep` it holds.
///
/// The sleeps are summed rather than compared: `sleep 40; sleep 40` blocks for
/// eighty seconds, and a cap that only ever saw the longer of the two would
/// wave it through. A branch that will not be taken is counted too, which
/// overstates the wait rather than understating it.
///
/// Only a `sleep` in command position is visible here. One reached through a
/// script, an interpreter or a variable is left to the per-call timeout, which
/// this sits in front of rather than replaces.
fn total_sleep(command: &str) -> Option<f64> {
    let sleeps: Vec<f64> = command
        .split(COMMAND_BOUNDARIES)
        .filter_map(|segment| {
            let mut words = segment
                .split_whitespace()
                .skip_while(|word| word.contains('='));
            let program = words.next()?.rsplit('/').next()?;
            (program == "sleep").then(|| words.map_while(sleep_seconds).sum::<f64>())
        })
        .collect();
    (!sleeps.is_empty()).then(|| sleeps.iter().sum())
}

/// One `sleep` operand in seconds: a count with an optional s, m, h or d.
///
/// A bare number is seconds and several operands add up, both as `sleep` reads
/// them. An operand that is not a duration ends the sum rather than the call:
/// what a variable holds is not knowable from here.
///
/// A negative operand counts as nothing. `sleep` rejects one rather than
/// running time backwards, and letting it subtract would have let a caller pay
/// for a long wait with a short one that never happens.
fn sleep_seconds(operand: &str) -> Option<f64> {
    let scale = match operand.chars().last()? {
        's' => 1.0,
        'm' => 60.0,
        'h' => 3_600.0,
        'd' => 86_400.0,
        _ => {
            return operand
                .parse::<f64>()
                .ok()
                .filter(|seconds| seconds.is_finite())
                .map(|seconds| seconds.max(0.0));
        }
    };
    let count = operand[..operand.len() - 1].parse::<f64>().ok()?;
    count.is_finite().then_some((count * scale).max(0.0))
}

#[async_trait]
impl Tool for RunShellTool {
    fn name(&self) -> &str {
        "run_shell"
    }

    fn description(&self) -> &str {
        "Run a shell command and return its stdout, stderr and exit code. Runs through `sh -c`, \
         so pipes, redirection and chaining work. Use for builds, tests, git and package managers."
    }

    fn tier(&self) -> Tier {
        Tier::Host
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let params: RunShellParams = serde_json::from_value(params.clone()).ok()?;
        Some(run_preview(&params.command, params.cwd.as_deref()))
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": format!(
                        "Shell command to run, e.g. 'cargo test 2>&1 | tail -40'. It may not \
                         block on sleep for more than {MAX_SLEEP_SECS} seconds: to wait longer, \
                         return and check again in a later call."
                    )
                },
                "cwd": {
                    "type": "string",
                    "description": "Directory to run in. Absolute, or relative to the working directory."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Wall-clock limit in seconds. Default 120, maximum 900."
                },
                MAX_OUTPUT_PARAM: max_output_property(),
                REASON_PARAM: reason_property()
            },
            "required": ["command", REASON_PARAM]
        })
    }

    fn timeout(&self, _context: &ToolContext) -> Duration {
        // Loose enough never to pre-empt the per-call limit enforced below.
        Duration::from_secs(MAX_SHELL_TIMEOUT_SECS + 30)
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: RunShellParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        tracing::debug!(
            tool = self.name(),
            reason_given = params
                .reason
                .as_deref()
                .is_some_and(|why| !why.trim().is_empty()),
            "Running tool"
        );

        if params.command.trim().is_empty() {
            return Err(ToolError::InvalidParams("Command is empty".to_string()));
        }

        if let Some(seconds) = total_sleep(&params.command)
            && seconds > MAX_SLEEP_SECS as f64
        {
            return Err(ToolError::Execution(format!(
                "This command sleeps for {seconds} seconds, and a call may block on sleep for at \
                 most {MAX_SLEEP_SECS}. Return without waiting and check again in a later call."
            )));
        }

        let cwd = match &params.cwd {
            Some(dir) => context.cwd.join(dir),
            None => context.cwd.clone(),
        };

        let limit = Duration::from_secs(
            params
                .timeout_secs
                .unwrap_or(120)
                .clamp(1, MAX_SHELL_TIMEOUT_SECS),
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&params.command)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Without this, a command that outlives its timeout keeps running
            // after we have stopped waiting for it.
            .kill_on_drop(true);

        cmd.env_clear();
        for (key, value) in &context.env {
            cmd.env(key, value);
        }
        Proxy::from_env().apply(&mut cmd);

        let output = match timeout(limit, cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(ToolError::Execution(format!("Failed to execute: {}", e))),
            Err(_) => {
                return Err(ToolError::Execution(format!(
                    "Command timed out after {} seconds and was killed",
                    limit.as_secs()
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut report = String::new();
        match output.status.code() {
            Some(0) => {}
            Some(code) => report.push_str(&format!("Exit code: {}\n", code)),
            None => report.push_str("Killed by signal\n"),
        }
        if !stdout.trim().is_empty() {
            report.push_str(&format!("stdout:\n{}\n", stdout));
        }
        if !stderr.trim().is_empty() {
            report.push_str(&format!("stderr:\n{}\n", stderr));
        }
        if report.is_empty() {
            report.push_str("(no output)");
        }

        // A non-zero exit is an observation, not a tool failure: the model
        // should read the compiler error rather than conclude the tool broke.
        Ok(ToolResult::success(trim_middle(
            report.trim_end(),
            clamp_output_chars(params.max_output_chars),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::MAX_TOOL_MESSAGE_CHARS;
    use crate::tools::test_support::captured_logs;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn create_test_context() -> ToolContext {
        ToolContext {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            env: HashMap::new(),
            max_file_size: 1024 * 1024,
            command_timeout: 30,
            unrestricted: false,
        }
    }

    /// Both shelling tools hand the child only what the context names.
    ///
    /// The context environment *is* the allowlist a caller builds: the server
    /// narrows it to a fixed set of names precisely because its own process
    /// holds the database URL, the JWT and encryption keys and the provider
    /// keys. A child that inherited the parent's environment would print all
    /// of it into tool output, so `env_clear` is what makes the caller's
    /// allowlist an allowlist.
    #[tokio::test]
    async fn shelling_tools_give_the_child_only_the_context_environment() {
        const MARKER: &str = "ZONE_COMMAND_ENVIRONMENT_MARKER";
        unsafe { std::env::set_var(MARKER, "must-not-reach-a-child") };

        let mut context = create_test_context();
        context.env = HashMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )]);

        let command = RunCommandTool
            .execute(json!({"command": "env"}), &context)
            .await
            .unwrap();
        let shell = RunShellTool
            .execute(json!({"command": "env"}), &context)
            .await
            .unwrap();
        unsafe { std::env::remove_var(MARKER) };

        for result in [command, shell] {
            assert!(result.success, "{result:?}");
            let output = result.output.unwrap();
            assert!(output.contains("PATH="), "the tool did not run: {output}");
            assert!(
                !output.contains(MARKER),
                "the process environment reached the child: {output}"
            );
            // `sh` computes these from the working directory it was given.
            const SHELL_OWN: &[&str] = &["PWD", "SHLVL", "_"];
            for (name, _) in output.lines().filter_map(|line| line.split_once('=')) {
                assert!(
                    context.env.contains_key(name)
                        || name.to_ascii_uppercase().ends_with("_PROXY")
                        || SHELL_OWN.contains(&name),
                    "{name} is not on the context environment and must not have survived"
                );
            }
        }
    }

    fn shell_test_context() -> ToolContext {
        let mut context = create_test_context();
        context.unrestricted = true;
        context.env.insert(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        );
        context
    }

    #[tokio::test]
    async fn proxy_overrides_command_and_shell_environment() {
        const NAME: &str = "tools::command::tests::proxy_overrides_command_and_shell_environment";
        if std::env::var("ZONE_PROXY_TEST_CHILD").as_deref() != Ok(NAME) {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture"])
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("ZONE_PROXY_TEST_CHILD", NAME)
                .env("TOOL_RUNNER_PROXY_URL", "http://127.0.0.1:28888")
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let mut context = create_test_context();
        context.env = HashMap::from([
            ("HTTPS_PROXY".to_string(), "http://wrong:8888".to_string()),
            ("http_proxy".to_string(), "http://wrong:8888".to_string()),
            ("NO_PROXY".to_string(), "*".to_string()),
            ("no_proxy".to_string(), "*".to_string()),
            ("TOOL_RUNNER_PROXY_URL".to_string(), "".to_string()),
        ]);
        let command = RunCommandTool
            .execute(json!({"command": "env"}), &context)
            .await
            .unwrap();
        let shell = RunShellTool
            .execute(json!({"command": "env"}), &context)
            .await
            .unwrap();
        for result in [command, shell] {
            assert!(result.success, "{result:?}");
            let output = result.output.unwrap();
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                assert!(
                    output
                        .lines()
                        .any(|line| line == format!("{key}=http://127.0.0.1:28888")),
                    "{output}"
                );
            }
            assert!(
                !output
                    .lines()
                    .any(|line| line == "NO_PROXY=*" || line == "no_proxy=*")
            );
            assert!(output.contains("NO_PROXY=localhost,127.0.0.1,::1"));
        }
    }

    #[test]
    fn test_run_command_tool_metadata() {
        let tool = RunCommandTool;
        assert_eq!(tool.name(), "run_command");
        assert!(!tool.description().is_empty());

        let schema = tool.parameters_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }

    #[tokio::test]
    async fn test_run_command_echo() {
        let tool = RunCommandTool;
        let context = create_test_context();

        let result = tool
            .execute(
                serde_json::json!({"command": "echo", "args": ["hello", "world"]}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.unwrap().contains("hello world"));
    }

    #[tokio::test]
    async fn test_run_command_pwd() {
        let tool = RunCommandTool;
        let context = create_test_context();

        let result = tool
            .execute(serde_json::json!({"command": "pwd"}), &context)
            .await
            .unwrap();

        assert!(result.success);
        // Should contain some path
        assert!(result.output.unwrap().contains("/"));
    }

    #[tokio::test]
    async fn test_run_command_not_found() {
        let tool = RunCommandTool;
        let context = create_test_context();

        let result = tool
            .execute(
                serde_json::json!({"command": "nonexistent_command_xyz_12345"}),
                &context,
            )
            .await;

        // Should fail because command doesn't exist
        assert!(result.is_err());
    }

    /// The allowlist was matched against the last path segment while the whole
    /// string was executed, so a file the agent had just written into `cwd` and
    /// named `cargo` satisfied the list and then ran.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_allowed_name_on_a_path_is_not_an_allowed_program() {
        use std::os::unix::fs::PermissionsExt;

        const MARKER: &str = "arbitrary-execution-marker";
        let directory = tempfile::tempdir().expect("a temporary directory");
        let impostor = directory.path().join("cargo");
        std::fs::write(&impostor, format!("#!/bin/sh\necho {MARKER}\n")).unwrap();
        std::fs::set_permissions(&impostor, std::fs::Permissions::from_mode(0o755)).unwrap();

        let context = ToolContext {
            cwd: directory.path().canonicalize().unwrap(),
            env: HashMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            ..ToolContext::default()
        };

        for command in [
            "./cargo".to_string(),
            "cargo/../cargo".to_string(),
            impostor.to_string_lossy().into_owned(),
        ] {
            let error = RunCommandTool
                .execute(
                    serde_json::json!({"command": command, "args": []}),
                    &context,
                )
                .await
                .expect_err("a path must not satisfy the allowlist");
            assert!(
                error.to_string().contains("not in the allowed list"),
                "{command}: {error}"
            );
            assert!(
                !error.to_string().contains(MARKER),
                "{command} ran: {error}"
            );
        }
    }

    #[tokio::test]
    async fn test_run_command_not_in_allowlist() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // Test command not in allowlist
        let result = tool
            .execute(
                serde_json::json!({"command": "rm", "args": ["-rf", "/"]}),
                &context,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not in the allowed list"));
    }

    #[tokio::test]
    async fn test_run_command_dangerous_not_allowed() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // sudo is not in allowlist
        let result = tool
            .execute(
                serde_json::json!({"command": "sudo", "args": ["ls"]}),
                &context,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not in the allowed list")
        );
    }

    #[tokio::test]
    async fn test_run_command_shell_injection_blocked() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // Test shell metacharacter injection
        let result = tool
            .execute(
                serde_json::json!({"command": "echo", "args": ["hello; rm -rf /"]}),
                &context,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("dangerous pattern")
        );
    }

    #[tokio::test]
    async fn test_run_command_pipe_injection_blocked() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // Test pipe injection
        let result = tool
            .execute(
                serde_json::json!({"command": "echo", "args": ["hello | cat /etc/passwd"]}),
                &context,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("dangerous pattern")
        );
    }

    #[tokio::test]
    async fn test_run_command_command_substitution_blocked() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // Test command substitution
        let result = tool
            .execute(
                serde_json::json!({"command": "echo", "args": ["$(whoami)"]}),
                &context,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("dangerous pattern")
        );
    }

    #[tokio::test]
    async fn test_run_command_allowed_commands() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // These commands should be allowed (we don't execute them, just check they pass validation)
        let allowed = ["cargo", "npm", "git", "python", "go", "ls", "cat"];
        for cmd in allowed {
            let result = tool
                .execute(
                    serde_json::json!({"command": cmd, "args": ["--version"]}),
                    &context,
                )
                .await;
            // May fail due to command not being installed, but should not fail security check
            if let Err(e) = &result {
                assert!(
                    !e.to_string().contains("not in the allowed list"),
                    "Command {} should be allowed",
                    cmd
                );
            }
        }
    }

    #[tokio::test]
    async fn test_run_command_with_exit_code() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // Command that fails (exit code 1)
        let result = tool
            .execute(serde_json::json!({"command": "false"}), &context)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_run_command_with_stderr() {
        let tool = RunCommandTool;
        let context = create_test_context();

        // ls on nonexistent dir should produce stderr
        let result = tool
            .execute(
                serde_json::json!({"command": "ls", "args": ["/nonexistent_path_xyz_12345"]}),
                &context,
            )
            .await
            .unwrap();

        assert!(!result.success);
        let output = result.error.unwrap();
        assert!(output.contains("stderr"));
    }

    // Note: The timeout test is removed because:
    // 1. "sleep" is not in the allowlist for security
    // 2. Python scripts would trigger the dangerous pattern check for semicolons
    // 3. In a real environment, the timeout functionality is tested via integration tests
    // The timeout logic itself is straightforward (tokio::time::timeout) and is
    // covered by the RunCommandTool's implementation which uses it correctly.

    #[test]
    fn test_run_command_tool_definition() {
        let tool = RunCommandTool;
        let def = tool.to_definition();

        assert_eq!(def.tool_type, "function");
        assert_eq!(def.function.name, "run_command");
        assert!(def.function.description.contains("shell"));
    }

    #[test]
    fn run_command_params_read_the_reason() {
        let params: RunCommandParams = serde_json::from_value(json!({
            "command": "cargo",
            "args": ["test"],
            "reason": "Check the suite still passes before committing."
        }))
        .unwrap();

        assert_eq!(
            params.reason.as_deref(),
            Some("Check the suite still passes before committing.")
        );
    }

    #[tokio::test]
    async fn run_command_accepts_a_call_carrying_a_reason() {
        let result = RunCommandTool
            .execute(
                json!({
                    "command": "echo",
                    "args": ["hello"],
                    "reason": "Show the user what the tool returns."
                }),
                &create_test_context(),
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn run_command_without_a_reason_still_runs() {
        let result = RunCommandTool
            .execute(
                json!({"command": "echo", "args": ["hello"]}),
                &create_test_context(),
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("hello"));
    }

    #[test]
    fn run_shell_params_read_the_reason() {
        let params: RunShellParams = serde_json::from_value(json!({
            "command": "cargo test 2>&1 | tail -40",
            "reason": "Check the suite still passes before committing."
        }))
        .unwrap();

        assert_eq!(
            params.reason.as_deref(),
            Some("Check the suite still passes before committing.")
        );
    }

    #[tokio::test]
    async fn run_shell_accepts_a_call_carrying_a_reason() {
        let result = RunShellTool
            .execute(
                json!({
                    "command": "echo hello",
                    "reason": "Show the user what the tool returns."
                }),
                &shell_test_context(),
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn run_shell_without_a_reason_still_runs() {
        let result = RunShellTool
            .execute(json!({"command": "echo hello"}), &shell_test_context())
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("hello"));
    }

    /// `sleep` reads a bare number as seconds and a suffix as its unit, and
    /// adds its operands up. Reading them the same way is what makes the cap
    /// land on the wait that actually happens.
    #[test]
    fn a_sleep_is_measured_the_way_sleep_itself_reads_its_operands() {
        assert_eq!(total_sleep("sleep 30"), Some(30.0));
        assert_eq!(total_sleep("sleep 0.5"), Some(0.5));
        assert_eq!(total_sleep("sleep 2m"), Some(120.0));
        assert_eq!(total_sleep("sleep 1h"), Some(3_600.0));
        assert_eq!(total_sleep("sleep 1d"), Some(86_400.0));
        assert_eq!(total_sleep("sleep 40 40"), Some(80.0));
        assert_eq!(total_sleep("echo hello"), None);
        assert_eq!(total_sleep("echo sleep 900"), None);
    }

    /// The wait is what counts, not the shape of the line it hides in: a
    /// segment reached by a pipe, a chain, a subshell or a path is still a
    /// segment whose command is `sleep`, and every one of them adds to the
    /// wait the caller is about to sit through.
    #[test]
    fn a_sleep_is_found_wherever_a_command_can_start() {
        assert_eq!(total_sleep("cargo build && sleep 300"), Some(300.0));
        assert_eq!(total_sleep("sleep 300; cargo test"), Some(300.0));
        assert_eq!(total_sleep("sleep 10 || sleep 300"), Some(310.0));
        assert_eq!(total_sleep("(sleep 300)"), Some(300.0));
        assert_eq!(total_sleep("{ sleep 300; }"), Some(300.0));
        assert_eq!(total_sleep("/bin/sleep 300"), Some(300.0));
        assert_eq!(total_sleep("DELAY=1 sleep 300"), Some(300.0));
        assert_eq!(total_sleep("sleep 300 | cat"), Some(300.0));
        assert_eq!(total_sleep("cargo build & sleep 300"), Some(300.0));
    }

    /// The cap is on how long the call blocks, and a line blocks for the sum
    /// of its sleeps. Comparing only the longest one let a call wait for as
    /// many multiples of the cap as it cared to write out.
    #[test]
    fn sleeps_in_sequence_add_up_to_the_wait_the_cap_is_measured_against() {
        assert_eq!(total_sleep("sleep 40; sleep 40"), Some(80.0));
        assert_eq!(total_sleep("sleep 30 && sleep 30 && sleep 30"), Some(90.0));
        assert_eq!(total_sleep("sleep 20 | cat; sleep 50"), Some(70.0));
    }

    /// An operand this cannot read is not an excuse to reject the call. The
    /// per-call timeout is still behind it, and refusing what might be a
    /// one-second wait would cost more than letting it through.
    /// A negative operand is not a wait to be credited against a real one.
    /// `sleep -100; sleep 120` summed to twenty and was let through, and then
    /// `sh` failed the first segment and blocked for the full two minutes on
    /// the second.
    #[test]
    fn a_negative_operand_buys_no_credit_against_a_real_wait() {
        assert_eq!(total_sleep("sleep -100; sleep 120"), Some(120.0));
        assert_eq!(total_sleep("sleep -100"), Some(0.0));
        assert_eq!(total_sleep("sleep -5m"), Some(0.0));
        assert_eq!(total_sleep("sleep -100 120"), Some(120.0));
    }

    #[test]
    fn an_unreadable_operand_ends_the_sum_rather_than_the_call() {
        assert_eq!(total_sleep("sleep $DELAY"), Some(0.0));
        assert_eq!(total_sleep("sleep 30 $DELAY 300"), Some(30.0));
    }

    /// The task loop announces a stall after the same interval, so a call that
    /// blocks past it would look wedged rather than waiting.
    ///
    /// The one-second limit is what makes the refusal visible: without the cap
    /// the call reaches the shell and fails on the limit instead, so the two
    /// outcomes cannot be confused for one another.
    #[tokio::test]
    async fn a_shell_call_may_not_block_on_sleep_past_the_cap() {
        let error = RunShellTool
            .execute(
                json!({
                    "command": "sleep 300",
                    "timeout_secs": 1,
                    "reason": "Wait for the deploy."
                }),
                &shell_test_context(),
            )
            .await
            .expect_err("a sleep past the cap is refused");

        let message = error.to_string();
        assert!(message.contains("300"), "{message}");
        assert!(
            message.contains(&MAX_SLEEP_SECS.to_string()),
            "the refusal names the cap it enforces: {message}"
        );
    }

    /// The cap is on waiting, not on `sleep`: a short one is how a command
    /// legitimately lets something settle, and it still runs.
    #[tokio::test]
    async fn a_shell_call_may_still_sleep_inside_the_cap() {
        let result = RunShellTool
            .execute(
                json!({"command": "sleep 0.01 && echo settled"}),
                &shell_test_context(),
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert!(result.output.unwrap().contains("settled"));
    }

    /// The model is told the rule in the schema, so the first it hears of the
    /// cap is not a call that failed on it.
    #[test]
    fn the_shell_schema_states_the_sleep_cap() {
        let schema = RunShellTool.parameters_schema();
        let described = schema["properties"]["command"]["description"]
            .as_str()
            .unwrap()
            .to_string();

        assert!(
            described.contains(&MAX_SLEEP_SECS.to_string()),
            "{described}"
        );
        assert!(described.contains("sleep"), "{described}");
    }

    /// A reason is the model's own prose and can carry whatever it just read
    /// out of a file or a page, so the run log records that one arrived and
    /// never what it said.
    #[tokio::test]
    async fn the_shell_tools_log_that_a_reason_arrived_without_repeating_it() {
        const LIFTED: &str = "AWS_SECRET_ACCESS_KEY read out of the .env I just opened";

        let (_, command_log) = captured_logs(RunCommandTool.execute(
            json!({"command": "echo", "args": ["hello"], "reason": LIFTED}),
            &create_test_context(),
        ))
        .await;
        let (_, shell_log) = captured_logs(RunShellTool.execute(
            json!({"command": "echo hello", "reason": LIFTED}),
            &shell_test_context(),
        ))
        .await;

        for (tool, logged) in [("run_command", command_log), ("run_shell", shell_log)] {
            assert!(logged.contains("Running tool"), "{logged}");
            assert!(logged.contains(tool), "{logged}");
            assert!(logged.contains("reason_given=true"), "{logged}");
            assert!(
                !logged.contains(LIFTED),
                "{tool} wrote the model's reason to the log: {logged}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_or_blank_reason_logs_as_none_given() {
        let (_, missing) = captured_logs(RunCommandTool.execute(
            json!({"command": "echo", "args": ["hello"]}),
            &create_test_context(),
        ))
        .await;
        let (_, blank) = captured_logs(RunShellTool.execute(
            json!({"command": "echo hello", "reason": "   "}),
            &shell_test_context(),
        ))
        .await;

        assert!(missing.contains("reason_given=false"), "{missing}");
        assert!(blank.contains("reason_given=false"), "{blank}");
    }

    fn huge_output_context(body: &str) -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("huge.txt"), body).unwrap();
        let mut context = shell_test_context();
        context.cwd = dir.path().to_path_buf();
        (dir, context)
    }

    #[tokio::test]
    async fn run_shell_output_keeps_its_tail_through_to_message() {
        let body = format!(
            "HEAD_MARKER{}TAIL_MARKER",
            "x".repeat(MAX_SHELL_OUTPUT_CHARS * 4)
        );
        let (_dir, context) = huge_output_context(&body);

        let result = RunShellTool
            .execute(json!({"command": "cat huge.txt"}), &context)
            .await
            .unwrap();

        let message = result.to_message();
        assert!(message.contains("HEAD_MARKER"), "{message}");
        assert!(
            message.contains("TAIL_MARKER"),
            "the transcript cut threw away the end of the output: {message}"
        );
        assert!(
            message.chars().count() <= MAX_TOOL_MESSAGE_CHARS,
            "{}",
            message.chars().count()
        );
    }

    #[test]
    fn max_output_chars_clamps_into_range() {
        assert_eq!(clamp_output_chars(None), MAX_SHELL_OUTPUT_CHARS);
        assert_eq!(clamp_output_chars(Some(2_000)), 2_000);
        assert_eq!(
            clamp_output_chars(Some(MAX_SHELL_OUTPUT_CHARS as u64 * 100)),
            MAX_SHELL_OUTPUT_CHARS,
            "the knob must never raise the ceiling"
        );
        assert_eq!(clamp_output_chars(Some(u64::MAX)), MAX_SHELL_OUTPUT_CHARS);
        assert_eq!(clamp_output_chars(Some(0)), MIN_SHELL_OUTPUT_CHARS);
    }

    #[test]
    fn params_read_an_optional_max_output_chars() {
        let command: RunCommandParams =
            serde_json::from_value(json!({"command": "cargo", "max_output_chars": 2_000})).unwrap();
        assert_eq!(command.max_output_chars, Some(2_000));

        let shell: RunShellParams =
            serde_json::from_value(json!({"command": "cargo test", "max_output_chars": 2_000}))
                .unwrap();
        assert_eq!(shell.max_output_chars, Some(2_000));

        let without: RunShellParams =
            serde_json::from_value(json!({"command": "cargo test"})).unwrap();
        assert_eq!(without.max_output_chars, None);
    }

    /// The knob clamps every number it is given, but only after serde has
    /// parsed one into a `u64`. A bare `"type": "integer"` advertises negatives
    /// the parser then refuses, which fails the whole call rather than clamping
    /// it — so the schema has to rule out what the parser cannot take. Zero is
    /// legal and clamps up, which is why the floor is here and not 500.
    #[test]
    fn the_schema_refuses_the_negative_max_output_chars_the_parser_cannot_read() {
        for schema in [
            RunCommandTool.parameters_schema(),
            RunShellTool.parameters_schema(),
        ] {
            assert_eq!(schema["properties"][MAX_OUTPUT_PARAM]["minimum"], json!(0));
        }

        assert!(
            serde_json::from_value::<RunCommandParams>(
                json!({"command": "cargo", "max_output_chars": -1})
            )
            .is_err(),
            "a negative would have to clamp rather than fail, so the schema must exclude it"
        );
        assert!(
            serde_json::from_value::<RunShellParams>(
                json!({"command": "cargo test", "max_output_chars": -1})
            )
            .is_err(),
            "a negative would have to clamp rather than fail, so the schema must exclude it"
        );
        assert_eq!(clamp_output_chars(Some(0)), MIN_SHELL_OUTPUT_CHARS);
    }

    #[test]
    fn shell_schemas_offer_max_output_chars_without_requiring_it() {
        for schema in [
            RunCommandTool.parameters_schema(),
            RunShellTool.parameters_schema(),
        ] {
            assert_eq!(schema["properties"][MAX_OUTPUT_PARAM]["type"], "integer");
            assert!(
                !schema["required"]
                    .as_array()
                    .expect("required array")
                    .iter()
                    .any(|name| name.as_str() == Some(MAX_OUTPUT_PARAM))
            );
        }
    }

    #[tokio::test]
    async fn run_shell_spends_only_the_requested_max_output_chars() {
        const REQUESTED: usize = 2_000;
        let body = format!(
            "HEAD_MARKER{}TAIL_MARKER",
            "x".repeat(MAX_SHELL_OUTPUT_CHARS * 4)
        );
        let (_dir, context) = huge_output_context(&body);

        let message = RunShellTool
            .execute(
                json!({"command": "cat huge.txt", "max_output_chars": REQUESTED}),
                &context,
            )
            .await
            .unwrap()
            .to_message();

        assert!(message.contains("HEAD_MARKER"), "{message}");
        assert!(message.contains("TAIL_MARKER"), "{message}");
        let chars = message.chars().count();
        assert!(chars <= REQUESTED, "{chars}");
        assert!(chars > REQUESTED - 100, "{chars}");
    }

    #[tokio::test]
    async fn run_shell_cannot_raise_the_cap_above_the_constant() {
        let body = format!(
            "HEAD_MARKER{}TAIL_MARKER",
            "x".repeat(MAX_SHELL_OUTPUT_CHARS * 4)
        );
        let (_dir, context) = huge_output_context(&body);

        let message = RunShellTool
            .execute(
                json!({"command": "cat huge.txt", "max_output_chars": 1_000_000}),
                &context,
            )
            .await
            .unwrap()
            .to_message();

        let chars = message.chars().count();
        assert!(chars <= MAX_SHELL_OUTPUT_CHARS, "{chars}");
        assert!(chars > MAX_SHELL_OUTPUT_CHARS - 100, "{chars}");
        assert!(message.contains("TAIL_MARKER"), "{message}");
    }

    #[tokio::test]
    async fn run_command_honours_a_smaller_max_output_chars() {
        const REQUESTED: usize = 2_000;
        let dir = tempfile::tempdir().unwrap();
        let body = format!("HEAD_MARKER{}TAIL_MARKER", "x".repeat(40_000));
        std::fs::write(dir.path().join("huge.txt"), &body).unwrap();

        let mut context = create_test_context();
        context.cwd = dir.path().to_path_buf();

        let message = RunCommandTool
            .execute(
                json!({"command": "cat", "args": ["huge.txt"], "max_output_chars": REQUESTED}),
                &context,
            )
            .await
            .unwrap()
            .to_message();

        assert!(message.contains("HEAD_MARKER"), "{message}");
        assert!(message.contains("TAIL_MARKER"), "{message}");
        assert!(message.chars().count() <= REQUESTED, "{message}");
    }

    /// The failed branch trims the body to the cap and `to_message` then
    /// prepends `Error: `, so what the model reads ran over the cap the caller
    /// asked for. The framing is paid for out of the budget, the same rule the
    /// success path and the transcript cap already follow.
    #[tokio::test]
    async fn a_failed_command_pays_for_the_error_prefix_out_of_the_requested_cap() {
        const REQUESTED: usize = 2_000;
        let dir = tempfile::tempdir().unwrap();
        let body = format!("HEAD_MARKER{}TAIL_MARKER", "x".repeat(40_000));
        std::fs::write(dir.path().join("huge.txt"), &body).unwrap();

        let mut context = create_test_context();
        context.cwd = dir.path().to_path_buf();

        let result = RunCommandTool
            .execute(
                json!({
                    "command": "cat",
                    "args": ["huge.txt", "missing.txt"],
                    "max_output_chars": REQUESTED
                }),
                &context,
            )
            .await
            .unwrap();

        assert!(!result.success, "cat of a missing file exits non-zero");
        let message = result.to_message();
        assert!(
            message.starts_with("Error: Command exited with"),
            "{message}"
        );
        assert!(message.contains("HEAD_MARKER"), "{message}");
        assert!(message.contains("TAIL_MARKER"), "{message}");
        assert!(message.contains("characters trimmed"), "{message}");

        let chars = message.chars().count();
        assert!(
            chars <= REQUESTED,
            "the model was handed {chars} characters against a cap of {REQUESTED}"
        );
        assert!(chars > REQUESTED - 100, "{chars}");
    }

    #[tokio::test]
    async fn run_command_trims_huge_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("HEAD_MARKER{}TAIL_MARKER", "x".repeat(40_000));
        std::fs::write(dir.path().join("huge.txt"), &body).unwrap();

        let mut context = create_test_context();
        context.cwd = dir.path().to_path_buf();

        let result = RunCommandTool
            .execute(
                serde_json::json!({"command": "cat", "args": ["huge.txt"]}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("HEAD_MARKER"), "{output}");
        assert!(output.contains("TAIL_MARKER"), "{output}");
        assert!(output.contains("characters trimmed"), "{output}");
        assert!(output.chars().count() <= MAX_SHELL_OUTPUT_CHARS);
        assert!(output.chars().count() < body.chars().count());
    }

    #[tokio::test]
    async fn run_command_trims_huge_error_payload() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("left.txt"),
            format!("HEAD_LEFT{}TAIL_LEFT", "x".repeat(40_000)),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("right.txt"),
            format!("HEAD_RIGHT{}TAIL_RIGHT", "y".repeat(40_000)),
        )
        .unwrap();

        let mut context = create_test_context();
        context.cwd = dir.path().to_path_buf();

        let result = RunCommandTool
            .execute(
                serde_json::json!({"command": "diff", "args": ["left.txt", "right.txt"]}),
                &context,
            )
            .await
            .unwrap();

        assert!(!result.success);
        let error = result.error.unwrap();
        assert!(error.contains("characters trimmed"), "{error}");
        assert!(error.chars().count() <= MAX_SHELL_OUTPUT_CHARS);
        assert!(
            error.contains("HEAD_LEFT") || error.contains("Command exited"),
            "{error}"
        );
        assert!(
            error.contains("TAIL_RIGHT") || error.contains("TAIL_LEFT"),
            "{error}"
        );
    }
}
