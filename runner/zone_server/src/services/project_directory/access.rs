use abnegate_secret::SecretValue;

use crate::db::projects::ProjectRow;
use crate::db::sources::SourceRow;
use crate::services::github_app::Credential;
use crate::state::AppState;

/// Whose credential a clone presents. It is resolved only when the clone is made: a GitHub App
/// installation mints its token from GitHub, which a checkout that already stands never needs
/// and an offline chat must never ask for.
pub enum Access {
    Source(Box<SourceRow>),
    Project(Box<ProjectRow>),
}

impl Access {
    /// The token to clone with. One that cannot be resolved is not presented, and the clone then
    /// fails or succeeds on what the repository allows anyone.
    pub async fn token(&self, state: &AppState) -> Option<SecretValue> {
        let resolved = match self {
            Self::Source(source) => state
                .github()
                .source(state.db(), source)
                .await
                .inspect_err(|error| {
                    tracing::warn!(source = %source.id, %error, "Could not resolve the source's credential");
                }),
            Self::Project(project) => state
                .github()
                .project(state.db(), project)
                .await
                .inspect_err(|error| {
                    tracing::warn!(project = %project.id, %error, "Could not resolve the project's credential");
                }),
        };
        resolved.ok().flatten().map(Credential::into_secret)
    }
}
