//! A provider backed by a coding agent CLI run as a child process.

use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};
use tool_runner::executor::{GRACE_PERIOD, OutputLimiter, ProcessGroup};

use super::agent::AgentKind;
use super::completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
use super::credential::Credential;
use super::error::{ExitStatus, ProviderError};
use super::event::AgentEvent;
use super::lines::{Lines, Overlong};
use super::settings::{CliSettings, Toolset};
use super::transcript;
use crate::llm::{Message, Usage};

const READ_BUFFER: usize = 8 * 1024;

/// A coding agent's events, yielded as the child emits them.
pub type AgentStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, ProviderError>> + Send>>;

/// Drives a coding agent CLI as a completion provider.
///
/// The agent runs its own tool loop, so [`CompletionRequest::tools`] is not
/// forwarded: schemas on a completions request are not something an agent
/// reads. Zone's tools reach it as [`CliSettings::toolset`] instead, over MCP,
/// and the calls come back to zone to be approved and run. What this returns
/// is the agent's final prose. Tool activity is still parsed, so a stream full
/// of it cannot derail the run, but it is not returned as tool calls -- the
/// agent already made them, and replaying them through zone's registry would
/// run each one a second time.
///
/// [`CliProvider::stream`] is the primary form. A turn takes minutes, and the
/// child's output is framed a line at a time, so the answer exists long before
/// the process exits; [`CompletionProvider::complete`] is that stream, read to
/// the end.
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

    /// The command this provider runs, as an operator would have to type it.
    fn executable(&self) -> String {
        self.settings.executable.as_ref().map_or_else(
            || self.agent.executable().to_string(),
            |path| path.display().to_string(),
        )
    }

    fn command(&self, model: &str) -> Command {
        let executable = self
            .settings
            .executable
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(self.agent.executable()));

        let mut command = Command::new(executable);
        command
            .args(self.agent.arguments_with(
                Some(model),
                self.settings.toolset.as_deref(),
                self.settings.builtin_tools,
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(directory) = &self.settings.working_directory {
            command.current_dir(directory);
        }

        match &self.settings.toolset {
            Some(toolset) => {
                command.env(Toolset::TOKEN_VARIABLE, toolset.token.expose());
            }
            // A turn serving no tools hands out no token, and a token left in
            // zone's own environment is not this turn's to pass on.
            None => {
                command.env_remove(Toolset::TOKEN_VARIABLE);
            }
        }

        match &self.settings.credential {
            Credential::Key { variable, value } => {
                command.env(variable, value.expose());
            }
            // An operator who points zone at the CLI signed in on this host
            // means to spend that subscription. A key left in the server's own
            // environment outranks the session silently, and bills the key.
            Credential::Inherited => {
                for agent in AgentKind::ALL {
                    command.env_remove(agent.variable());
                }
            }
        }

        // A coding agent forks a tree of its own -- language servers, search,
        // git. Giving the child its own group is what makes a timeout able to
        // reap all of it instead of orphaning the grandchildren.
        command.process_group(0);

        command
    }

    /// The agent's events as it produces them.
    ///
    /// The child is spawned before this returns, so an agent that is not
    /// installed is reported as an error rather than as a stream that fails on
    /// its first item.
    pub fn stream(&self, request: CompletionRequest<'_>) -> Result<AgentStream, ProviderError> {
        let prompt = transcript::render(request.messages);
        let mut child = self
            .command(request.model)
            .spawn()
            .map_err(|error| ProviderError::unavailable(&self.name, &self.executable(), error))?;

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
        let output_limit = self.settings.output_limit;
        let diagnostics = tokio::spawn(async move { read_diagnostics(stderr, output_limit).await });

        let mut session = Session {
            name: self.name.clone(),
            child,
            group,
            stdout,
            writer,
            diagnostics: Some(diagnostics),
            deadline: Instant::now() + self.settings.timeout,
            timeout: self.settings.timeout,
            reaped: false,
        };

        let name = self.name.clone();
        let agent = self.agent;
        let line_limit = self.settings.line_limit;

        Ok(Box::pin(async_stream::try_stream! {
            let mut lines = Lines::new(line_limit);
            let mut limiter = OutputLimiter::new(output_limit);
            let mut buffer = [0_u8; READ_BUFFER];
            let mut events = Vec::new();
            let mut ended = false;
            let mut terminal = false;

            while !ended {
                let read = session.read(&mut buffer).await?;
                if read == 0 {
                    ended = true;
                    match lines.flush() {
                        Ok(Some(line)) => agent.interpret(&line, &mut events),
                        Ok(None) => {}
                        Err(overlong) => Err(session.stop(unframed(&name, overlong)).await)?,
                    }
                } else {
                    let (accepted, count, _) = limiter.check(read);
                    if !accepted || count < read {
                        Err(session.stop(ProviderError::agent(
                            &name,
                            &format!("the agent produced more than {output_limit} bytes of output"),
                        )).await)?;
                    }
                    lines.extend(&buffer[..count]);
                    loop {
                        match lines.take() {
                            Ok(Some(line)) => agent.interpret(&line, &mut events),
                            Ok(None) => break,
                            Err(overlong) => Err(session.stop(unframed(&name, overlong)).await)?,
                        }
                    }
                }

                for event in events.drain(..) {
                    // The agent's own report of what went wrong beats an exit
                    // code, which says only that something did.
                    if let AgentEvent::Failed(message) = &event {
                        Err(session.stop(ProviderError::agent(&name, message)).await)?;
                    }
                    terminal |= event.terminal();
                    yield event;
                }
            }

            session.finish(terminal).await?;
        }))
    }

    async fn run(&self, request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        let mut events = self.stream(request)?;
        let mut text = String::new();
        let mut usage: Option<Usage> = None;
        let mut finish_reason: Option<String> = None;

        while let Some(event) = events.next().await {
            match event? {
                AgentEvent::Text(chunk) => text.push_str(&chunk),
                AgentEvent::Usage(counts) => usage = Some(counts),
                AgentEvent::Tool(call) => {
                    tracing::debug!(provider = %self.name, tool = %call.function.name, "agent tool call");
                }
                AgentEvent::Failed(message) => {
                    return Err(ProviderError::agent(&self.name, &message));
                }
                AgentEvent::Finished {
                    finish_reason: reason,
                } => finish_reason = reason,
            }
        }

        Ok(Completion {
            provider: self.name.clone(),
            message: Message::assistant(text),
            usage,
            finish_reason,
        })
    }
}

