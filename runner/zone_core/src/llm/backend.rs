use abnegate_agent_cli::{AgentKind, CliSettings};

use super::cli::SignIn;

/// Where completions come from.
#[derive(Debug, Clone, Default)]
pub enum LlmBackend {
    /// The OpenAI-compatible endpoint at
    /// [`LlmConfig::base_url`](abnegate_llm::LlmConfig::base_url).
    #[default]
    Http,
    /// A coding agent CLI on this host, run as a child process. A single-user
    /// self-host can point zone at the `claude` or `codex` it has already
    /// signed in and spend that subscription instead of a metered API key.
    Cli {
        agent: AgentKind,
        settings: Box<CliSettings>,
        sign_in: SignIn,
    },
}

impl LlmBackend {
    /// `agent` run under `settings`, on the server's own sign-in until
    /// [`Self::with_sign_in`] says whose.
    pub fn cli(agent: AgentKind, settings: CliSettings) -> Self {
        Self::Cli {
            agent,
            settings: Box::new(settings),
            sign_in: SignIn::default(),
        }
    }

    /// This backend run under `sign_in`. An HTTP backend signs in with its
    /// key and is left as it was.
    pub fn with_sign_in(mut self, sign_in: SignIn) -> Self {
        if let Self::Cli {
            sign_in: current, ..
        } = &mut self
        {
            *current = sign_in;
        }
        self
    }

    /// Whose sign-in the agent runs under, or `None` for an HTTP backend.
    pub fn sign_in(&self) -> Option<SignIn> {
        match self {
            Self::Http => None,
            Self::Cli { sign_in, .. } => Some(*sign_in),
        }
    }

    /// This backend with its agent's settings passed through `configure`,
    /// keeping the agent and whose sign-in it runs under. An HTTP backend
    /// has no settings and is left as it was.
    pub fn configured(self, configure: impl FnOnce(CliSettings) -> CliSettings) -> Self {
        match self {
            Self::Cli {
                agent,
                settings,
                sign_in,
            } => Self::Cli {
                agent,
                settings: Box::new(configure(*settings)),
                sign_in,
            },
            Self::Http => Self::Http,
        }
    }
}
