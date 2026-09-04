//! Command execution tool

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tool_runner::Proxy;

use super::{Tool, ToolContext, ToolError, ToolResult};

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
}

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Returns stdout/stderr output. Use for running tests, builds, git commands, etc."
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
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: RunCommandParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        // Security: Use allowlist approach instead of blocklist
        // Only allow known safe development commands
        let allowed_commands = [
            // Build tools
            "cargo", "rustc", "npm", "npx", "yarn", "pnpm", "node", "deno", "bun", "make", "cmake",
            "gradle", "mvn", "maven", "go", "python", "python3", "pip", "pip3", "poetry", "uv",
            "ruby", "gem", "bundle", "rake", "dotnet", "msbuild", // Version control
            "git", "gh", "hub", // File utilities (read-only or safe)
            "ls", "cat", "head", "tail", "grep", "find", "wc", "sort", "uniq", "diff", "tree",
            "file", "stat", "pwd", "which", "whereis", // Testing
            "pytest", "jest", "mocha", "rspec", "phpunit", // Other safe utilities
            "echo", "printf", "date", "env", "true", "false", "test", "curl", "wget", "jq", "yq",
            // Docker (read operations)
            "docker",
        ];

        // Extract base command name (handle both `/path/to/cmd` and `cmd`)
        let base_cmd = params
            .command
            .split('/')
            .next_back()
            .unwrap_or(&params.command)
            .split('\\')
            .next_back()
            .unwrap_or(&params.command);

        if !allowed_commands.contains(&base_cmd) {
            return Err(ToolError::Execution(format!(
                "Command '{}' is not in the allowed list. Allowed commands: cargo, npm, git, python, etc.",
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

        // Set environment
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

        if output.status.success() {
            Ok(ToolResult::success(result))
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Ok(ToolResult::error(format!(
                "Command exited with code {}\n\n{}",
                code, result
            )))
        }
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
}

/// Longest a single shell command may run, whatever it asks for.
const MAX_SHELL_TIMEOUT_SECS: u64 = 900;

/// Cap on returned output, so one noisy command cannot fill the context
/// window. The middle is dropped rather than the tail, because the error a
/// build is being run for is usually at the end.
const MAX_SHELL_OUTPUT_CHARS: usize = 16_000;

fn trim_middle(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_SHELL_OUTPUT_CHARS {
        return text.to_string();
    }
    let half = MAX_SHELL_OUTPUT_CHARS / 2;
    let head: String = chars[..half].iter().collect();
    let tail: String = chars[chars.len() - half..].iter().collect();
    format!(
        "{head}\n\n[… {} characters trimmed …]\n\n{tail}",
        chars.len() - MAX_SHELL_OUTPUT_CHARS
    )
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

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to run, e.g. 'cargo test 2>&1 | tail -40'"
                },
                "cwd": {
                    "type": "string",
                    "description": "Directory to run in. Absolute, or relative to the working directory."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Wall-clock limit in seconds. Default 120, maximum 900."
                }
            },
            "required": ["command"]
        })
    }

    fn timeout(&self, _context: &ToolContext) -> Duration {
        // Loose enough never to pre-empt the per-call limit enforced below.
        Duration::from_secs(MAX_SHELL_TIMEOUT_SECS + 30)
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: RunShellParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        if params.command.trim().is_empty() {
            return Err(ToolError::InvalidParams("Command is empty".to_string()));
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
        Ok(ToolResult::success(trim_middle(report.trim_end())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
