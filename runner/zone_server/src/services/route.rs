//! Where a workspace's or a chat's completions go, read from its AI settings once.

use std::fmt;

use abnegate_agent_cli::AgentKind;
use uuid::Uuid;
use zone_core::llm::LlmBackend;

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings};
use crate::db::chats::provider::{self, Choice};
use crate::db::chats::{ChatRow, ChatSession};
use crate::db::workspaces;
use crate::services::backend::{self, Continuation, Resolved, Routing};
use crate::services::endpoint::{Endpoint, UrlError};
use crate::services::provider::Provider;
use crate::services::stages::Preferences;
use crate::state::AppState;

/// Why a workspace's saved endpoint, or a chat's chosen provider, cannot take its completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The saved URL fails [`crate::services::endpoint::validate_url`].
    Url(UrlError),
    /// The URL completions would go to, saved or a provider's default, is on a
    /// host `ZONE_ENDPOINT_HOSTS` does not list.
    Host,
    /// The workspace or its AI settings could not be read.
    Unreadable,
    /// The provider a chat chose could not be read.
    ChoiceUnreadable,
    /// The AI settings the provider a chat chose runs on could not be read.
    SettingsUnreadable(Provider),
    /// The provider a chat chose has no key, endpoint or credentials saved.
    Unconfigured(Provider),
    /// The URL saved for the provider a chat chose fails
    /// [`crate::services::endpoint::validate_url`], its host unlisted included.
    Refused(Provider, UrlError),
    /// The model the chat chose is not one its provider's settings save.
    Unsaved(Provider),
}

impl From<UrlError> for Reason {
    fn from(error: UrlError) -> Self {
        match error {
            UrlError::Unlisted => Self::Host,
            error => Self::Url(error),
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => {
                formatter.write_str("its saved URL is refused (")?;
                clause(formatter, *error)?;
                formatter.write_str(")")
            }
            Self::Host => {
                formatter.write_str("its host isn't one this instance allows endpoints on")
            }
            Self::Unreadable | Self::SettingsUnreadable(_) => {
                formatter.write_str("its AI settings couldn't be read")
            }
            Self::ChoiceUnreadable => formatter.write_str("its choice couldn't be read"),
            Self::Unconfigured(_) => {
                formatter.write_str("no key, endpoint or credentials are saved for it")
            }
            Self::Refused(_, error) => {
                formatter.write_str("its saved endpoint is refused (")?;
                clause(formatter, *error)?;
                formatter.write_str(")")
            }
            Self::Unsaved(_) => {
                formatter.write_str("the model this chat chose isn't saved for it in AI Settings")
            }
        }
    }
}

/// `error` as a clause: without its closing period, its first letter lowercased.
fn clause(formatter: &mut fmt::Formatter<'_>, error: UrlError) -> fmt::Result {
    let sentence = error.to_string();
    let mut characters = sentence.trim_end_matches('.').chars();
    match characters.next() {
        Some(first) => write!(
            formatter,
            "{}{}",
            first.to_ascii_lowercase(),
            characters.as_str()
        ),
        None => Ok(()),
    }
}

/// A saved endpoint, or a chat's chosen provider, whose completions fail rather than go anywhere
/// its organization or the chat did not choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub struct Unusable {
    pub reason: Reason,
}

impl fmt::Display for Unusable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = self.reason;
        match reason {
            Reason::Unconfigured(provider) => write!(
                formatter,
                "This chat's AI provider, {}, can't be used: {reason}. Choose another provider for this chat, or ask an admin to set it up in AI Settings.",
                provider.label()
            ),
            Reason::Refused(provider, _) => write!(
                formatter,
                "This chat's AI provider, {}, can't be used: {reason}. Choose another provider for this chat, or ask an admin to fix it in AI Settings.",
                provider.label()
            ),
            Reason::Unsaved(provider) => write!(
                formatter,
                "This chat's AI provider, {}, can't be used: {reason}. Choose a saved model for this chat, or ask an admin to save this one.",
                provider.label()
            ),
            Reason::SettingsUnreadable(provider) => write!(
                formatter,
                "This chat's AI provider, {}, can't be used: {reason}. Try again shortly.",
                provider.label()
            ),
            Reason::ChoiceUnreadable => write!(
                formatter,
                "This chat's AI provider can't be used: {reason}. Try again shortly."
            ),
            Reason::Url(_) | Reason::Host | Reason::Unreadable => write!(
                formatter,
                "This workspace's AI endpoint can't be used: {reason}. Check AI Settings."
            ),
        }
    }
}

impl From<Reason> for Unusable {
    fn from(reason: Reason) -> Self {
        Self { reason }
    }
}

/// What a workspace's AI settings choose for its completions: the endpoint an
/// HTTP backend sends them to, and the settings themselves, read once.
///
/// Only a workspace with no saved settings, or with a row saved before
/// completions were routed, runs on the instance. Settings that cannot be read
/// or name an endpoint that cannot be used leave the route [`Unusable`]: its
/// completions fail, and never fall back to the instance.
///
/// A chat that chose its provider runs on that provider alone, a coding agent
/// on that agent's logins alone, or fails: never on its workspace's provider.
#[derive(Clone)]
pub struct Route {
    endpoint: Result<Endpoint, Unusable>,
    saved: Option<Saved>,
    chosen: Option<Choice>,
    /// Whether the context window a chat keeps for its workspace's provider
    /// bounds its completions: on that provider, or on the same self-hosted
    /// endpoint chosen by name.
    windowed: bool,
}

#[derive(Clone)]
struct Saved {
    organization: Uuid,
    settings: EffectiveAiSettings,
}

