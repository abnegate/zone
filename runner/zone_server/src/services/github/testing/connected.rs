//! A workspace whose GitHub, and the App that mints its installation tokens,
//! is a stand-in, shared by the tests of everything that reaches GitHub
//! through a source.

use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::MockServer;

use super::OWNER;
use super::REPOSITORY;
use crate::db::github_apps::testing::pool;
use crate::db::github_apps::testing::seed_organization;
use crate::db::github_apps::testing::seed_user;
use crate::db::github_apps::testing::seed_workspace;
use crate::db::github_installations;
use crate::db::sources;
use crate::db::sources::NewSource;
use crate::db::sources::SourceChanges;
use crate::db::sources::SourceRow;
use crate::db::workspace_members;
use crate::db::workspace_members::WorkspaceRole;
use crate::services::github_app::testing::installed;
use crate::services::github_app::testing::provider;
use crate::state::AppState;
use crate::state::test_config;

pub(crate) struct Connected {
    pub(crate) pool: PgPool,
    pub(crate) github: MockServer,
    pub(crate) state: AppState,
    pub(crate) organization: Uuid,
    pub(crate) workspace: Uuid,
    pub(crate) user: Uuid,
}

impl Connected {
    /// A workspace the user administers, whose server points at `github`.
    pub(crate) async fn new() -> Self {
        let pool = pool().await;
        let github = MockServer::start().await;
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;
        let user = seed_user(&pool).await;
        workspace_members::add_member(&pool, workspace, user, WorkspaceRole::Admin, None)
            .await
            .expect("the user administers the workspace");
        let mut config = test_config();
        config.github_api_url = github.uri();
        config.github_api_allows_http = true;
        let state = AppState::new(config, pool.clone(), None);
        state.install_github(provider(&github, std::sync::Arc::clone(state.master_key())));
        Self {
            pool,
            github,
            state,
            organization,
            workspace,
            user,
        }
    }

    /// An active installation of the organization's App, numbered `number` on GitHub.
    pub(crate) async fn installation(&self, number: u64) -> Uuid {
        installed(&self.pool, self.state.github(), self.organization, number).await
    }

    /// A source connected through installation `number`, holding `token`,
    /// whose installation GitHub has since removed.
    pub(crate) async fn revoked_source(&self, number: u64, token: Option<&str>) -> SourceRow {
        let installation = self.installation(number).await;
        let source = self.source(Some(installation), token).await;
        github_installations::mark_revoked(&self.pool, installation)
            .await
            .expect("revoke the installation");
        source
    }

    /// A GitHub source on `owner/repository`'s main branch, connected through
    /// `installation`, holding `token`, or both.
    pub(crate) async fn source(
        &self,
        installation: Option<Uuid>,
        token: Option<&str>,
    ) -> SourceRow {
        let sealed =
            token.map(|token| crate::secret::fixture::seal(token, self.state.master_key()));
        self.created(
            sources::GITHUB,
            json!({"owner": OWNER, "repo": REPOSITORY, "branch": "main"}),
            installation,
            sealed.as_deref(),
        )
        .await
    }

    /// A source of `kind` holding `configuration`, with no credential.
    pub(crate) async fn source_of(&self, kind: &str, configuration: Value) -> SourceRow {
        self.created(kind, configuration, None, None).await
    }

    pub(crate) async fn deactivate(&self, source: &SourceRow) {
        sources::update_source(
            &self.pool,
            source.id,
            self.workspace,
            SourceChanges {
                is_active: Some(false),
                ..SourceChanges::default()
            },
        )
        .await
        .expect("deactivate the source");
    }

    async fn created(
        &self,
        kind: &str,
        configuration: Value,
        installation: Option<Uuid>,
        credential: Option<&str>,
    ) -> SourceRow {
        let name = format!("Source {}", Uuid::new_v4());
        sources::create_source(
            &self.pool,
            NewSource {
                workspace_id: self.workspace,
                name: &name,
                source_type: kind,
                config: configuration,
                description: None,
                url: None,
                credentials_encrypted: credential,
                github_installation_id: installation,
            },
        )
        .await
        .expect("create the source")
    }
}
