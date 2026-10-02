//! The home each login's agent keeps its state in, beside the working directory every login of
//! that agent shares.

use std::fs;
use std::io;
use std::path::PathBuf;

use zone_core::llm::AgentKind;

use super::codex::{self, CREDENTIALS};
use super::devices;
use super::error::Error;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::state::AppState;

/// The login's home, created when it is missing.
///
/// Before logins had homes of their own, codex kept an organization's one login in
/// `<state>/<organization>/codex/auth.json`. While the organization holds a single codex login,
/// that file is moved into the login's home the first time it is needed; a second login could be
/// either account, so with one the file is left where it is. Only a regular file is moved, never a
/// link.
///
/// Takes the organization's sign-in lock for a codex login, so it must never be called holding it.
pub async fn adopt(state: &AppState, login: &AgentLoginRow) -> Result<PathBuf, Error> {
    let agent = agent(login)?;
    if agent != AgentKind::Codex {
        return create(state, login, agent);
    }
    let _guard = devices::lock(login.organization_id).await;
    adopt_held(state, login).await
}

/// [`adopt`], for a caller that already holds the organization's sign-in lock.
pub(super) async fn adopt_held(state: &AppState, login: &AgentLoginRow) -> Result<PathBuf, Error> {
    let agent = agent(login)?;
    let home = create(state, login, agent)?;
    if agent != AgentKind::Codex {
        return Ok(home);
    }

    let organization = login.organization_id;
    let agents = &state.config().agents;
    let root = agents.home(organization, agent);
    if !codex::signed_in(&root) || codex::signed_in(&home) {
        return Ok(home);
    }
    let logins = agent_logins::list_for(state.db(), organization, agent.as_str()).await?;
    if !matches!(logins.as_slice(), [only] if only.id == login.id) {
        return Ok(home);
    }
    match fs::rename(root.join(CREDENTIALS), home.join(CREDENTIALS)) {
        Ok(()) => Ok(home),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(home),
        Err(error) => Err(Error::Internal(format!(
            "Could not move the organization's codex login into its home: {error}"
        ))),
    }
}

fn agent(login: &AgentLoginRow) -> Result<AgentKind, Error> {
    AgentKind::named(&login.agent)
        .ok_or_else(|| Error::Internal(format!("{} is not an agent Zone drives", login.agent)))
}

fn create(state: &AppState, login: &AgentLoginRow, agent: AgentKind) -> Result<PathBuf, Error> {
    state
        .config()
        .agents
        .create_login_home(login.organization_id, agent, login.id)
        .map_err(|error| Error::Internal(format!("Could not prepare the login's home: {error}")))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use sqlx::PgPool;
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::config::{AgentConfig, Config};
    use crate::db::agent_logins::Insert;

    const LEGACY: &str = r#"{"tokens":"legacy"}"#;

    struct Scene {
        pool: PgPool,
        state: AppState,
        organization: Uuid,
        directory: TempDir,
    }

    impl Scene {
        async fn new() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated test database"),
            )
            .await
            .expect("the test database accepts connections");
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Login homes', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to sign in");
            let directory = TempDir::new().expect("a temporary directory");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: directory.path().join("agents"),
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
                state,
                organization,
                directory,
            }
        }

        async fn sign_in(&self, agent: AgentKind) -> AgentLoginRow {
            agent_logins::insert(
                &self.pool,
                &Insert {
                    organization_id: self.organization,
                    agent: agent.as_str(),
                    account: None,
                    credential: None,
                    label: None,
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login")
        }

        fn root(&self) -> PathBuf {
            self.state
                .config()
                .agents
                .create_home(self.organization, AgentKind::Codex)
                .expect("the organization's codex root")
        }

        fn legacy(&self) -> PathBuf {
            let legacy = self.root().join(CREDENTIALS);
            fs::write(&legacy, LEGACY).expect("a codex login from before login homes");
            legacy
        }

        async fn adopt(&self, login: &AgentLoginRow) -> PathBuf {
            adopt(&self.state, login).await.expect("the login's home")
        }

        async fn remove(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization can be deleted");
        }
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("the file is readable")
    }

    #[tokio::test]
    async fn a_legacy_codex_login_is_moved_into_the_only_logins_home_once() {
        let scene = Scene::new().await;
        let login = scene.sign_in(AgentKind::Codex).await;
        let legacy = scene.legacy();

        let home = scene.adopt(&login).await;

        assert_eq!(
            home,
            scene
                .state
                .config()
                .agents
                .login_home(scene.organization, AgentKind::Codex, login.id)
        );
        assert_eq!(read(&home.join(CREDENTIALS)), LEGACY);
        assert!(!legacy.exists(), "the legacy login was copied, not moved");

        fs::write(home.join(CREDENTIALS), "renewed").expect("codex renews its login");
        fs::write(&legacy, "stray").expect("a stray file where the legacy login was");
        assert_eq!(scene.adopt(&login).await, home);
        assert_eq!(
            read(&home.join(CREDENTIALS)),
            "renewed",
            "a login that has its own sign-in never takes another"
        );
        assert_eq!(read(&legacy), "stray");

        let claude = scene.sign_in(AgentKind::Claude).await;
        let claude_home = scene.adopt(&claude).await;
        assert!(claude_home.is_dir());
        assert!(!claude_home.join(CREDENTIALS).exists());
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_legacy_login_is_left_alone_once_a_second_codex_login_exists() {
        let scene = Scene::new().await;
        let first = scene.sign_in(AgentKind::Codex).await;
        let second = scene.sign_in(AgentKind::Codex).await;
        let legacy = scene.legacy();

        for login in [&first, &second] {
            let home = scene.adopt(login).await;
            assert!(home.is_dir());
            assert!(
                !home.join(CREDENTIALS).exists(),
                "with two logins, the legacy one could belong to either"
            );
        }
        assert_eq!(read(&legacy), LEGACY);
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_linked_auth_file_is_never_adopted() {
        let scene = Scene::new().await;
        let login = scene.sign_in(AgentKind::Codex).await;
        let target = scene.directory.path().join("elsewhere.json");
        fs::write(&target, LEGACY).expect("a file outside the state root");
        let link = scene.root().join(CREDENTIALS);
        symlink(&target, &link).expect("a linked auth.json");

        let home = scene.adopt(&login).await;

        assert!(!home.join(CREDENTIALS).exists());
        assert!(
            fs::symlink_metadata(&link).is_ok_and(|metadata| metadata.file_type().is_symlink()),
            "the link stays where it was"
        );
        assert_eq!(read(&target), LEGACY);
        scene.remove().await;
    }
}