impl Route {
    pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Self {
        match saved(state, workspace).await {
            Ok(Some((organization, settings))) => {
                Self::new(state.config(), workspace, organization, settings)
            }
            Ok(None) => Self::instance(state.config()),
            Err(reason) => Self::unusable(reason),
        }
    }

    /// The route `chat`'s completions take: the provider the chat chose, on
    /// its workspace's saved keys and endpoints, else its workspace's route.
    pub async fn for_chat(state: &AppState, chat: &ChatRow) -> Self {
        let Some(workspace) = chat.workspace_id else {
            return Self::instance(state.config());
        };
        let choice = match provider::get(state.db(), chat.id).await {
            Ok(Some(choice)) => choice,
            Ok(None) => return Self::for_workspace(state, workspace).await,
            Err(error) => {
                tracing::warn!(
                    chat_id = %chat.id,
                    %error,
                    "Could not read the chat's chosen provider; its completions fail until it can be read"
                );
                return Self::unusable(Reason::ChoiceUnreadable);
            }
        };
        match saved(state, workspace).await {
            Ok(Some((organization, settings))) => {
                Self::choosing(state.config(), workspace, organization, &settings, choice)
            }
            Ok(None) | Err(_) => {
                let reason = Reason::SettingsUnreadable(choice.provider);
                Self {
                    chosen: Some(choice),
                    ..Self::unusable(reason)
                }
            }
        }
    }

    /// The route `choice` takes on `settings`, one of `organization`'s for
    /// `workspace`: unusable when nothing saved reaches the chosen provider,
    /// or when its endpoint saves models and not the one chosen, and never
    /// the workspace's provider or another model instead.
    fn choosing(
        config: &Config,
        workspace: Uuid,
        organization: Uuid,
        settings: &EffectiveAiSettings,
        choice: Choice,
    ) -> Self {
        let provider = choice.provider;
        let windowed = provider == Provider::SelfHosted
            && Provider::named(&settings.provider) == Some(Provider::SelfHosted);
        let settings = settings.choosing(&choice);
        let endpoint = match provider.configured(&settings) {
            true => Endpoint::try_resolve(config, &settings).map_err(|error| {
                tracing::warn!(
                    %workspace,
                    provider = %provider.as_str(),
                    %error,
                    "The endpoint saved for a chat's chosen provider can't be used; the chat's completions fail until it is fixed or another is chosen"
                );
                Unusable::from(Reason::Refused(provider, error))
            }),
            false => Err(Unusable::from(Reason::Unconfigured(provider))),
        };
        let endpoint = endpoint.and_then(|endpoint| {
            match choice.model.as_deref() {
                Some(model)
                    if provider.agent().is_none()
                        && !Preferences::for_endpoint(&settings, "", &endpoint).admits(model) =>
                {
                    tracing::warn!(
                        %workspace,
                        provider = %provider.as_str(),
                        model,
                        "A chat's chosen model is no longer saved for its provider; the chat's completions fail until a saved one is chosen"
                    );
                    Err(Unusable::from(Reason::Unsaved(provider)))
                }
                _ => Ok(endpoint),
            }
        });
        Self {
            endpoint,
            saved: Some(Saved {
                organization,
                settings,
            }),
            chosen: Some(choice),
            windowed,
        }
    }

    /// The route of a chat in `workspace` with `choice` among `settings`, already read.
    pub fn of(
        config: &Config,
        workspace: Uuid,
        organization: Uuid,
        settings: EffectiveAiSettings,
        choice: Option<Choice>,
    ) -> Self {
        match choice {
            Some(choice) => Self::choosing(config, workspace, organization, &settings, choice),
            None => Self::new(config, workspace, organization, settings),
        }
    }

    /// The route `settings`, one of `organization`'s for `workspace`, choose.
    pub fn new(
        config: &Config,
        workspace: Uuid,
        organization: Uuid,
        settings: EffectiveAiSettings,
    ) -> Self {
        let endpoint = Endpoint::try_resolve(config, &settings).map_err(|error| {
            tracing::warn!(
                %workspace,
                provider = %settings.provider,
                %error,
                "The endpoint saved in AI Settings can't be used; its completions fail until it is fixed"
            );
            Unusable::from(Reason::from(error))
        });
        Self {
            endpoint,
            saved: Some(Saved {
                organization,
                settings,
            }),
            chosen: None,
            windowed: true,
        }
    }

    pub fn instance(config: &Config) -> Self {
        Self {
            endpoint: Ok(Endpoint::instance(config)),
            saved: None,
            chosen: None,
            windowed: true,
        }
    }

    fn unusable(reason: Reason) -> Self {
        Self {
            endpoint: Err(reason.into()),
            saved: None,
            chosen: None,
            windowed: true,
        }
    }

    /// The provider the chat chose, when it chose one rather than follow its
    /// workspace's.
    pub fn chosen(&self) -> Option<&Choice> {
        self.chosen.as_ref()
    }

    /// The chat's choice as read: none when it couldn't be read, and no
    /// choice while the chat follows its workspace's provider.
    pub fn choice(&self) -> Option<Option<&Choice>> {
        match self.endpoint {
            Err(Unusable {
                reason: Reason::ChoiceUnreadable,
            }) => None,
            _ => Some(self.chosen()),
        }
    }

    /// The model the chat's completions ask for, given `stored`, the model the
    /// chat keeps for its workspace's provider.
    pub fn requested(&self, stored: &str) -> String {
        provider::requested(self.chosen(), stored)
    }

