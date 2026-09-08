//! A provider backed by a coding agent CLI run as a child process.

use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, ChildStdout, Command};
use tokio::time::timeout;
use tool_runner::executor::{GRACE_PERIOD, OutputLimiter, ProcessGroup};

use super::agent::AgentKind;
use super::completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
use super::credential::Credential;
use super::error::{ExitStatus, ProviderError};
use super::event::AgentEvent;
use super::lines::Lines;
use super::settings::CliSettings;
use super::transcript;
use crate::llm::{Message, Usage};

const READ_BUFFER: usize = 8 * 1024;

/// Drives a coding agent CLI as a completion provider.
///
/// The agent runs its own tool loop, so [`CompletionRequest::tools`] is not
/// forwarded: the agent has its own tools and no way to call zone's. What
/// comes back is the agent's final prose. Tool activity is still parsed, so a
/// stream full of it cannot derail the run, but it is not returned as tool
/// calls -- the agent already executed them, and replaying them through zone's
/// registry would run each one a second time.
#[derive(Debug)]
pub struct CliProvider {
    name: String,
    agent: AgentKind,
    settings: CliSettings,
}

impl CliProvider {
    pub fn new(name: impl Into<String>, agent: AgentKind, settings: CliSettings) -> Self {
        Self {
            name: name.into(),
            agent,
            settings,
        }
    }

    /// A provider named after the agent it drives.
    pub fn agent(agent: AgentKind, settings: CliSettings) -> Self {
        Self::new(agent.as_str(), agent, settings)
    }

    pub fn settings(&self) -> &CliSettings {
        &self.settings
    }

    fn command(&self, model: &str) -> Command {
        let executable = self
            .settings
            .executable
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(self.agent.executable()));

        let mut command = Command::new(executable);
        command
            .args(self.agent.arguments(Some(model)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(directory) = &self.settings.working_directory {
            command.current_dir(directory);
        }

        if let Credential::Key { variable, value } = &self.settings.credential {
            command.env(variable, value.expose());
        }

        // A coding agent forks a tree of its own -- language servers, search,
        // git. Giving the child its own group is what makes a timeout able to
        // reap all of it instead of orphaning the grandchildren.
        command.process_group(0);

        command
    }

    async fn run(&self, request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        let prompt = transcript::render(request.messages);
        let mut child = self.command(request.model).spawn().map_err(|error| {
            ProviderError::unavailable(
                &self.name,
                &self.settings.executable.as_ref().map_or_else(
                    || self.agent.executable().to_string(),
                    |path| path.display().to_string(),
                ),
                error,
            )
        })?;

        let group = child.id().map(ProcessGroup::new);

        // Writing the prompt has to overlap reading the reply. A prompt larger
        // than the pipe buffer blocks until the child drains it, and a child
        // that is not being read from blocks on its own output first.
        let stdin = child.stdin.take();
        let writer = tokio::spawn(async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }
        });

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let agent = self.agent;
        let line_limit = self.settings.line_limit;
        let output_limit = self.settings.output_limit;

        let reader =
            tokio::spawn(async move { read_events(stdout, agent, line_limit, output_limit).await });
        let diagnostics = tokio::spawn(async move { read_diagnostics(stderr, output_limit).await });

