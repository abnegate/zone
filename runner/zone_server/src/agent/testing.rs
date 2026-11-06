//! A workspace whose GitHub, and the App that mints its installation tokens,
//! is a stand-in, shared by the tests of the tools that reach GitHub through a source.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::MockServer;

use super::WorkspaceScope;
use crate::db::github_apps::tests::{pool, seed_organization, seed_user, seed_workspace};
use crate::db::github_installations;
use crate::db::sources::{self, NewSource, SourceRow};
use crate::db::workspace_members::{self, WorkspaceRole};
use crate::services::github_app::testing::{installed, provider};
use crate::state::{AppState, test_config};

pub(crate) const OWNER: &str = "owner";
pub(crate) const REPOSITORY: &str = "repository";
pub(crate) const MINTED_TOKEN: &str = "ghs_mintedinstallationtoken";
pub(crate) const STORED_TOKEN: &str = "ghp_storedpersonaltoken";

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
        state.install_github(provider(&github, *state.encryption_key()));
        Self {
            pool,
            github,
            state,
            organization,
            workspace,
            user,
        }
    }

    pub(crate) fn scope(&self) -> WorkspaceScope {
        WorkspaceScope {
            state: self.state.clone(),
            workspace_id: self.workspace,
            chat_id: None,
            user_id: self.user,
            offline: false,
        }
    }

    /// An active installation of the organization's App, numbered `number` on GitHub.
    pub(crate) async fn installation(&self, number: u64) -> Uuid {
        installed(&self.pool, self.state.github(), self.organization, number).await
    }

    /// An installation GitHub has since removed.
    pub(crate) async fn revoked(&self, number: u64) -> Uuid {
        let installation = self.installation(number).await;
        github_installations::mark_revoked(&self.pool, installation)
            .await
            .expect("revoke the installation");
        installation
    }

    /// A GitHub source on `owner/repository`'s main branch, connected through
    /// `installation`, holding `token`, or both.
    pub(crate) async fn source(
        &self,
        installation: Option<Uuid>,
        token: Option<&str>,
    ) -> SourceRow {
        let sealed = token.map(|token| {
            crate::crypto::encrypt(self.state.encryption_key(), token).expect("seal the token")
        });
        let name = format!("GitHub {}", Uuid::new_v4());
        sources::create_source(
            &self.pool,
            NewSource {
                workspace_id: self.workspace,
                name: &name,
                source_type: sources::GITHUB,
                config: json!({"owner": OWNER, "repo": REPOSITORY, "branch": "main"}),
                description: None,
                url: None,
                credentials_encrypted: sealed.as_deref(),
                github_installation_id: installation,
            },
        )
        .await
        .expect("create the source")
    }
}

/// A GitHub installation number no other test uses.
pub(crate) fn number() -> u64 {
    rand::random_range(1..i64::MAX as u64)
}