/// One running agent, and everything that has to be cleaned up after it.
struct Session {
    name: String,
    child: Child,
    group: Option<ProcessGroup>,
    stdout: Option<ChildStdout>,
    writer: JoinHandle<()>,
    diagnostics: Option<JoinHandle<String>>,
    deadline: Instant,
    timeout: Duration,
    reaped: bool,
}

impl Session {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ProviderError> {
        let deadline = self.deadline;
        let read = {
            let Some(stdout) = self.stdout.as_mut() else {
                return Ok(0);
            };
            timeout_at(deadline, stdout.read(buffer)).await
        };

        match read {
            Ok(Ok(read)) => Ok(read),
            Ok(Err(error)) => Err(ProviderError::malformed(
                &self.name,
                format!("could not read the agent's output: {error}"),
            )),
            Err(_) => {
                let expired = self.expired();
                Err(self.stop(expired).await)
            }
        }
    }

    /// Wait for the agent to exit and judge the run by how it ended.
    async fn finish(&mut self, terminal: bool) -> Result<(), ProviderError> {
        let status = match timeout_at(self.deadline, self.child.wait()).await {
            Ok(status) => status.map_err(|error| ProviderError::malformed(&self.name, error))?,
            Err(_) => {
                let expired = self.expired();
                return Err(self.stop(expired).await);
            }
        };
        self.reaped = true;
        self.writer.abort();

        let status = ExitStatus::from(status);
        if status != ExitStatus::Code(0) {
            let diagnostics = self.diagnostics().await;
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
        if !terminal {
            return Err(ProviderError::agent(
                &self.name,
                "the agent exited without completing its event stream",
            ));
        }

        Ok(())
    }

    /// Stop the agent, then report why.
    ///
    /// Every abnormal end goes through here: a child left running writes into
    /// a pipe nobody is reading and blocks there until it is killed anyway.
    async fn stop(&mut self, error: ProviderError) -> ProviderError {
        self.writer.abort();
        if let Some(group) = &self.group {
            let _ = group.terminate();
        }
        if timeout(GRACE_PERIOD, self.child.wait()).await.is_err() {
            if let Some(group) = &self.group {
                let _ = group.kill();
            }
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }
        self.reaped = true;

        error
    }

    /// Whatever the agent wrote to stderr, given a bounded wait.
    ///
    /// A grandchild that inherited the pipe can hold it open after the agent
    /// itself is gone, and diagnostics are never worth hanging a turn for.
    async fn diagnostics(&mut self) -> String {
        let Some(handle) = self.diagnostics.take() else {
            return String::new();
        };
        timeout(GRACE_PERIOD, handle)
            .await
            .map(Result::unwrap_or_default)
            .unwrap_or_default()
    }

    fn expired(&self) -> ProviderError {
        ProviderError::Timeout {
            provider: self.name.clone(),
            seconds: self.timeout.as_secs(),
        }
    }
}

/// A consumer that stops reading -- a chat turn the user cancelled -- leaves
/// the agent and every process it forked running. Dropping the session is the
/// only notice that arrives on that path.
impl Drop for Session {
    fn drop(&mut self) {
        self.writer.abort();
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.abort();
        }
        if self.reaped {
            return;
        }
        if let Some(group) = &self.group {
            let _ = group.kill();
        }
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

fn unframed(provider: &str, overlong: Overlong) -> ProviderError {
    ProviderError::malformed(
        provider,
        format!("one event exceeded {} bytes", overlong.limit),
    )
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

    fn request(messages: &[Message]) -> CompletionRequest<'_> {
        CompletionRequest {
            model: "sonnet",
            messages,
            tools: None,
            options: RequestOptions { reserved: 512 },
        }
    }