        let status = match timeout(self.settings.timeout, child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                if let Some(group) = &group {
                    let _ = group.graceful_kill(GRACE_PERIOD).await;
                }
                let _ = child.kill().await;
                writer.abort();
                reader.abort();
                diagnostics.abort();
                return Err(ProviderError::Timeout {
                    provider: self.name.clone(),
                    seconds: self.settings.timeout.as_secs(),
                });
            }
        };

        let _ = writer.await;
        let events = reader
            .await
            .map_err(|error| ProviderError::malformed(&self.name, error))?;
        let diagnostics = diagnostics.await.unwrap_or_default();

        let status = status.map_err(|error| ProviderError::malformed(&self.name, error))?;
        let events = events.map_err(|message| ProviderError::malformed(&self.name, message))?;

        self.assemble(events, diagnostics, status.into())
    }

    fn assemble(
        &self,
        events: Vec<AgentEvent>,
        diagnostics: String,
        status: ExitStatus,
    ) -> Result<Completion, ProviderError> {
        let mut text = String::new();
        let mut usage: Option<Usage> = None;
        let mut failure: Option<String> = None;
        let mut finish_reason: Option<String> = None;
        let mut finished = false;

        for event in events {
            match event {
                AgentEvent::Text(chunk) => text.push_str(&chunk),
                AgentEvent::Usage(counts) => usage = Some(counts),
                AgentEvent::Tool(call) => {
                    tracing::debug!(provider = %self.name, tool = %call.function.name, "agent tool call");
                }
                AgentEvent::Failed(message) => {
                    finished = true;
                    failure.get_or_insert(message);
                }
                AgentEvent::Finished {
                    finish_reason: reason,
                } => {
                    finished = true;
                    finish_reason = reason;
                }
            }
        }

        // The agent's own report of what went wrong beats an exit code, which
        // says only that something did.
        if let Some(message) = failure {
            return Err(ProviderError::agent(&self.name, &message));
        }

        if status != ExitStatus::Code(0) {
            let message = if diagnostics.trim().is_empty() {
                "the agent produced no diagnostics".to_string()
            } else {
                diagnostics
            };
            return Err(ProviderError::exit(&self.name, status, &message));
        }

        // A clean exit is not proof of a finished turn. An agent killed
        // between its last token and its result line still exits zero, and
        // returning that truncated text as the answer hides the truncation.
        if !finished {
            return Err(ProviderError::agent(
                &self.name,
                "the agent exited without completing its event stream",
            ));
        }

        Ok(Completion {
            provider: self.name.clone(),
            message: Message::assistant(text),
            usage,
            finish_reason,
        })
    }
}

#[async_trait]
impl CompletionProvider for CliProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Cli
    }

    async fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        self.run(request).await
    }
}

async fn read_events(
    stdout: Option<ChildStdout>,
    agent: AgentKind,
    line_limit: usize,
    output_limit: usize,
) -> Result<Vec<AgentEvent>, String> {
    let Some(mut stdout) = stdout else {
        return Ok(Vec::new());
    };

    let mut events = Vec::new();
    let mut lines = Lines::new(line_limit);
    let mut limiter = OutputLimiter::new(output_limit);
    let mut buffer = [0_u8; READ_BUFFER];

    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .map_err(|error| format!("could not read the agent's output: {error}"))?;
        if read == 0 {
            break;
        }
        let (accepted, count, _) = limiter.check(read);
        if !accepted {
            break;
        }
        lines.extend(&buffer[..count]);
        while let Some(line) = lines
            .take()
            .map_err(|overlong| format!("one event exceeded {} bytes", overlong.limit))?
        {
            agent.interpret(&line, &mut events);
        }
        if count < read {
            break;
        }
    }

    if let Some(line) = lines
        .flush()
        .map_err(|overlong| format!("one event exceeded {} bytes", overlong.limit))?
    {
        agent.interpret(&line, &mut events);
    }

    Ok(events)
}

