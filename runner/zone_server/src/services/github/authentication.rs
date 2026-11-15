mod failure;

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::ApplicationError;

use super::client::Github;
use super::client::SERVER_ERRORS;
use super::configuration::Configuration;
use crate::db::sources::SourceRow;
use crate::services::github_app::Credential;
use crate::services::github_app::Error as GitHubError;
use crate::services::github_app::Unauthenticated;
use crate::state::AppState;
use failure::Failure;

pub(crate) const SOURCE_DISCONNECTED: &str =
    "This source's GitHub connection was removed or suspended. Reconnect GitHub.";
const SOURCE_CREDENTIALS_INVALID: &str = "The source credentials could not be decrypted.";
pub(crate) const SOURCE_UNREACHABLE: &str = "GitHub could not be reached.";
const SOURCE_CREDENTIALS_UNRESOLVED: &str = "The source's GitHub credential could not be resolved.";

/// The bearer token `source` reaches GitHub with: a token minted for its
/// GitHub App installation, or the personal access token stored on a source
/// connected before installations existed. A removed or suspended
/// installation is refused outright rather than answered with a stored token.
pub(crate) async fn source_token(
    state: &AppState,
    source: &SourceRow,
) -> Result<Option<SecretValue>, &'static str> {
    Ok(source_credential(state, source)
        .await?
        .map(Credential::into_secret))
}

async fn source_credential(
    state: &AppState,
    source: &SourceRow,
) -> Result<Option<Credential>, &'static str> {
    state
        .github()
        .source(state.db(), source)
        .await
        .inspect_err(|error| {
            tracing::warn!(%error, source = %source.id, "GitHub source credential could not be resolved");
        })
        .map_err(|error| credential_message(&error))
}

/// Runs `operation` on a client for `configuration` that holds `source`'s
/// credential. A minted token GitHub refuses is minted again and the
/// operation run once more with it.
pub(crate) async fn authenticated<T, F, Fut>(
    state: &AppState,
    source: &SourceRow,
    configuration: Configuration,
    operation: F,
) -> Result<T, String>
where
    F: Fn(Github) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let operation = &operation;
    credentialed(state, source, configuration, |github| async move {
        operation(github).await.map_err(Failure)
    })
    .await
    .map_err(|Failure(message)| message)
}

pub(crate) async fn credentialed<T, E, F, Fut>(
    state: &AppState,
    source: &SourceRow,
    configuration: Configuration,
    operation: F,
) -> Result<T, E>
where
    F: Fn(Github) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Unauthenticated + From<GitHubError> + From<String>,
{
    let client = |token: Option<SecretValue>| {
        Github::new(
            Configuration {
                token,
                ..configuration.clone()
            },
            state.config(),
        )
        .map_err(E::from)
    };
    let credential = source_credential(state, source)
        .await
        .map_err(|message| E::from(message.to_string()))?;
    let Some(credential) = credential else {
        return operation(client(None)?).await;
    };
    let (client, operation) = (&client, &operation);
    state
        .github()
        .retrying(state.db(), credential, |token| async move {
            operation(client(Some(token))?).await
        })
        .await
}

pub(crate) fn credential_message(error: &GitHubError) -> &'static str {
    match error {
        GitHubError::Revoked | GitHubError::Suspended | GitHubError::NotConnected => {
            SOURCE_DISCONNECTED
        }
        GitHubError::Credential | GitHubError::Unsealing => SOURCE_CREDENTIALS_INVALID,
        GitHubError::Application(ApplicationError::Transport(_)) => SOURCE_UNREACHABLE,
        GitHubError::Application(ApplicationError::Status(status))
            if SERVER_ERRORS.contains(status) =>
        {
            SOURCE_UNREACHABLE
        }
        _ => SOURCE_CREDENTIALS_UNRESOLVED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_failure_is_refused_for_what_it_is() {
        for (error, expected) in [
            (GitHubError::Revoked, SOURCE_DISCONNECTED),
            (GitHubError::Suspended, SOURCE_DISCONNECTED),
            (GitHubError::NotConnected, SOURCE_DISCONNECTED),
            (GitHubError::Credential, SOURCE_CREDENTIALS_INVALID),
            (GitHubError::Unsealing, SOURCE_CREDENTIALS_INVALID),
            (
                GitHubError::Application(ApplicationError::Transport("reset".into())),
                SOURCE_UNREACHABLE,
            ),
            (
                GitHubError::Application(ApplicationError::Status(500)),
                SOURCE_UNREACHABLE,
            ),
            (
                GitHubError::Application(ApplicationError::Status(503)),
                SOURCE_UNREACHABLE,
            ),
            (
                GitHubError::Application(ApplicationError::Status(401)),
                SOURCE_CREDENTIALS_UNRESOLVED,
            ),
            (
                GitHubError::Application(ApplicationError::PrivateKey),
                SOURCE_CREDENTIALS_UNRESOLVED,
            ),
            (GitHubError::Sealing, SOURCE_CREDENTIALS_UNRESOLVED),
        ] {
            assert_eq!(credential_message(&error), expected, "{error:?}");
        }
    }
}