    async fn run(
        provider: &CliProvider,
        messages: &[Message],
    ) -> Result<Completion, ProviderError> {
        provider.complete(request(messages)).await
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

    /// Recorded from claude 2.1.269 with no session on the host. The subtype
    /// says success and the run still failed, which is why the flag and not
    /// the subtype decides.
    const SIGNED_OUT: &str = r#"{"type":"result","subtype":"success","is_error":true,"terminal_reason":"api_error","result":"Not logged in · Please run /login"}"#;

    #[tokio::test]
    async fn text_reaches_the_consumer_before_the_agent_exits() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Looking now."}]}}'
sleep 30
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));
        let messages = [Message::user("What does a.rs do?")];

        let mut events = provider
            .stream(request(&messages))
            .expect("a running agent");
        let first = tokio::time::timeout(Duration::from_secs(5), events.next())
            .await
            .expect("the text to arrive while the agent is still running")
            .expect("an event")
            .expect("a readable event");

        assert!(
            matches!(&first, AgentEvent::Text(text) if text == "Looking now."),
            "expected the agent's text, got {first:?}"
        );
    }

    #[tokio::test]
    async fn a_signed_out_agent_ends_the_stream_as_a_failure() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = format!("printf '%s\\n' '{SIGNED_OUT}'");
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, &script));
        let messages = [Message::user("hi")];

        let mut events = provider
            .stream(request(&messages))
            .expect("a running agent");
        let mut delivered = Vec::new();
        let mut failure = None;
        while let Some(event) = events.next().await {
            match event {
                Ok(event) => delivered.push(event),
                Err(error) => failure = Some(error),
            }
        }

        let failure = failure.expect("a signed-out agent to fail the run");
        assert!(
            failure
                .to_string()
                .contains("Not logged in · Please run /login"),
            "lost the agent's wording: {failure}"
        );
        assert!(
            delivered.is_empty(),
            "a refusal was delivered as an answer: {delivered:?}"
        );
    }

    #[tokio::test]
    async fn output_past_the_cap_stops_the_run_instead_of_waiting_for_the_timeout() {
        let directory = TempDir::new().expect("a temporary directory");
        let settings = settings(&directory, "yes 'xxxxxxxxxxxxxxxx'").with_output_limit(4 * 1024);
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let started = std::time::Instant::now();
        let error = run(&provider, &[Message::user("hi")])
            .await
            .expect_err("a failure");

        assert!(
            error.to_string().contains("4096"),
            "the cap the run broke is not named: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the run blocked on a full pipe until the timeout instead of stopping the agent"
        );
    }

    #[test]
    fn an_inherited_session_clears_the_keys_that_would_outrank_it() {
        let provider = CliProvider::agent(AgentKind::Claude, CliSettings::default());

        let command = provider.command("sonnet");
        let cleared: Vec<String> = command
            .as_std()
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();

        for variable in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"] {
            assert!(
                cleared.contains(&variable.to_string()),
                "{variable} in the server's environment would be spent instead of the host session: {cleared:?}"
            );
        }
    }

    fn toolset() -> Toolset {
        Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file"],
        )
    }

    fn arguments(command: &Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_turns_token_reaches_the_agents_environment_and_never_its_arguments() {
        let provider = CliProvider::agent(
            AgentKind::Claude,
            CliSettings::default().with_toolset(toolset()),
        );

        let command = provider.command("sonnet");
        let token = command
            .as_std()
            .get_envs()
            .find(|(name, _)| name.to_string_lossy() == Toolset::TOKEN_VARIABLE)
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned());

        assert_eq!(token.as_deref(), Some("zone-turn-notarealtoken"));

        let arguments = arguments(&command);
        assert!(arguments.iter().any(|argument| argument == "--mcp-config"));
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "mcp__zone__read_file")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.contains("zone-turn-notarealtoken")),
            "the token reached argv: {arguments:?}"
        );
    }

    #[test]
    fn a_turn_serving_no_tools_withholds_the_agents_own_and_clears_the_token() {
        let provider = CliProvider::agent(AgentKind::Claude, CliSettings::default());

        let command = provider.command("sonnet");
        let cleared: Vec<String> = command
            .as_std()
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();

        assert!(
            cleared.contains(&Toolset::TOKEN_VARIABLE.to_string()),
            "a token from the server's own environment would have been passed on: {cleared:?}"
        );

        let arguments = arguments(&command);
        assert!(
            arguments.iter().any(|argument| argument == "--tools"),
            "the agent kept its own file and shell tools: {arguments:?}"
        );
    }

    #[test]
    fn a_configured_key_is_still_handed_to_the_agent() {
        let settings = CliSettings::default()
            .with_credential(Credential::key("ANTHROPIC_API_KEY", "sk-ant-present"));
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let command = provider.command("sonnet");
        let value = command
            .as_std()
            .get_envs()
            .find(|(name, _)| name.to_string_lossy() == "ANTHROPIC_API_KEY")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned());

        assert_eq!(value.as_deref(), Some("sk-ant-present"));
    }
}