/// Stderr is drained whether or not it is ever read back. An agent run with a
/// piped-but-unread stderr blocks the moment it fills the pipe buffer, which
/// on a verbose agent happens long before it reaches its answer.
async fn read_diagnostics(stderr: Option<ChildStderr>, limit: usize) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };

    let mut collected = Vec::new();
    let mut limiter = OutputLimiter::new(limit);
    let mut buffer = [0_u8; READ_BUFFER];

    while let Ok(read) = stderr.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let (accepted, count, _) = limiter.check(read);
        if !accepted {
            break;
        }
        collected.extend_from_slice(&buffer[..count]);
    }

    String::from_utf8_lossy(&collected).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::RequestOptions;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    /// A stand-in agent, so no test needs a real CLI installed.
    ///
    /// It reads its prompt from stdin exactly as the real agents do, which is
    /// what keeps the delivery path under test the real one.
    fn fake(directory: &TempDir, script: &str) -> PathBuf {
        let path = directory.path().join("agent");
        let mut file = std::fs::File::create(&path).expect("the fake agent");
        write!(file, "#!/bin/sh\n{script}\n").expect("the fake agent body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the fake agent to be executable");
        wait_until_executable(&path);
        path
    }

    /// Linux refuses to exec a file any process still holds open for writing.
    /// The descriptor here is closed, but a sibling test forking between its
    /// own open and exec inherits it for that window, so a freshly written
    /// script can hit ETXTBSY under a parallel run. Production never meets this:
    /// a provider execs an installed binary, not one it just wrote.
    fn wait_until_executable(path: &std::path::Path) {
        for _ in 0..50 {
            match std::process::Command::new(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Err(error) if error.raw_os_error() == Some(26) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    }

    fn settings(directory: &TempDir, script: &str) -> CliSettings {
        CliSettings::default()
            .with_executable(fake(directory, script))
            .with_timeout(Duration::from_secs(20))
    }

    async fn run(
        provider: &CliProvider,
        messages: &[Message],
    ) -> Result<Completion, ProviderError> {
        provider
            .complete(CompletionRequest {
                model: "sonnet",
                messages,
                tools: None,
                options: RequestOptions { reserved: 512 },
            })
            .await
    }

    const CLAUDE_SESSION: &str = r#"
echo '{"type":"system","subtype":"init","session_id":"6f1"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Looking now. "}],"usage":{"input_tokens":4,"cache_read_input_tokens":800,"output_tokens":6}}}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/w/a.rs"}}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"It is empty."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":9,"cache_read_input_tokens":1600,"output_tokens":24}}'
"#;

    #[tokio::test]
    async fn a_streamed_session_becomes_one_assistant_message() {
        let directory = TempDir::new().expect("a temporary directory");
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, CLAUDE_SESSION));

        let completion = run(&provider, &[Message::user("What does a.rs do?")])
            .await
            .expect("an answer");

        assert_eq!(completion.provider, "claude");
        assert_eq!(
            completion.message.content.as_deref(),
            Some("Looking now. It is empty.")
        );
        assert_eq!(completion.finish_reason.as_deref(), Some("success"));

        let usage = completion.usage.expect("token counts");
        assert_eq!(usage.prompt_tokens, 9 + 1600);
        assert_eq!(usage.completion_tokens, 24);
    }

    #[tokio::test]
    async fn the_prompt_reaches_the_agent_on_stdin() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
prompt=$(cat | tr '\n' ' ')
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "$prompt"
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));

        let completion = run(&provider, &[Message::user("ping")])
            .await
            .expect("an answer");

        let content = completion.message.content.unwrap_or_default();
        assert!(content.contains("User:"), "roles were lost: {content}");
        assert!(content.contains("ping"), "the prompt was lost: {content}");
    }

    #[tokio::test]
    async fn a_prompt_far_larger_than_a_pipe_buffer_still_gets_through() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
