//! A provider backed by a coding agent CLI run as a child process.

use std::collections::VecDeque;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};
use tool_runner::executor::{GRACE_PERIOD, ProcessGroup};

use super::agent::AgentKind;
use super::completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
use super::credential::Credential;
use super::environment;
use super::error::{ExitStatus, ProviderError};
use super::event::AgentEvent;
use super::lines::{Frame, Lines};
use super::settings::{CliSettings, Toolset};
use super::transcript;
use crate::llm::{Message, Usage};

const READ_BUFFER: usize = 8 * 1024;

/// Bytes of the agent's stderr that a failure it reported carries.
const DIAGNOSTIC_TAIL: usize = 1024;

/// Stands between an agent's own report of a failure and the end of its
/// stderr that follows it, so a reader can tell the two apart.
pub const STDERR_HEADING: &str = "\n\nThe end of the agent's stderr:\n";

/// How a turn stopped for an answer past its output limit begins its failure,
/// before it names the limit.
pub const OUTGROWN: &str = "the agent's answer grew past";

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
                self.settings.sandbox,
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(directory) = &self.settings.working_directory {
            command.current_dir(directory);
        }

        command
            .env_clear()
            .envs(environment::inherited())
            .envs(&self.settings.variables);
        if let Some(toolset) = &self.settings.toolset {
            command.env(Toolset::TOKEN_VARIABLE, toolset.token.expose());
        }
        // Last, so that no variable of the same name can replace it.
        if let Credential::Key { variable, value } = &self.settings.credential {
            command.env(variable, value.expose());
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
            let mut buffer = [0_u8; READ_BUFFER];
            let mut events = Vec::new();
            let mut kept = 0_usize;
            let mut ended = false;
            let mut terminal = false;

            while !ended {
                let read = session.read(&mut buffer).await?;
                if read == 0 {
                    ended = true;
                    if let Some(frame) = lines.flush() {
                        interpret(agent, &name, line_limit, frame, &mut events);
                    }
                } else {
                    lines.extend(&buffer[..read]);
                    while let Some(frame) = lines.take() {
                        interpret(agent, &name, line_limit, frame, &mut events);
                    }
                }

                for event in events.drain(..) {
                    // The agent's own report of what went wrong beats an exit
                    // code, which says only that something did.
                    if let AgentEvent::Failed(message) = &event {
                        Err(session.fail(message).await)?;
                    }
                    kept = kept.saturating_add(retained(&event));
                    if kept > output_limit {
                        Err(session.stop(ProviderError::agent(
                            &name,
                            &format!("{OUTGROWN} {output_limit} bytes"),
                        )).await)?;
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
    async fn stop(&mut self, error: ProviderError) -> ProviderError {
        self.halt().await;
        error
    }

    /// Stop the agent, then report the failure it named, followed by the end
    /// of its stderr: codex reports a sign-in it could not renew only there.
    async fn fail(&mut self, message: &str) -> ProviderError {
        self.halt().await;
        let diagnostics = self.diagnostics().await;
        ProviderError::agent(&self.name, &failure(message, &diagnostics))
    }

    /// Every abnormal end goes through here: a child left running writes into
    /// a pipe nobody is reading and blocks there until it is killed anyway.
    async fn halt(&mut self) {
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

fn interpret(
    agent: AgentKind,
    provider: &str,
    limit: usize,
    frame: Frame,
    events: &mut Vec<AgentEvent>,
) {
    match frame {
        Frame::Line(line) => agent.interpret(&line, events),
        Frame::Dropped => tracing::warn!(
            provider,
            limit,
            "dropped an agent event longer than the line limit; the turn goes on without it"
        ),
    }
}

/// The bytes of `event` that whoever reads the stream keeps. A tool call is
/// kept as its name on a line of its own; what it was made with reaches no
/// reader.
fn retained(event: &AgentEvent) -> usize {
    match event {
        AgentEvent::Text(text) | AgentEvent::Failed(text) => text.len(),
        AgentEvent::Tool(call) => call.function.name.len() + '\n'.len_utf8(),
        AgentEvent::Usage(_) | AgentEvent::Finished { .. } => 0,
    }
}

/// `message`, then [`STDERR_HEADING`] and the last whole lines of
/// `diagnostics` that fit in [`DIAGNOSTIC_TAIL`] bytes.
fn failure(message: &str, diagnostics: &str) -> String {
    let diagnostics = diagnostics.trim();
    if diagnostics.is_empty() {
        return message.to_string();
    }
    let mut start = diagnostics.len().saturating_sub(DIAGNOSTIC_TAIL);
    while !diagnostics.is_char_boundary(start) {
        start += 1;
    }
    let tail = match diagnostics[start..].split_once('\n') {
        Some((_, whole)) if start > 0 => whole,
        _ => &diagnostics[start..],
    };
    format!("{message}{STDERR_HEADING}{tail}")
}

/// The last `limit` bytes of the agent's stderr, which is read to its end
/// whether or not it is ever read back: an agent writing into a pipe nobody
/// reads blocks once the pipe is full, and one whose pipe was closed dies of
/// the next write.
async fn read_diagnostics(stderr: Option<ChildStderr>, limit: usize) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };

    let mut kept = VecDeque::new();
    let mut buffer = [0_u8; READ_BUFFER];

    while let Ok(read) = stderr.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        kept.extend(&buffer[..read]);
        kept.drain(..kept.len().saturating_sub(limit));
    }

    let kept = Vec::from(kept);
    let start = kept
        .iter()
        .position(|byte| !is_continuation(*byte))
        .unwrap_or(kept.len());
    String::from_utf8_lossy(&kept[start..]).into_owned()
}

/// Whether `byte` continues a UTF-8 character rather than starting one.
fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::RequestOptions;
    use crate::llm::provider::settings::{DEFAULT_LINE_LIMIT, DEFAULT_OUTPUT_LIMIT};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    /// A stand-in agent, so no test needs a real CLI installed.
    ///
    /// It reads its prompt from stdin exactly as the real agents do, which is
    /// what keeps the delivery path under test the real one. Run without
    /// arguments, as only [`wait_until_executable`] runs it, it exits at once.
    fn fake(directory: &TempDir, script: &str) -> PathBuf {
        let path = directory.path().join("agent");
        let mut file = std::fs::File::create(&path).expect("the fake agent");
        write!(file, "#!/bin/sh\n[ \"$#\" -gt 0 ] || exit 0\n{script}\n")
            .expect("the fake agent body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the fake agent to be executable");
        wait_until_executable(&path);
        path
    }

    /// Runs the fake once to completion, before a test's deadlines start.
    ///
    /// Linux refuses to exec a file any process still holds open for writing.
    /// The descriptor here is closed, but a sibling test forking between its
    /// own open and exec inherits it for that window, so a freshly written
    /// script can hit ETXTBSY under a parallel run. macOS assesses a new
    /// executable on its first run, which can take seconds, and a run killed
    /// at once leaves that to the next run. Production never meets either: a
    /// provider execs an installed binary, not one it just wrote.
    fn wait_until_executable(path: &std::path::Path) {
        for _ in 0..50 {
            match std::process::Command::new(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Err(error) if error.raw_os_error() == Some(26) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => return,
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
    async fn a_codex_turn_that_recovers_from_an_error_still_answers() {
        let directory = TempDir::new().expect("a temporary directory");
        let recording = directory.path().join("recording.jsonl");
        std::fs::write(
            &recording,
            include_str!("parser/fixtures/codex/rung3-mock-reconnect-then-complete.jsonl"),
        )
        .expect("the recording");
        let script = format!("cat '{}'", recording.display());
        let provider = CliProvider::agent(AgentKind::Codex, settings(&directory, &script));

        let completion = run(&provider, &[Message::user("Echo the nonce.")])
            .await
            .expect("the answer codex reached after reconnecting");

        assert_eq!(
            completion.message.content.as_deref(),
            Some("The echo tool returned: Wall time: 0.0012 seconds\nOutput: r6-rung3-nonce-9b2d")
        );
    }

    #[tokio::test]
    async fn a_failed_codex_turn_carries_the_renewal_failure_codex_wrote_only_to_stderr() {
        let directory = TempDir::new().expect("a temporary directory");
        let recording = directory.path().join("recording.jsonl");
        std::fs::write(
            &recording,
            include_str!("parser/fixtures/codex/refresh-invalidated.jsonl"),
        )
        .expect("the recording");
        let diagnostics = directory.path().join("diagnostics.stderr");
        std::fs::write(
            &diagnostics,
            include_str!("parser/fixtures/codex/refresh-invalidated-errors.stderr"),
        )
        .expect("the diagnostics");
        let script = format!(
            "cat '{}' >&2\ncat '{}'\nexit 1",
            diagnostics.display(),
            recording.display()
        );
        let provider = CliProvider::agent(AgentKind::Codex, settings(&directory, &script));

        let error = run(&provider, &[Message::user("Echo the nonce.")])
            .await
            .expect_err("a failed turn");

        let rendered = error.to_string();
        assert!(
            rendered.contains("workspace routing discovery unauthorized (401)"),
            "lost codex's own wording: {rendered}"
        );
        assert!(
            rendered.contains("Your access token could not be refreshed"),
            "lost the reason codex gave on stderr: {rendered}"
        );
    }

    #[test]
    fn a_failure_keeps_only_the_last_whole_lines_of_stderr() {
        let lines: Vec<String> = (0..500)
            .map(|number| format!("diagnostic line {number}"))
            .collect();

        let rendered = failure("the turn failed", &lines.join("\n"));

        let (message, tail) = rendered
            .split_once(STDERR_HEADING)
            .expect("the agent's words, then its stderr");
        assert_eq!(message, "the turn failed");
        assert!(tail.len() <= DIAGNOSTIC_TAIL, "{} bytes", tail.len());
        assert!(tail.ends_with("diagnostic line 499"), "{tail}");
        assert!(
            tail.lines()
                .all(|line| lines.iter().any(|whole| whole == line)),
            "a line was cut short: {tail}"
        );
    }

    #[test]
    fn a_failure_cuts_one_long_stderr_line_between_characters() {
        let rendered = failure("the turn failed", &"—".repeat(1000));

        let (_, tail) = rendered
            .split_once(STDERR_HEADING)
            .expect("the agent's words, then its stderr");
        assert!(!tail.is_empty() && tail.len() <= DIAGNOSTIC_TAIL);
        assert!(tail.chars().all(|character| character == '—'), "{tail}");
    }

    /// An agent's own report can run to several lines. None of them may read
    /// as stderr, and no line of stderr as the agent's own words.
    #[test]
    fn a_failure_of_several_lines_stays_apart_from_the_stderr_after_it() {
        let words = "tool call error: tool call failed for `zone/echo`\n\nCaused by:\n    \
                     timed out awaiting tools/call after 2s";
        let stderr = "2026-09-23T07:43:43Z WARN codex_mcp: docs: 401 Unauthorized";

        let rendered = failure(words, stderr);

        assert_eq!(rendered.split_once(STDERR_HEADING), Some((words, stderr)));
    }

    #[test]
    fn a_failure_with_nothing_on_stderr_is_the_agents_words_alone() {
        assert_eq!(failure("the turn failed", " \n\t"), "the turn failed");
    }

    /// Codex echoes what each of zone's tools returned, so a file the agent
    /// read can make one event longer than any answer.
    fn codex_tool_result(id: usize, bytes: usize) -> String {
        json!({
            "type": "item.completed",
            "item": {
                "id": format!("item_{id}"),
                "type": "mcp_tool_call",
                "server": "zone",
                "tool": "read_file",
                "arguments": {"path": "a.rs"},
                "result": {"content": [{"type": "text", "text": "x".repeat(bytes)}]},
                "error": null,
                "status": "completed",
            },
        })
        .to_string()
    }

    const CODEX_ANSWER: &str = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"The file is large."}}"#;
    const CODEX_COMPLETED: &str =
        r#"{"type":"turn.completed","usage":{"input_tokens":40,"output_tokens":8}}"#;

    fn replaying(directory: &TempDir, lines: &[String]) -> String {
        let recording = directory.path().join("recording.jsonl");
        std::fs::write(&recording, format!("{}\n", lines.join("\n"))).expect("the recording");
        format!("cat '{}'", recording.display())
    }

    #[tokio::test]
    async fn an_event_past_the_line_limit_is_dropped_and_the_turn_still_answers() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = replaying(
            &directory,
            &[
                codex_tool_result(1, DEFAULT_LINE_LIMIT + 1),
                CODEX_ANSWER.to_string(),
                CODEX_COMPLETED.to_string(),
            ],
        );
        let provider = CliProvider::agent(AgentKind::Codex, settings(&directory, &script));

        let completion = run(&provider, &[Message::user("Read a.rs.")])
            .await
            .expect("the answer that followed the dropped event");

        assert_eq!(
            completion.message.content.as_deref(),
            Some("The file is large.")
        );
    }

    #[tokio::test]
    async fn an_unterminated_event_past_the_line_limit_is_skipped_to_its_end() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
head -c 5000 /dev/zero | tr '\0' 'x'
sleep 0.2
head -c 5000 /dev/zero | tr '\0' 'y'
echo
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Still here."}]}}'
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let mut settings = settings(&directory, script);
        settings.line_limit = 256;
        let provider = CliProvider::agent(AgentKind::Claude, settings);

        let completion = run(&provider, &[Message::user("hi")])
            .await
            .expect("the answer after the skipped event");

        assert_eq!(completion.message.content.as_deref(), Some("Still here."));
    }

    #[tokio::test]
    async fn tool_results_the_agent_reads_past_the_output_cap_do_not_end_its_turn() {
        let directory = TempDir::new().expect("a temporary directory");
        let mut lines: Vec<String> = (1..=32).map(|id| codex_tool_result(id, 4 * 1024)).collect();
        lines.extend([CODEX_ANSWER.to_string(), CODEX_COMPLETED.to_string()]);
        let script = replaying(&directory, &lines);
        let provider = CliProvider::agent(
            AgentKind::Codex,
            settings(&directory, &script).with_output_limit(4 * 1024),
        );

        let completion = run(&provider, &[Message::user("Read every file.")])
            .await
            .expect("an answer after 128 KiB of tool results");

        assert_eq!(
            completion.message.content.as_deref(),
            Some("The file is large.")
        );
    }

    /// A call codex made through zone's tools, whose arguments hold `bytes`
    /// of a file it wrote.
    fn codex_tool_call(id: usize, bytes: usize) -> String {
        json!({
            "type": "item.completed",
            "item": {
                "id": format!("item_{id}"),
                "type": "mcp_tool_call",
                "server": "zone",
                "tool": "write_file",
                "arguments": {"path": format!("part_{id}.txt"), "content": "x".repeat(bytes)},
                "result": {"content": [{"type": "text", "text": "Wrote it."}]},
                "error": null,
                "status": "completed",
            },
        })
        .to_string()
    }

    /// What a call was made with reaches no consumer: the answer keeps none of
    /// it and zone's loop keeps the tool's name. So a turn whose calls carry
    /// more than the cap between them, under the default limits, still answers.
    #[tokio::test]
    async fn tool_arguments_past_the_output_cap_do_not_end_its_turn() {
        let directory = TempDir::new().expect("a temporary directory");
        let bytes = DEFAULT_LINE_LIMIT - 64 * 1024;
        let calls = DEFAULT_OUTPUT_LIMIT / bytes + 1;
        let mut lines: Vec<String> = (1..=calls).map(|id| codex_tool_call(id, bytes)).collect();
        lines.extend([CODEX_ANSWER.to_string(), CODEX_COMPLETED.to_string()]);
        let script = replaying(&directory, &lines);
        let provider = CliProvider::agent(AgentKind::Codex, settings(&directory, &script));

        let completion = run(&provider, &[Message::user("Write every part.")])
            .await
            .expect("an answer after more tool arguments than the output cap");

        assert!(calls * bytes > DEFAULT_OUTPUT_LIMIT);
        assert_eq!(
            completion.message.content.as_deref(),
            Some("The file is large.")
        );
    }

    #[tokio::test]
    async fn stderr_past_the_output_cap_is_still_drained() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
head -c 262144 /dev/zero | tr '\0' 'e' >&2
echo 'still logging' >&2
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}'
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let provider = CliProvider::agent(
            AgentKind::Claude,
            settings(&directory, script)
                .with_output_limit(4 * 1024)
                .with_timeout(Duration::from_secs(5)),
        );

        let completion = run(&provider, &[Message::user("hi")])
            .await
            .expect("an answer from an agent that wrote 256 KiB to stderr");

        assert_eq!(completion.message.content.as_deref(), Some("Done."));
    }

    #[tokio::test]
    async fn a_failure_past_the_output_cap_reports_the_end_of_stderr() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
