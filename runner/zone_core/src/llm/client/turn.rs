use std::path::Path;
use std::sync::Arc;

use abnegate_agent_cli::{AgentEvent, AgentKind, AgentStream, StdoutParseResult, activity};
use abnegate_llm::finish_reason::STOP;
use abnegate_llm::{
    ChatStreamChunk, Completion, Error, Message, ProviderError, StreamChoice, StreamDelta,
};
use futures::{Stream, StreamExt};

use super::reported::Reported;
use crate::llm::cli::{Observer, SignIn, Toolset};
use crate::llm::window;

/// What one agent turn's stream needs beyond its events: the agent, whose
/// sign-in it runs under and where, who is told of its tools and subagents,
/// and where the context it reports is kept.
pub(super) struct Turn {
    agent: AgentKind,
    sign_in: SignIn,
    directory: Option<String>,
    observer: Option<Arc<dyn Observer>>,
    reported: Reported,
    hidden: String,
    spoke: bool,
}

impl Turn {
    pub(super) fn new(
        agent: AgentKind,
        sign_in: SignIn,
        directory: Option<&Path>,
        observer: Option<Arc<dyn Observer>>,
        reported: Reported,
    ) -> Self {
        Self {
            agent,
            sign_in,
            directory: directory.map(|directory| directory.display().to_string()),
            observer,
            reported,
            hidden: agent.tool(Toolset::SERVER, ""),
            spoke: false,
        }
    }

    /// Hand `event` to whoever it is for besides the reader: a tool's output
    /// and a subagent to the observer, a turn on usage credits to the log,
    /// and the context the agent reports to the client. Returns whether that
    /// is all the event was for.
    fn notice(&self, event: &AgentEvent) -> bool {
        match event {
            AgentEvent::Output(output) => {
                if let Some(observer) = &self.observer {
                    observer.observe(output);
                }
                true
            }
            AgentEvent::Subagent(subagent) => match &self.observer {
                Some(observer) => {
                    observer.track(subagent);
                    true
                }
                None => false,
            },
            AgentEvent::Credits(window) => {
                tracing::info!(
                    provider = %self.agent,
                    sign_in = ?self.sign_in,
                    directory = ?self.directory,
                    %window,
                    "the turn runs on usage credits past the plan's limit"
                );
                true
            }
            AgentEvent::Occupancy(occupancy) => {
                self.reported.record(self.agent, *occupancy);
                true
            }
            _ => false,
        }
    }

    /// One agent event in the shape the agent loop already consumes, or
    /// `None` for one that is no chunk at all.
    ///
    /// Tool activity becomes reasoning rather than a tool-call delta. The
    /// agent has already run those tools, so replaying them through zone's
    /// registry would run each a second time -- but a turn that spends ten
    /// minutes reading and editing files should not look to the reader like a
    /// turn doing nothing, so what the agent reached for is listed as the
    /// thinking it was. Zone's own tools reach the reader as cards when the
    /// agent calls them over MCP. A subagent's calls are listed the same way
    /// when no observer took them.
    ///
    /// A failure is never an event: the crate ends the stream in the run's
    /// error instead, carrying the end of the agent's stderr.
    fn chunk(&mut self, event: AgentEvent) -> Option<ChatStreamChunk> {
        if self.notice(&event) {
            return None;
        }

        let mut delta = StreamDelta::default();
        let mut finished = false;
        let mut usage = None;
        let mut reported_window = None;
        let mut session = None;

        match event {
            AgentEvent::Text(text) => {
                self.spoke = true;
                delta.content = Some(text);
            }
            AgentEvent::Answer(answer) => {
                if self.spoke {
                    return None;
                }
                self.spoke = true;
                delta.content = Some(answer);
            }
            AgentEvent::Tool(call) => {
                delta.reasoning_content = activity(&call, &[&self.hidden]);
            }
            AgentEvent::Subagent(subagent) => {
                delta.reasoning_content = Some(subagent.activity(&[&self.hidden])?);
            }
            AgentEvent::Usage(counts) => usage = Some(counts),
            AgentEvent::Window(window) => reported_window = Some(window::renamed(window)),
            AgentEvent::Session(announced) => session = Some(announced),
            AgentEvent::Finished { finish_reason, .. } => {
                tracing::debug!(
                    provider = %self.agent,
                    reported = finish_reason.as_deref().unwrap_or("none"),
                    "agent finished its turn"
                );
                finished = true;
            }
            _ => return None,
        }

        let choice = StreamChoice::new(0, delta);
        let choice = if finished {
            choice.with_finish_reason(STOP)
        } else {
            choice
        };
        let mut chunk = ChatStreamChunk::new(vec![choice]);
        chunk.usage = usage;
        chunk.window = reported_window;
        chunk.session = session;
        Some(chunk)
    }

    /// `events` as the chunks the agent loop reads, ending in the run's
    /// error when it failed.
    pub(super) fn chunks(
        mut self,
        events: AgentStream,
    ) -> impl Stream<Item = Result<ChatStreamChunk, Error>> + Send {
        async_stream::stream! {
            let mut events = events;
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => {
                        if let Some(chunk) = self.chunk(event) {
                            yield Ok(chunk);
                        }
                    }
                    Err(error) => {
                        yield Err(failed(error));
                        break;
                    }
                }
            }
        }
    }

    /// What `events` amount to once the run has ended, read as the crate's
    /// own `complete` reads them, with each event noticed on the way as a
    /// streamed turn notices it.
    pub(super) async fn completion(self, mut events: AgentStream) -> Result<Completion, Error> {
        let mut stdout = StdoutParseResult::default();
        while let Some(event) = events.next().await {
            let event = event.map_err(failed)?;
            self.notice(&event);
            stdout.record(event);
        }
        Ok(
            Completion::new(self.agent.as_str(), Message::assistant(stdout.reply()))
                .with_usage(stdout.usage)
                .with_finish_reason(stdout.finish_reason),
        )
    }
}

/// The error a run ended in, with the window a limit refused it on under
/// the name zone records it by.
pub(super) fn failed(error: ProviderError) -> Error {
    let mut error = Error::from(error);
    if let Error::Limited { limit, .. } = &mut error {
        limit.window = limit.window.take().map(window::renamed);
    }
    error
}