    /// `chat` as its completions see it: on a chosen provider, the model it
    /// asks that provider for, and the context window it keeps for its
    /// workspace's provider only when it chose that same self-hosted endpoint.
    pub fn apply(&self, mut chat: ChatRow) -> ChatRow {
        if self.chosen.is_some() {
            chat.model_name = self.requested(&chat.model_name);
        }
        if !self.windowed {
            chat.context_tokens = None;
        }
        chat
    }

    /// The endpoint completions go to, or why there is none.
    pub fn endpoint(&self) -> Result<&Endpoint, Unusable> {
        self.endpoint.as_ref().map_err(|unusable| *unusable)
    }

    pub fn into_endpoint(self) -> Result<Endpoint, Unusable> {
        self.endpoint
    }

    /// These settings with their completions sent to the instance's own
    /// endpoint instead. An unusable route stays unusable.
    pub fn on_instance(self, config: &Config) -> Self {
        Self {
            endpoint: self.endpoint.map(|_| Endpoint::instance(config)),
            ..self
        }
    }

    pub fn settings(&self) -> Option<&EffectiveAiSettings> {
        self.saved.as_ref().map(|saved| &saved.settings)
    }

    /// The coding agent CLI this route prefers for its turns; the router may pick another agent's
    /// login as a turn starts. None when they go over HTTP or this route is unusable.
    pub fn agent(&self, config: &Config) -> Option<AgentKind> {
        let origin = self.endpoint().ok()?.origin();
        match self.settings() {
            Some(settings) => backend::agent(config, settings, origin),
            None => config.model_backend().agent(),
        }
    }

    /// The organization whose settings run its workspace, or the chat, on `agent`.
    pub fn organization_on(&self, agent: AgentKind) -> Option<Uuid> {
        self.saved
            .as_ref()
            .filter(|saved| saved.settings.agent() == Some(agent))
            .map(|saved| saved.organization)
    }

    /// The backend these settings choose for a session or run starting now, on the login the
    /// router picks: `sticky` while it can still run, never one in `exclude`, and one that lately
    /// refused `model`, what the session runs on the configured agent, only when no other can
    /// run it. The login picked is recorded as used. An unusable route has none.
    pub async fn resolve(
        &self,
        state: &AppState,
        exclude: &[Uuid],
        sticky: Option<Uuid>,
        model: Option<&str>,
    ) -> Result<Resolved, backend::Error> {
        self.routed(
            state,
            Routing {
                exclude,
                sticky,
                touch: true,
                model,
                ..Routing::default()
            },
        )
        .await
    }

    /// [`Self::resolve`] for a chat's turn running `model` on the configured agent: kept on the
    /// login `previous`, the chat's session, runs on, and in that session while the login picked
    /// holds it, else in a fresh one.
    pub async fn chat(
        &self,
        state: &AppState,
        exclude: &[Uuid],
        previous: Option<&ChatSession>,
        model: Option<&str>,
    ) -> Result<Resolved, backend::Error> {
        self.routed(
            state,
            Routing {
                exclude,
                sticky: previous.and_then(|session| session.login),
                touch: true,
                continuation: Continuation::Chat(previous),
                model,
                ..Routing::default()
            },
        )
        .await
    }

    /// The backend these settings choose, resolved when a completion needs
    /// one: a coding agent's sign-in is read only then, and the login it runs
    /// on is not recorded as used. An unusable route has none.
    pub async fn backend(&self, state: &AppState) -> Result<LlmBackend, backend::Error> {
        self.backend_for(state, None).await
    }

    /// [`Self::backend`], kept on `sticky`, the login a chat runs on, while it can still run.
    pub async fn backend_for(
        &self,
        state: &AppState,
        sticky: Option<Uuid>,
    ) -> Result<LlmBackend, backend::Error> {
        self.routed(
            state,
            Routing {
                sticky,
                ..Routing::default()
            },
        )
        .await
        .map(|resolved| resolved.backend)
    }

    /// A chosen agent is routed strictly: onto its own logins, never another
    /// agent's.
    async fn routed(
        &self,
        state: &AppState,
        routing: Routing<'_>,
    ) -> Result<Resolved, backend::Error> {
        let endpoint = self.endpoint()?;
        let routing = Routing {
            strict: self.chosen().is_some(),
            ..routing
        };
        match &self.saved {
            Some(saved) => {
                backend::for_settings(
                    state,
                    saved.organization,
                    &saved.settings,
                    endpoint.origin(),
                    routing,
                )
                .await
            }
            None => Ok(Resolved::unrouted(backend::instance(state.config()))),
        }
    }

    /// The models these settings prefer for completions sent to this route's
    /// endpoint.
    pub fn preferences(&self, classifier: &str) -> Preferences {
        match (self.settings(), self.endpoint()) {
            (Some(settings), Ok(endpoint)) => {
                Preferences::for_endpoint(settings, classifier, endpoint)
            }
            _ => Preferences::from_optional_settings(None, classifier),
        }
    }
}

/// The organization `workspace` belongs to and its effective AI settings,
/// nothing when there is no such workspace, or [`Reason::Unreadable`] when
/// either cannot be read, with the reason logged.
pub(crate) async fn saved(
    state: &AppState,
    workspace: Uuid,
) -> Result<Option<(Uuid, EffectiveAiSettings)>, Reason> {
    #[cfg(test)]
    reads::record(workspace);
    let organization = match workspaces::get_workspace(state.db(), workspace).await {
        Ok(Some(row)) => row.organization_id,
        Ok(None) => {
            tracing::warn!(%workspace, "No such workspace");
            return Ok(None);
        }
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the workspace; its completions fail until it can be read"
            );
            return Err(Reason::Unreadable);
        }
    };
    match ai_settings::get_effective_ai_settings(state.db(), organization, workspace).await {
        Ok(settings) => Ok(Some((organization, settings))),
        Err(error) => {
            tracing::warn!(
                %workspace,
                %error,
                "Could not read the AI settings; the workspace's completions fail until they can be read"
            );
            Err(Reason::Unreadable)
        }
    }
}