bytes=$(cat | wc -c | tr -d ' ')
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "$bytes"
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));
        let large = "x".repeat(512 * 1024);
        let messages = [Message::user(large.clone())];

        let completion = run(&provider, &messages).await.expect("an answer");

        let reported: usize = completion
            .message
            .content
            .as_deref()
            .expect("a byte count")
            .trim()
            .parse()
            .expect("a number");
        assert!(
            reported >= large.len(),
            "the agent received only {reported} of {} bytes",
            large.len()
        );
    }

    #[tokio::test]
    async fn an_agent_that_reports_a_failure_fails_the_request() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"echo '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Invalid API key provided"}'"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        assert!(
            error.to_string().contains("Invalid API key"),
            "lost the agent's wording: {error}"
        );
        assert_eq!(error.provider(), Some("claude"));
    }

    #[tokio::test]
    async fn a_nonzero_exit_reports_the_agents_diagnostics() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = "echo 'error: could not reach the model endpoint' >&2\nexit 3";
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        let ProviderError::Exit {
            status, message, ..
        } = &error
        else {
            panic!("expected an exit failure, got {error:?}");
        };
        assert_eq!(*status, ExitStatus::Code(3));
        assert!(message.contains("could not reach the model endpoint"));
    }

    #[tokio::test]
    async fn a_clean_exit_without_a_result_is_not_treated_as_an_answer() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"echo '{"type":"assistant","message":{"content":[{"type":"text","text":"half an ans"}]}}'"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        assert!(
            error.to_string().contains("without completing"),
            "truncation was hidden: {error}"
        );
    }

    #[tokio::test]
    async fn an_agent_that_never_finishes_is_stopped_at_the_timeout() {
        let directory = TempDir::new().expect("a temporary directory");
        let settings = CliSettings::default()
            .with_executable(fake(&directory, "sleep 120"))
            .with_timeout(Duration::from_millis(300));
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        assert!(
            matches!(error, ProviderError::Timeout { .. }),
            "expected a timeout, got {error:?}"
        );
    }

    #[tokio::test]
    async fn a_missing_agent_names_the_command_that_was_missing() {
        let provider = CliProvider::agent(
            AgentKind::Codex,
            CliSettings::default().with_executable("/nonexistent/zone/codex"),
        );

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        let ProviderError::Unavailable { executable, .. } = &error else {
            panic!("expected an unavailable agent, got {error:?}");
        };
        assert_eq!(executable, "/nonexistent/zone/codex");
    }

    #[tokio::test]
    async fn a_credential_the_agent_echoes_back_never_reaches_the_error() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = "echo \"fatal: rejected credential $ANTHROPIC_API_KEY\" >&2\nexit 1";
        let settings = settings(&directory, script).with_credential(Credential::key(
            "ANTHROPIC_API_KEY",
            "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ));
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        let rendered = format!("{error} {error:?} {provider:?}");
        assert!(
            !rendered.contains("sk-ant-api03-AAAA"),
            "credential leaked: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"), "not redacted: {rendered}");
    }

    #[tokio::test]
    async fn the_credential_reaches_the_agent_that_needs_it() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "${ANTHROPIC_API_KEY:-absent}"
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let settings = settings(&directory, script)
            .with_credential(Credential::key("ANTHROPIC_API_KEY", "sk-ant-present"));
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let completion = run(&provider, &[Message::user("hi")])
            .await
            .expect("an answer");

        assert_eq!(
            completion.message.content.as_deref(),
            Some("sk-ant-present")
        );
    }

    #[tokio::test]
    async fn a_codex_session_is_driven_by_the_same_provider() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
echo '{"type":"thread.started","thread_id":"t1"}'
echo '{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"The suite passes."}}'
echo '{"type":"turn.completed","usage":{"input_tokens":40,"output_tokens":8}}'
"#;
        let provider = CliProvider::agent(AgentKind::Codex, settings(&directory, script));

        let completion = run(&provider, &[Message::user("run the tests")])
            .await
            .expect("an answer");

        assert_eq!(completion.provider, "codex");
        assert_eq!(
            completion.message.content.as_deref(),
            Some("The suite passes.")
        );
        assert_eq!(completion.usage.expect("token counts").total_tokens, 48);
    }

    #[tokio::test]
    async fn a_single_oversized_event_is_reported_as_malformed_output() {
        let directory = TempDir::new().expect("a temporary directory");
        let mut settings = settings(&directory, "head -c 5000 /dev/zero | tr '\\0' 'x'; echo");
        settings.line_limit = 256;
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        assert!(
            matches!(error, ProviderError::Malformed { .. }),
            "expected malformed output, got {error:?}"
        );
    }
}
