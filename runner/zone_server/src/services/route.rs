//! Where a workspace's completions go, read from its AI settings once.

use std::fmt;

use uuid::Uuid;
use zone_core::llm::{AgentKind, LlmBackend};

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings};
use crate::db::workspaces;
use crate::services::backend;
use crate::services::endpoint::{Endpoint, UrlError};
use crate::services::stages::Preferences;
use crate::state::AppState;

/// Why a workspace's saved endpoint cannot take its completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The saved URL fails [`crate::services::endpoint::validate_url`].
    Url(UrlError),
    /// The URL completions would go to, saved or a provider's default, is on a
    /// host `ZONE_ENDPOINT_HOSTS` does not list.
    Host,
    /// The workspace or its AI settings could not be read.
    Unreadable,
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
                let detail = error.to_string();
                let detail = detail.trim_end_matches('.');
                let mut characters = detail.chars();
                let first = characters.next().map(|first| first.to_ascii_lowercase());
                write!(formatter, "its saved URL is refused (")?;
                if let Some(first) = first {
                    write!(formatter, "{first}{}", characters.as_str())?;
                }
                write!(formatter, ")")
            }
            Self::Host => {
                formatter.write_str("its host isn't one this instance allows endpoints on")
            }
            Self::Unreadable => formatter.write_str("its AI settings couldn't be read"),
        }
    }
}

/// A saved endpoint whose completions fail rather than go anywhere its
/// organization did not choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("This workspace's AI endpoint can't be used: {reason}. Check AI Settings.")]
pub struct Unusable {
    pub reason: Reason,
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
#[derive(Clone)]
pub struct Route {
    endpoint: Result<Endpoint, Unusable>,
    saved: Option<Saved>,
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
        }
    }

    pub fn instance(config: &Config) -> Self {
        Self {
            endpoint: Ok(Endpoint::instance(config)),
            saved: None,
        }
    }

    fn unusable(reason: Reason) -> Self {
        Self {
            endpoint: Err(reason.into()),
            saved: None,
        }
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

    /// The organization whose settings run its workspace on `agent`.
    pub fn organization_on(&self, agent: AgentKind) -> Option<Uuid> {
        self.saved
            .as_ref()
            .filter(|saved| saved.settings.agent() == Some(agent))
            .map(|saved| saved.organization)
    }

    /// The backend these settings choose, resolved when a completion needs
    /// one: a coding agent's sign-in is read only then. An unusable route has
    /// none.
    pub async fn backend(&self, state: &AppState) -> Result<LlmBackend, backend::Error> {
        let endpoint = self.endpoint()?;
        match &self.saved {
            Some(saved) => {
                backend::for_settings(
                    state,
                    saved.organization,
                    &saved.settings,
                    endpoint.origin(),
                )
                .await
            }
            None => Ok(backend::instance(state.config())),
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
async fn saved(
    state: &AppState,
    workspace: Uuid,
) -> Result<Option<(Uuid, EffectiveAiSettings)>, Reason> {
    #[cfg(test)]
    reads::record(workspace);
    let organization = match workspaces::get_workspace(state.db(), workspace).await {
        Ok(Some(row)) => row.organization_id,
        Ok(None) => {
            tracing::warn!(%workspace, "No such workspace; using the instance's endpoint");
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
    use zone_context::embeddings::providers::PROVIDER_OPENAI;

    use super::*;
    use crate::db::organizations;

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
}