#[cfg(test)]
pub(crate) mod reads {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use uuid::Uuid;

    static READS: LazyLock<Mutex<HashMap<Uuid, usize>>> = LazyLock::new(Mutex::default);

    pub(super) fn record(workspace: Uuid) {
        *READS
            .lock()
            .expect("the read counter")
            .entry(workspace)
            .or_default() += 1;
    }

    /// How many times `workspace`'s AI settings have been read.
    pub fn of(workspace: Uuid) -> usize {
        READS
            .lock()
            .expect("the read counter")
            .get(&workspace)
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use sqlx::PgPool;
    use tempfile::TempDir;
    use zone_context::embeddings::providers::{PROVIDER_OPENAI, PROVIDER_SELF_HOSTED};
    use zone_core::llm::SignIn;

    use super::*;
    use crate::config::{AgentConfig, ModelBackend};
    use crate::db::ai_settings::{PROVIDER_ANTHROPIC, PROVIDER_CLAUDE_CODE};
    use crate::db::{agent_logins, chats, organizations};
    use crate::services::endpoint::testing::settings;
    use crate::services::login::usage::Snapshot;
    use crate::services::stages::AUTO;

    const INVALID: &str = "The endpoint saved in AI Settings can't be used";

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the captured log")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn lines(&self) -> Vec<String> {
            String::from_utf8_lossy(&self.0.lock().expect("the captured log"))
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[tokio::test]
    async fn an_invalid_saved_url_leaves_the_route_unusable_and_is_reported_once() {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Route", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Route", &suffix, None)
                .await
                .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings \
             (organization_id, provider, openai_base_url, completions_routed) \
             VALUES ($1, $2, 'https://proxy.example/v1?key=leak', true)",
        )
        .bind(organization.id)
        .bind(PROVIDER_OPENAI)
        .execute(&pool)
        .await
        .expect("the organization's AI settings");
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();