head -c 262144 /dev/zero | tr '\0' 'e' >&2
echo 'error: the real reason' >&2
exit 3
"#;
        let provider = CliProvider::agent(
            AgentKind::Claude,
            settings(&directory, script)
                .with_output_limit(4 * 1024)
                .with_timeout(Duration::from_secs(5)),
        );

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
        assert!(
            message.trim_end().ends_with("error: the real reason"),
            "lost the end of stderr: {}",
            &message[message.len().saturating_sub(200)..]
        );
        assert!(message.len() <= 4 * 1024 + 256, "{} bytes", message.len());
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
    async fn an_answer_past_the_output_cap_stops_the_run_instead_of_waiting_for_the_timeout() {
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
while :; do
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"xxxxxxxxxxxxxxxx"}]}}'
done
"#;
        let settings = settings(&directory, script).with_output_limit(4 * 1024);
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

    fn toolset() -> Toolset {
        Toolset::new(
            "http://127.0.0.1:8421/mcp",
            "zone-turn-notarealtoken",
            ["read_file"],
        )
    }

    /// A stand-in agent that writes down how it was started -- its arguments
    /// and its whole environment -- and then finishes the way claude does.
    struct Recorder {
        directory: TempDir,
    }

    impl Recorder {
        const ARGUMENTS: &'static str = "arguments";
        const ENVIRONMENT: &'static str = "environment";

        fn new() -> Self {
            Self {
                directory: TempDir::new().expect("a temporary directory"),
            }
        }

        fn settings(&self) -> CliSettings {
            let script = format!(
                r#"
cat > /dev/null
printf '%s\0' "$@" > '{arguments}'
/usr/bin/env -0 > '{environment}'
echo '{result}'
"#,
                arguments = self.path(Self::ARGUMENTS).display(),
                environment = self.path(Self::ENVIRONMENT).display(),
                result = r#"{"type":"result","subtype":"success","is_error":false}"#,
            );
            settings(&self.directory, &script)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        fn entries(&self, name: &str) -> Vec<String> {
            let recorded = std::fs::read(self.path(name)).expect("the agent's record");
            String::from_utf8_lossy(&recorded)
                .split_terminator('\0')
                .map(str::to_string)
                .collect()
        }

        fn arguments(&self) -> Vec<String> {
            self.entries(Self::ARGUMENTS)
        }

        fn environment(&self) -> BTreeMap<String, String> {
            self.entries(Self::ENVIRONMENT)
                .iter()
                .filter_map(|entry| entry.split_once('='))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect()
        }
    }

    async fn answered(settings: CliSettings) {
        let provider = CliProvider::agent(AgentKind::Claude, settings);
        run(&provider, &[Message::user("hi")])
            .await
            .expect("an answer");
    }

    /// Cargo sets it in every test process it runs, the way the server's own
    /// configuration sits in the server's environment.
    const SERVERS_OWN: &str = "CARGO_MANIFEST_DIR";

    #[tokio::test]
    async fn a_variable_from_the_servers_own_environment_never_reaches_the_agent() {
        assert!(
            std::env::var_os(SERVERS_OWN).is_some(),
            "cargo sets {SERVERS_OWN} for every test it runs"
        );
        let directory = TempDir::new().expect("a temporary directory");
        let script = r#"
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "${CARGO_MANIFEST_DIR:-absent}"
echo '{"type":"result","subtype":"success","is_error":false}'
"#;
        let provider = CliProvider::agent(AgentKind::Claude, settings(&directory, script));

        let completion = run(&provider, &[Message::user("hi")])
            .await
            .expect("an answer");

        assert_eq!(completion.message.content.as_deref(), Some("absent"));
    }

    /// Marks the copy of this test binary that runs with a server's secrets.
    const SERVER_COPY: &str = "ZONE_CORE_TEST_SERVER_COPY";

    /// What a server's environment holds that no agent may be handed. Every
    /// value says `notreal`, so a leak shows up under any name.
    const SERVER_SECRETS: [(&str, &str); 11] = [
        (
            "DATABASE_URL",
            "postgres://zone:notrealpassword@postgres/zone",
        ),
        ("JWT_SECRET", "notreal-jwt-secret"),
        ("ENCRYPTION_KEY", "notreal-encryption-key"),
        ("LITELLM_KEY", "sk-notreal-litellm-key"),
        ("ANTHROPIC_API_KEY", "sk-ant-notreal-key"),
        ("OPENAI_API_KEY", "sk-notreal-openai-key"),
        ("CLAUDE_CONFIG_DIR", "/notreal/server/claude"),
        ("CODEX_HOME", "/notreal/server/codex"),
        ("CLAUDECODE", "notreal-session"),
        ("CLAUDE_CODE_ENTRYPOINT", "notreal-entrypoint"),
        ("ZONE_MCP_TOKEN", "notreal-leftover-turn-token"),
    ];

    /// Setting the secrets in this process would race every other test that
    /// spawns a child, so a copy of the binary runs this test with them set.
    #[tokio::test]
    async fn the_servers_secrets_never_reach_the_agent() {
        if std::env::var_os(SERVER_COPY).is_none() {
            let output =
                tokio::process::Command::new(std::env::current_exe().expect("this test binary"))
                    .args([
                        "--exact",
                        "llm::provider::cli::tests::the_servers_secrets_never_reach_the_agent",
                        "--nocapture",
                    ])
                    .env(SERVER_COPY, "1")
                    .envs(SERVER_SECRETS)
                    .env(environment::PASSTHROUGH, "CORPORATE_CA")
                    .env("CORPORATE_CA", "/etc/ssl/corporate.pem")
                    .output()
                    .await
                    .expect("a copy of this test binary");
            let printed = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            assert!(output.status.success(), "{printed}");
            assert!(
                printed.contains("1 passed"),
                "the copy ran no test: {printed}"
            );
            return;
        }

        let recorder = Recorder::new();
        answered(recorder.settings()).await;

        let environment = recorder.environment();
        let leaked: Vec<&str> = SERVER_SECRETS
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| environment.contains_key(*name))
            .collect();
        assert!(leaked.is_empty(), "these reached the agent: {leaked:?}");
        let carrying: Vec<&String> = environment
            .iter()
            .filter(|(_, value)| value.contains("notreal"))
            .map(|(name, _)| name)
            .collect();
        assert!(
            carrying.is_empty(),
            "these carried a secret's value to the agent: {carrying:?}"
        );
        assert_eq!(
            environment.get("CORPORATE_CA").map(String::as_str),
            Some("/etc/ssl/corporate.pem"),
            "the operator's passthrough was not honoured"
        );
    }

    #[tokio::test]
    async fn the_agent_still_finds_its_home_and_its_commands() {
        let recorder = Recorder::new();
        answered(recorder.settings()).await;

        let environment = recorder.environment();
        for name in ["HOME", "PATH"] {
            assert_eq!(
                environment.get(name),
                std::env::var(name).ok().as_ref(),
                "{name} did not reach the agent as it was"
            );
        }
    }

    #[tokio::test]
    async fn the_variables_zone_sets_reach_the_agent_over_what_it_would_inherit() {
        let recorder = Recorder::new();
        answered(
            recorder
                .settings()
                .with_variable("CLAUDE_CONFIG_DIR", "/state/organization/claude")
                .with_variable("DISABLE_AUTOUPDATER", "1")
                .with_variable("HOME", "/state/organization/home"),
        )
        .await;

        let environment = recorder.environment();
        for (name, value) in [
            ("CLAUDE_CONFIG_DIR", "/state/organization/claude"),
            ("DISABLE_AUTOUPDATER", "1"),
            ("HOME", "/state/organization/home"),
        ] {
            assert_eq!(
                environment.get(name).map(String::as_str),
                Some(value),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn the_credential_outranks_a_variable_of_the_same_name() {
        let recorder = Recorder::new();
        answered(
            recorder
                .settings()
                .with_variable("CLAUDE_CODE_OAUTH_TOKEN", "set-as-a-variable")
                .with_credential(Credential::key(
                    "CLAUDE_CODE_OAUTH_TOKEN",
                    "sk-ant-oat01-the-credential",
                )),
        )
        .await;

        assert_eq!(
            recorder
                .environment()
                .get("CLAUDE_CODE_OAUTH_TOKEN")
                .map(String::as_str),
            Some("sk-ant-oat01-the-credential")
        );
    }

    #[tokio::test]
    async fn the_turns_token_reaches_the_agents_environment_and_never_its_arguments() {
        let recorder = Recorder::new();
        answered(recorder.settings().with_toolset(toolset())).await;

        assert_eq!(
            recorder
                .environment()
                .get(Toolset::TOKEN_VARIABLE)
                .map(String::as_str),
            Some("zone-turn-notarealtoken")
        );

        let arguments = recorder.arguments();
        assert!(
            arguments.iter().any(|argument| argument == "--mcp-config"),
            "{arguments:?}"
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "mcp__zone__read_file"),
            "{arguments:?}"
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.contains("zone-turn-notarealtoken")),
            "the token reached argv: {arguments:?}"
        );
    }

    #[tokio::test]
    async fn a_turn_serving_no_tools_withholds_the_agents_own_and_hands_out_no_token() {
        let recorder = Recorder::new();
        answered(recorder.settings()).await;

        assert!(
            !recorder.environment().contains_key(Toolset::TOKEN_VARIABLE),
            "a turn serving no tools was handed a token"
        );
        let arguments = recorder.arguments();
        assert!(
            arguments.iter().any(|argument| argument == "--tools"),
            "the agent kept its own file and shell tools: {arguments:?}"
        );
    }
}