        let guard = tracing::subscriber::set_default(subscriber);
        let route = Route::for_workspace(&state, workspace.id).await;
        let backend = route.backend(&state).await;
        drop(guard);
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        let unusable = Unusable {
            reason: Reason::Url(UrlError::Query),
        };
        assert_eq!(route.endpoint().err(), Some(unusable));
        assert!(
            matches!(backend, Err(backend::Error::Unusable(refused)) if refused == unusable),
            "{backend:?}"
        );
        assert!(!unusable.to_string().contains("leak"));
        let warnings: Vec<String> = captured
            .lines()
            .into_iter()
            .filter(|line| line.contains(INVALID))
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:#?}");
        assert!(
            warnings[0].contains(&workspace.id.to_string()),
            "the warning does not say which workspace saved the URL: {}",
            warnings[0]
        );
    }

    #[tokio::test]
    async fn settings_that_cannot_be_read_leave_the_route_unusable() {
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unreachable")
            .expect("a lazy pool that never connects");
        let state = AppState::new(crate::state::test_config(), pool, None);

        let route = Route::for_workspace(&state, Uuid::new_v4()).await;
        let backend = route.backend(&state).await;

        assert_eq!(
            route.endpoint().err(),
            Some(Unusable {
                reason: Reason::Unreadable
            })
        );
        assert!(
            matches!(backend, Err(backend::Error::Unusable(_))),
            "{backend:?}"
        );
        assert!(
            route
                .clone()
                .on_instance(state.config())
                .endpoint()
                .is_err(),
            "an unusable route was moved onto the instance"
        );
    }

    #[test]
    fn an_unusable_route_says_why_in_words_a_person_can_act_on() {
        for (reason, said) in [
            (
                Reason::Host,
                "This workspace's AI endpoint can't be used: its host isn't one this instance allows endpoints on. Check AI Settings.",
            ),
            (
                Reason::Unreadable,
                "This workspace's AI endpoint can't be used: its AI settings couldn't be read. Check AI Settings.",
            ),
            (
                Reason::Url(UrlError::Scheme),
                "This workspace's AI endpoint can't be used: its saved URL is refused (the URL must use http or https). Check AI Settings.",
            ),
        ] {
            assert_eq!(Unusable { reason }.to_string(), said);
        }
        assert_eq!(Reason::from(UrlError::Unlisted), Reason::Host);
        assert_eq!(
            Reason::from(UrlError::Metadata),
            Reason::Url(UrlError::Metadata)
        );
    }

    #[test]
    fn an_unusable_choice_names_the_chats_provider_and_what_to_do() {
        for (reason, said) in [
            (
                Reason::Unconfigured(Provider::OpenAI),
                "This chat's AI provider, OpenAI, can't be used: no key, endpoint or credentials are saved for it. Choose another provider for this chat, or ask an admin to set it up in AI Settings.",
            ),
            (
                Reason::Refused(Provider::Anthropic, UrlError::Query),
                "This chat's AI provider, Anthropic, can't be used: its saved endpoint is refused (the URL must not include a query). Choose another provider for this chat, or ask an admin to fix it in AI Settings.",
            ),
            (
                Reason::Refused(Provider::SelfHosted, UrlError::Unlisted),
                "This chat's AI provider, Self-Hosted, can't be used: its saved endpoint is refused (the URL's host is not one this instance allows endpoints on). Choose another provider for this chat, or ask an admin to fix it in AI Settings.",
            ),
            (
                Reason::SettingsUnreadable(Provider::Codex),
                "This chat's AI provider, Codex, can't be used: its AI settings couldn't be read. Try again shortly.",
            ),
            (
                Reason::Unsaved(Provider::OpenAI),
                "This chat's AI provider, OpenAI, can't be used: the model this chat chose isn't saved for it in AI Settings. Choose a saved model for this chat, or ask an admin to save this one.",
            ),
            (
                Reason::ChoiceUnreadable,
                "This chat's AI provider can't be used: its choice couldn't be read. Try again shortly.",
            ),
        ] {
            assert_eq!(Unusable { reason }.to_string(), said);
        }
    }

    #[tokio::test]
    async fn a_chat_whose_choice_cannot_be_read_says_so_and_reports_no_choice() {
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unreachable")
            .expect("a lazy pool that never connects");
        let state = AppState::new(crate::state::test_config(), pool, None);
        let chat = crate::services::chat::session::chat_row(None, false, false);

        let route = Route::for_chat(&state, &chat).await;

        assert_eq!(
            route.endpoint().err(),
            Some(Unusable {
                reason: Reason::ChoiceUnreadable
            })
        );
        assert_eq!(
            route.choice(),
            None,
            "an unread choice is not reported as following the workspace"
        );
        assert_eq!(route.chosen(), None);
    }

    /// An organization and its workspace, with the AI settings each saves.
    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        workspace: Uuid,
        state: AppState,
        _agents: TempDir,
    }

    impl Fixture {
        async fn new(provider: &str) -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let suffix = Uuid::new_v4().simple().to_string();
            let organization = organizations::create_organization(&pool, "Chosen", &suffix, None)
                .await
                .expect("an organization")
                .id;
            let workspace =
                workspaces::create_workspace(&pool, organization, "Chosen", &suffix, None)
                    .await
                    .expect("a workspace")
                    .id;
            sqlx::query(
                "INSERT INTO organization_ai_settings (organization_id, provider, completions_routed) \
                 VALUES ($1, $2, true)",
            )
            .bind(organization)
            .bind(provider)
            .execute(&pool)
            .await
            .expect("the organization's AI settings");
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        host_login: false,
                        ..crate::state::test_agents()
                    },
                    ..crate::state::test_config()
                },
                pool.clone(),
                None,
            );
            Self {
                pool,
                organization,
                workspace,
                state,
                _agents: agents,
            }
        }

        /// Saves `url` as the organization's endpoint for `provider`.
        async fn organization(&self, provider: Provider, url: &str) {
            let update = match provider {
                Provider::SelfHosted => {
                    "UPDATE organization_ai_settings SET litellm_host = $2 WHERE organization_id = $1"
                }
                Provider::OpenAI => {
                    "UPDATE organization_ai_settings SET openai_base_url = $2 WHERE organization_id = $1"
                }
                Provider::Anthropic => {
                    "UPDATE organization_ai_settings SET anthropic_base_url = $2 WHERE organization_id = $1"
                }
                provider => panic!("{provider:?} saves no endpoint"),
            };
            sqlx::query(update)
                .bind(self.organization)
                .bind(url)
                .execute(&self.pool)
                .await
                .expect("the organization's endpoint");
        }

        /// Makes `provider` the organization's.
        async fn switch(&self, provider: Provider) {
            sqlx::query(
                "UPDATE organization_ai_settings SET provider = $2 WHERE organization_id = $1",
            )
            .bind(self.organization)
            .bind(provider.as_str())
            .execute(&self.pool)
            .await
            .expect("the organization's provider");
        }

        /// Saves the workspace's own OpenAI endpoint, `url`, and makes OpenAI its provider.
        async fn workspace(&self, url: &str) {
            sqlx::query(
                "INSERT INTO workspace_ai_settings (workspace_id, provider, openai_base_url, completions_routed) \
                 VALUES ($1, $2, $3, true)",
            )
            .bind(self.workspace)
            .bind(PROVIDER_OPENAI)
            .bind(url)
            .execute(&self.pool)
            .await
            .expect("the workspace's AI settings");
        }

        /// A chat of the workspace on `choice`, or following its workspace without one.
        async fn chat(&self, choice: Option<Choice>) -> ChatRow {
            let chat = chats::create_chat(
                &self.pool,
                Some(self.workspace),
                "Chosen",
                "stored-model",
                false,
                true,
            )
            .await
            .expect("a chat");
            chats::provider::set(&self.pool, chat.id, choice.as_ref())
                .await
                .expect("the chat's provider");
            chat
        }

        /// Saves `fast` and `reasoning` as the organization's models.
        async fn models(&self, fast: Option<&str>, reasoning: Option<&str>) {
            sqlx::query(
                "UPDATE organization_ai_settings SET model_fast = $2, model_reasoning = $3 \
                 WHERE organization_id = $1",
            )
            .bind(self.organization)
            .bind(fast)
            .bind(reasoning)
            .execute(&self.pool)
            .await
            .expect("the organization's models");
        }

        async fn route(&self, chat: &ChatRow) -> Route {
            Route::for_chat(&self.state, chat).await
        }

        /// A signed-in login of `agent`, with half its usage left.
        async fn login(&self, agent: AgentKind) -> Uuid {
            let login = agent_logins::insert(
                &self.pool,
                &agent_logins::Insert {
                    organization_id: self.organization,
                    agent: agent.as_str(),
                    account: Some(agent.as_str()),
                    credential: None,
                    label: Some(agent.as_str()),
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login")
            .id;
            agent_logins::observe(
                &self.pool,
                login,
                &Snapshot {
                    windows: Vec::new(),
                    headroom: Some(50.0),
                    fetched_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("the login's usage");
            login
        }

        async fn remove(&self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
        }
    }

    fn choice(provider: Provider, model: Option<&str>) -> Option<Choice> {
        Some(Choice {
            provider,
            model: model.map(str::to_string),
        })
    }

    fn url(route: &Route) -> String {
        route
            .endpoint()
            .map(|endpoint| endpoint.url().to_string())
            .unwrap_or_else(|unusable| panic!("an unusable route: {unusable}"))
    }

    fn provider(route: &Route) -> Option<&str> {
        route.settings().map(|settings| settings.provider.as_str())
    }

    #[tokio::test]
    async fn a_chats_choice_comes_before_its_workspaces_provider_and_its_organizations() {
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture
            .organization(Provider::SelfHosted, "http://organization.example:4000")
            .await;
        fixture
            .organization(Provider::Anthropic, "http://anthropic.example")
            .await;
        fixture.workspace("http://workspace.example/v1").await;
        let following = fixture.chat(None).await;
        let anthropic = fixture
            .chat(choice(Provider::Anthropic, Some("claude-chosen")))
            .await;
        let organizations = fixture.chat(choice(Provider::SelfHosted, None)).await;

        let workspace = fixture.route(&following).await;
        let chosen = fixture.route(&anthropic).await;
        let own = fixture.route(&organizations).await;
        fixture.remove().await;

        assert_eq!(workspace.chosen(), None);
        assert_eq!(workspace.choice(), Some(None));
        assert_eq!(provider(&workspace), Some(PROVIDER_OPENAI));
        assert!(url(&workspace).starts_with("http://workspace.example"));
        assert_eq!(workspace.requested("stored-model"), "stored-model");

        assert_eq!(
            chosen.chosen(),
            choice(Provider::Anthropic, Some("claude-chosen")).as_ref()
        );
        assert_eq!(
            chosen.choice(),
            Some(choice(Provider::Anthropic, Some("claude-chosen")).as_ref())
        );
        assert_eq!(provider(&chosen), Some(PROVIDER_ANTHROPIC));
        assert!(
            url(&chosen).starts_with("http://anthropic.example"),
            "{}",
            url(&chosen)
        );
        assert_eq!(chosen.requested("stored-model"), "claude-chosen");

        assert_eq!(provider(&own), Some(PROVIDER_SELF_HOSTED));
        assert!(
            url(&own).starts_with("http://organization.example:4000"),
            "{}",
            url(&own)
        );
        assert_eq!(
            own.requested("stored-model"),
            AUTO,
            "a choice without a model leaves the provider to pick, never the stored model"
        );
    }

    #[tokio::test]
    async fn a_chosen_provider_drops_the_window_kept_for_the_workspaces_provider() {
        const WINDOW: i64 = 8192;
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture
            .organization(Provider::Anthropic, "http://anthropic.example")
            .await;
        fixture.workspace("http://workspace.example/v1").await;
        let following = ChatRow {
            context_tokens: Some(WINDOW),
            ..fixture.chat(None).await
        };
        let anthropic = ChatRow {
            context_tokens: Some(WINDOW),
            ..fixture
                .chat(choice(Provider::Anthropic, Some("claude-chosen")))
                .await
        };

        let workspace = fixture.route(&following).await;
        let chosen = fixture.route(&anthropic).await;
        fixture.remove().await;

        let kept = workspace.apply(following);
        assert_eq!(kept.model_name, "stored-model");
        assert_eq!(kept.context_tokens, Some(WINDOW));

        let applied = chosen.apply(anthropic);
        assert_eq!(applied.model_name, "claude-chosen");
        assert_eq!(
            applied.context_tokens, None,
            "a chosen provider was held to the window kept for the workspace's provider"
        );
    }

    #[tokio::test]
    async fn the_same_self_hosted_endpoint_chosen_by_name_keeps_the_window() {
        const WINDOW: i64 = 8192;
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture
            .organization(Provider::SelfHosted, "http://self-hosted.example:4000")
            .await;
        let named = ChatRow {
            context_tokens: Some(WINDOW),
            ..fixture
                .chat(choice(Provider::SelfHosted, Some("qwen3:8b")))
                .await
        };

        let route = fixture.route(&named).await;
        fixture.remove().await;

        let applied = route.apply(named);
        assert_eq!(applied.model_name, "qwen3:8b");
        assert_eq!(
            applied.context_tokens,
            Some(WINDOW),
            "a chat that chose its workspace's own self-hosted endpoint lost the window it keeps there"
        );
    }

    #[tokio::test]
    async fn a_chat_that_follows_its_workspace_follows_a_later_change_and_a_chosen_one_stays() {
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture
            .organization(Provider::SelfHosted, "http://before.example:4000")
            .await;
        let following = fixture.chat(None).await;
        let pinned = fixture.chat(choice(Provider::SelfHosted, None)).await;

        let before = url(&fixture.route(&following).await);
        fixture.switch(Provider::OpenAI).await;
        fixture
            .organization(Provider::OpenAI, "http://after.example/v1")
            .await;
        let after = fixture.route(&following).await;
        let kept = fixture.route(&pinned).await;
        fixture.remove().await;

        assert!(before.starts_with("http://before.example:4000"), "{before}");
        assert_eq!(provider(&after), Some(PROVIDER_OPENAI));
        assert!(
            url(&after).starts_with("http://after.example"),
            "{}",
            url(&after)
        );
        assert_eq!(provider(&kept), Some(PROVIDER_SELF_HOSTED));
        assert!(
            url(&kept).starts_with("http://before.example:4000"),
            "{}",
            url(&kept)
        );
    }

    #[tokio::test]
    async fn a_chosen_model_the_settings_stop_saving_fails_the_chat_instead_of_switching() {
        let fixture = Fixture::new(PROVIDER_OPENAI).await;
        fixture
            .organization(Provider::OpenAI, "http://openai.example/v1")
            .await;
        fixture
            .organization(Provider::Anthropic, "http://anthropic.example")
            .await;
        fixture.models(Some("gpt-4o"), None).await;
        let openai = fixture.chat(choice(Provider::OpenAI, Some("gpt-4o"))).await;
        let anthropic = fixture
            .chat(choice(Provider::Anthropic, Some("claude-unsaved")))
            .await;

        let before = fixture.route(&openai).await;
        fixture.models(Some("gpt-4o-mini"), None).await;
        let after = fixture.route(&openai).await;
        let turn = after.chat(&fixture.state, &[], None, None).await;
        let backend = after.backend(&fixture.state).await;
        let named = fixture.route(&anthropic).await;
        fixture.remove().await;

        assert!(before.preferences("").admits("gpt-4o"), "{}", url(&before));

        let settings = after.settings().expect("the chosen provider's settings");
        let endpoint =
            Endpoint::try_resolve(fixture.state.config(), settings).expect("OpenAI's endpoint");
        let saved = Preferences::for_endpoint(settings, "", &endpoint);
        assert_eq!(
            saved.completions(),
            ["gpt-4o-mini"],
            "the chat's choice stood in for the admin's saved models"
        );
        assert!(!saved.admits("gpt-4o"));
        assert_eq!(
            after.chosen(),
            choice(Provider::OpenAI, Some("gpt-4o")).as_ref()
        );
        let unusable = after
            .endpoint()
            .expect_err("an unsaved model fails the chat");
        assert_eq!(unusable.reason, Reason::Unsaved(Provider::OpenAI));
        assert_eq!(
            unusable.to_string(),
            "This chat's AI provider, OpenAI, can't be used: the model this chat chose isn't saved for it in AI Settings. Choose a saved model for this chat, or ask an admin to save this one."
        );
        assert!(
            matches!(turn, Err(backend::Error::Unusable(_))),
            "the turn ran on another model"
        );
        assert!(matches!(backend, Err(backend::Error::Unusable(_))));

        assert!(
            named.endpoint().is_ok(),
            "a provider with nothing saved runs the model the chat names"
        );
        assert!(named.preferences("").admits("claude-unsaved"));
    }

    #[tokio::test]
    async fn a_chosen_provider_whose_workspace_is_gone_fails_rather_than_run_on_the_instance() {
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        let chat = fixture
            .chat(choice(Provider::OpenAI, Some("gpt-chosen")))
            .await;
        let orphaned = ChatRow {
            workspace_id: Some(Uuid::new_v4()),
            ..chat
        };

        let route = fixture.route(&orphaned).await;
        fixture.remove().await;

        assert_eq!(
            route.endpoint().err(),
            Some(Unusable {
                reason: Reason::SettingsUnreadable(Provider::OpenAI)
            }),
            "a chosen provider was dropped for the instance"
        );
        assert_eq!(
            route.chosen(),
            choice(Provider::OpenAI, Some("gpt-chosen")).as_ref()
        );
    }

    #[tokio::test]
    async fn an_unusable_choice_fails_its_completions_and_never_reaches_the_instance() {
        let fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture
            .organization(Provider::Anthropic, "https://anthropic.example/v1?key=leak")
            .await;
        let unconfigured = fixture
            .chat(choice(Provider::OpenAI, Some("gpt-chosen")))
            .await;
        let refused = fixture.chat(choice(Provider::Anthropic, None)).await;
        let following = fixture.chat(None).await;

        let mut routes = Vec::new();
        for chat in [&unconfigured, &refused] {
            let route = fixture.route(chat).await;
            let backend = route.backend(&fixture.state).await;
            let classified = route.backend_for(&fixture.state, None).await;
            let turn = route.chat(&fixture.state, &[], None, None).await;
            routes.push((route, backend, classified, turn));
        }
        let workspace = fixture.route(&following).await;
        fixture.remove().await;

        let said: Vec<String> = routes
            .iter()
            .map(|(route, ..)| {
                route
                    .endpoint()
                    .expect_err("an unusable choice")
                    .to_string()
            })
            .collect();
        assert_eq!(
            said,
            [
                "This chat's AI provider, OpenAI, can't be used: no key, endpoint or credentials are saved for it. Choose another provider for this chat, or ask an admin to set it up in AI Settings.",
                "This chat's AI provider, Anthropic, can't be used: its saved endpoint is refused (the URL must not include a query). Choose another provider for this chat, or ask an admin to fix it in AI Settings.",
            ]
        );
        assert!(!said[1].contains("leak"));
        for (route, backend, classified, turn) in &routes {
            assert!(
                matches!(backend, Err(backend::Error::Unusable(_))),
                "{backend:?}"
            );
            assert!(
                matches!(classified, Err(backend::Error::Unusable(_))),
                "{classified:?}"
            );
            assert!(
                matches!(turn, Err(backend::Error::Unusable(_))),
                "the turn ran"
            );
            assert!(
                route
                    .clone()
                    .on_instance(fixture.state.config())
                    .endpoint()
                    .is_err(),
                "an unusable choice was moved onto the instance"
            );
        }
        assert_eq!(
            workspace.endpoint().map(Endpoint::origin).ok(),
            Some(crate::services::endpoint::Origin::Instance),
            "the workspace itself still runs on the instance"
        );
    }

    #[tokio::test]
    async fn a_chosen_agent_runs_on_its_own_logins_or_not_at_all() {
        let fixture = Fixture::new(PROVIDER_CLAUDE_CODE).await;
        let codex = fixture.login(AgentKind::Codex).await;
        let chosen = fixture.chat(choice(Provider::ClaudeCode, None)).await;
        let following = fixture.chat(None).await;

        let route = fixture.route(&chosen).await;
        let turn = route
            .chat(&fixture.state, &[], None, None)
            .await
            .map(|resolved| resolved.backend);
        let classifier = route.backend_for(&fixture.state, Some(codex)).await;
        let title = route.backend(&fixture.state).await;
        let workspace = fixture
            .route(&following)
            .await
            .backend(&fixture.state)
            .await;
        fixture.remove().await;

        for (completion, failed) in [("turn", turn), ("classifier", classifier), ("title", title)] {
            assert!(
                matches!(
                    failed,
                    Err(backend::Error::SignedOut {
                        agent: AgentKind::Claude
                    })
                ),
                "the {completion} of a chat that chose Claude Code did not fail signed out: {failed:?}"
            );
        }
        assert!(
            matches!(
                workspace,
                Ok(LlmBackend::Cli {
                    agent: AgentKind::Codex,
                    ..
                })
            ),
            "a chat following its workspace lost the fallback to another agent: {workspace:?}"
        );
    }

    #[tokio::test]
    async fn a_chosen_agent_without_a_login_runs_only_under_its_own_host_sign_in() {
        let mut fixture = Fixture::new(PROVIDER_SELF_HOSTED).await;
        fixture.login(AgentKind::Codex).await;
        let chosen = fixture.chat(choice(Provider::ClaudeCode, None)).await;
        fixture.state = AppState::new(
            Config {
                agents: AgentConfig {
                    host_login: true,
                    ..fixture.state.config().agents.clone()
                },
                ..crate::state::test_config()
            },
            fixture.pool.clone(),
            None,
        );

        let backend = fixture.route(&chosen).await.backend(&fixture.state).await;
        fixture.remove().await;

        let Ok(LlmBackend::Cli { agent, sign_in, .. }) = backend else {
            panic!("expected the host's Claude sign-in, got {backend:?}");
        };
        assert_eq!(agent, AgentKind::Claude);
        assert_eq!(sign_in, SignIn::Host);
    }

    fn agent(
        config: &Config,
        settings: &EffectiveAiSettings,
        choice: Option<&Choice>,
    ) -> Option<AgentKind> {
        Route::of(
            config,
            Uuid::nil(),
            Uuid::nil(),
            settings.clone(),
            choice.cloned(),
        )
        .agent(config)
    }

    fn instance_on(agent: Option<AgentKind>) -> Config {
        let mut config = crate::state::test_config();
        config.model_backend = match agent {
            Some(agent) => ModelBackend::Cli {
                agent,
                executable: None,
            },
            None => ModelBackend::LiteLlm,
        };
        config
    }

    fn openai_at(url: &str) -> EffectiveAiSettings {
        let mut settings = settings(PROVIDER_OPENAI);
        settings.openai_base_url = Some(url.to_string());
        settings
    }

    #[test]
    fn a_coding_agent_provider_prefers_that_agent() {
        let settings = settings(PROVIDER_CLAUDE_CODE);

        assert_eq!(
            agent(&instance_on(None), &settings, None),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            agent(&instance_on(Some(AgentKind::Codex)), &settings, None),
            Some(AgentKind::Claude),
            "the instance's own agent never stands in for the one chosen"
        );
    }

    #[test]
    fn a_provider_on_the_instance_runs_its_turns_on_the_instance_backend() {
        let settings = settings(PROVIDER_SELF_HOSTED);

        assert_eq!(agent(&instance_on(None), &settings, None), None);
        assert_eq!(
            agent(&instance_on(Some(AgentKind::Codex)), &settings, None),
            Some(AgentKind::Codex)
        );
    }

    #[test]
    fn a_saved_endpoint_runs_its_turns_over_http_whatever_the_instance_runs() {
        let settings = openai_at("https://api.openai.com/v1");

        assert_eq!(
            agent(&instance_on(Some(AgentKind::Claude)), &settings, None),
            None
        );
    }

    #[test]
    fn a_chat_choice_decides_the_agent_over_its_workspace_default() {
        let config = instance_on(None);
        let http = openai_at("https://api.openai.com/v1");
        let claude = settings(PROVIDER_CLAUDE_CODE);

        assert_eq!(
            agent(
                &config,
                &http,
                Some(&Choice {
                    provider: Provider::Codex,
                    model: None,
                })
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            agent(
                &config,
                &claude,
                Some(&Choice {
                    provider: Provider::SelfHosted,
                    model: None,
                })
            ),
            None
        );
    }

    #[test]
    fn a_choice_that_cannot_run_names_no_agent_even_on_an_agent_instance() {
        let config = instance_on(Some(AgentKind::Claude));
        let unconfigured = Choice {
            provider: Provider::OpenAI,
            model: None,
        };
        assert_eq!(
            agent(
                &config,
                &settings(PROVIDER_SELF_HOSTED),
                Some(&unconfigured)
            ),
            None,
            "nothing is saved for the chosen provider, so its turns fail"
        );

        let mut saved = openai_at("https://api.openai.com/v1");
        saved.model_fast = Some("gpt-4o".to_string());
        let unsaved = Choice {
            provider: Provider::OpenAI,
            model: Some("gpt-3.5".to_string()),
        };
        assert_eq!(
            agent(&config, &saved, Some(&unsaved)),
            None,
            "the chosen model is no longer saved, so its turns fail"
        );
    }

    #[test]
    fn an_endpoint_that_cannot_be_used_runs_no_agent() {
        let settings = openai_at("https://proxy.example/v1?key=leak");

        assert_eq!(
            agent(&instance_on(Some(AgentKind::Claude)), &settings, None),
            None
        );
    }
}
