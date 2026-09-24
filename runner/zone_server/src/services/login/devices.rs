//! Codex device sign-ins in flight, at most one per organization, and the lock that orders every
//! change to an organization's agent sign-ins.
//!
//! A sign-in finishes on its own, long after the request that started it. The task that records
//! it takes the organization's lock and acts only while its attempt is still the current one, and
//! a sign-out takes the same lock and ends the attempt first, so a sign-in that finishes around a
//! sign-out can never record a login after it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, SubsecRound, Utc};
use dashmap::DashMap;
use futures::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::llm::provider::environment;
use zone_core::secret::redact;

use super::codex::{self, CREDENTIALS, Device, Prompt};
use super::error::Error;
use super::locks::{Guard, Locks};
use super::{audit, oauth, probe};
use crate::config::Config;
use crate::db::agent_logins::{self, Upsert};
use crate::db::organizations;
use crate::state::AppState;

const STOPPED: &str = "The codex sign-in stopped before it finished";
const UNSAVED: &str = "Codex signed in, but Zone could not record the sign-in. Start again.";

static DEVICES: LazyLock<Devices> = LazyLock::new(Devices::default);

/// How a pending sign-in ends: `Err` carries why it failed, in codex's own words.
type Outcome = Shared<BoxFuture<'static, Result<(), String>>>;

/// Resolves once how an attempt ended has been recorded.
type Completion = Shared<BoxFuture<'static, ()>>;

struct Attempt {
    id: Uuid,
    initiator: Uuid,
    email: String,
    prompt: Prompt,
    cancel: Option<oneshot::Sender<()>>,
    outcome: Outcome,
    completion: Completion,
}

#[derive(Default)]
struct Devices {
    locks: Locks,
    attempts: DashMap<Uuid, Attempt>,
    failures: DashMap<Uuid, String>,
}

/// Starts a codex device sign-in for the organization, or hands back the prompt of the one
/// already waiting for its code.
pub async fn start(
    state: &AppState,
    organization: Uuid,
    user: Uuid,
    email: &str,
) -> Result<Prompt, Error> {
    let (prompt, _) = DEVICES.start(state, organization, user, email).await?;
    Ok(prompt)
}

/// Signs the organization out of `agent`, and records who did. Every Claude sign-in in flight is
/// ended first; for codex, a pending sign-in is stopped, and codex then logs out of the
/// organization's home.
pub async fn sign_out(
    state: &AppState,
    organization: Uuid,
    agent: AgentKind,
    user: Uuid,
    email: &str,
) -> Result<(), Error> {
    DEVICES
        .sign_out(state, organization, agent, user, email)
        .await
}

/// The lock that orders every change to the organization's agent sign-ins, until it is dropped.
pub(super) async fn lock(organization: Uuid) -> Guard<'static> {
    DEVICES.locks.lock(organization).await
}

/// Forgets everything this server keeps for a deleted organization's coding agents: its Claude
/// sign-ins in flight are dropped, a pending codex sign-in is stopped and codex logs out of the
/// organization's home before it returns, and the organization's agent state is then removed in
/// the background, which logs a removal that fails. It runs to the end even when its caller stops
/// waiting for it.
pub async fn forget(state: &AppState, organization: Uuid) -> Result<(), Error> {
    forget_with(state, organization, |directory: &Path| {
        fs::remove_dir_all(directory)
    })
    .await
}

/// [`forget`], removing the organization's agent state with `remove`.
async fn forget_with(
    state: &AppState,
    organization: Uuid,
    remove: impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
) -> Result<(), Error> {
    let state = state.clone();
    tokio::spawn(async move { DEVICES.forget(&state, organization, remove).await })
        .await
        .unwrap_or_else(|error| {
            Err(Error::Internal(format!(
                "Forgetting organization {organization}'s coding agents stopped: {error}"
            )))
        })
}

/// The organization's codex sign-in in flight: the prompt codex printed, while its code can still
/// be entered, and who started it.
pub fn pending(organization: Uuid) -> Option<(Option<Prompt>, Uuid)> {
    let now = Utc::now();
    DEVICES.attempts.get(&organization).map(|attempt| {
        let prompt = attempt.waiting(now).then(|| attempt.prompt.clone());
        (prompt, attempt.initiator)
    })
}

/// Why the organization's last device sign-in failed, until another one starts.
pub fn failure(organization: Uuid) -> Option<String> {
    DEVICES
        .failures
        .get(&organization)
        .map(|failure| failure.value().clone())
}

/// The environment every sign-in command for `agent` runs with: what any agent inherits from the
/// server, and the agent's own defaults. It names no home, so a command reads the host's own
/// sign-in unless its caller adds one.
pub(super) fn variables(agent: AgentKind) -> BTreeMap<String, String> {
    let mut variables = environment::inherited();
    variables.extend(
        agent
            .defaults()
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string())),
    );
    variables
}

impl Attempt {
    /// Whether its code can still be entered: codex still waits for it, and it has not expired.
    fn waiting(&self, now: DateTime<Utc>) -> bool {
        self.outcome.peek().is_none() && self.prompt.expires_at > now
    }
}

impl Devices {
    /// The prompt, and for a new attempt the task that records how it ends. An attempt whose code
    /// can no longer be entered is ended, and how it ended recorded, before a new one starts.
    async fn start(
        &'static self,
        state: &AppState,
        organization: Uuid,
        user: Uuid,
        email: &str,
    ) -> Result<(Prompt, Option<Completion>), Error> {
        let _guard = loop {
            let guard = self.locks.lock(organization).await;
            let ending = match self.attempts.get_mut(&organization) {
                None => None,
                Some(attempt) if attempt.waiting(Utc::now()) => {
                    return Ok((attempt.prompt.clone(), None));
                }
                Some(mut attempt) => {
                    if let Some(cancel) = attempt.cancel.take() {
                        let _ = cancel.send(());
                    }
                    Some(attempt.completion.clone())
                }
            };
            let Some(completion) = ending else {
                break guard;
            };
            drop(guard);
            completion.await;
        };
        if organizations::get_organization(state.db(), organization)
            .await?
            .is_none()
        {
            return Err(Error::Deleted);
        }
        self.failures.remove(&organization);

        let config = state.config();
        let home = config
            .agents
            .create_home(organization, AgentKind::Codex)
            .map_err(|error| {
                Error::Internal(format!(
                    "Could not prepare the codex CLI's state directory: {error}"
                ))
            })?;
        let executable = config.agent_executable(AgentKind::Codex);
        let device = codex::device(&executable, &home, &variables(AgentKind::Codex)).await;
        let Device {
            prompt,
            outcome,
            cancel,
        } = match device {
            Ok(device) => device,
            Err(codex::Error::Filesystem(message)) => return Err(Error::Internal(message)),
            Err(error) => {
                let error = refusal(error);
                self.failures.insert(organization, error.to_string());
                return Err(error);
            }
        };

        let id = Uuid::new_v4();
        let outcome = ending(organization, outcome);
        let prompt = Prompt {
            expires_at: prompt.expires_at.trunc_subsecs(0),
            ..prompt
        };
        let completion =
            tokio::spawn(self.complete(state.clone(), organization, id, outcome.clone()))
                .map(|_| ())
                .boxed()
                .shared();
        self.attempts.insert(
            organization,
            Attempt {
                id,
                initiator: user,
                email: email.to_string(),
                prompt: prompt.clone(),
                cancel: Some(cancel),
                outcome,
                completion: completion.clone(),
            },
        );
        Ok((prompt, Some(completion)))
    }

    /// Records how attempt `id` ended, unless it is no longer the organization's current one.
    async fn complete(&self, state: AppState, organization: Uuid, id: Uuid, outcome: Outcome) {
        let ended = outcome.await;
        let label = match ended {
            Ok(()) => describe(state.config(), organization).await,
            Err(_) => None,
        };

        let _guard = self.locks.lock(organization).await;
        let Some((initiator, email)) = self
            .attempts
            .get(&organization)
            .filter(|attempt| attempt.id == id)
            .map(|attempt| (attempt.initiator, attempt.email.clone()))
        else {
            return;
        };
        match ended {
            Ok(()) => {
                let login = Upsert {
                    organization_id: organization,
                    agent: AgentKind::Codex.as_str(),
                    credential: None,
                    label: label.as_deref(),
                    expires_at: None,
                };
                match agent_logins::upsert(state.db(), &login).await {
                    Ok(login) => {
                        self.attempts.remove(&organization);
                        audit::signed_in(state.db(), organization, initiator, &email, &login).await;
                    }
                    Err(error) => {
                        tracing::error!(
                            %organization,
                            %error,
                            "could not record a finished codex sign-in; logging codex out"
                        );
                        if let Err(error) = log_out(state.config(), organization).await {
                            tracing::error!(
                                %organization,
                                %error,
                                "could not log codex out of a sign-in Zone did not record"
                            );
                        }
                        self.failures.insert(organization, UNSAVED.to_string());
                        self.attempts.remove(&organization);
                    }
                }
            }
            Err(reason) => {
                self.failures.insert(organization, reason);
                self.attempts.remove(&organization);
            }
        }
    }

    async fn sign_out(
        &self,
        state: &AppState,
        organization: Uuid,
        agent: AgentKind,
        user: Uuid,
        email: &str,
    ) -> Result<(), Error> {
        let _guard = self.locks.lock(organization).await;
        match agent {
            AgentKind::Claude => oauth::forget(organization),
            AgentKind::Codex => {
                self.stop(organization).await;
                log_out(state.config(), organization).await?;
            }
        }
        let Some(login) = agent_logins::get(state.db(), organization, agent.as_str()).await? else {
            return Ok(());
        };
        agent_logins::delete(state.db(), organization, agent.as_str()).await?;
        audit::signed_out(state.db(), organization, user, email, &login).await;
        Ok(())
    }

    /// Returns once codex has logged out. The organization's agent state is then removed off the
    /// async runtime, still under the organization's lock.
    async fn forget(
        &'static self,
        state: &AppState,
        organization: Uuid,
        remove: impl FnOnce(&Path) -> io::Result<()> + Send + 'static,
    ) -> Result<(), Error> {
        let guard = self.locks.lock(organization).await;
        oauth::forget(organization);
        self.stop(organization).await;
        self.failures.remove(&organization);
        let config = state.config();
        let Some(directory) = agent_state(config, organization).await? else {
            return Ok(());
        };
        if let Err(error) = log_out(config, organization).await {
            tracing::warn!(%organization, %error, "codex could not log a deleted organization out");
        }
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            if let Err(error) = remove(&directory) {
                tracing::error!(
                    %organization,
                    %error,
                    directory = %directory.display(),
                    "Could not remove a deleted organization's agent state"
                );
            }
        });
        Ok(())
    }

    /// Ends the organization's pending sign-in and waits for codex to exit, so a login codex
    /// saved just before is already in place when codex logs out.
    async fn stop(&self, organization: Uuid) {
        let Some((_, attempt)) = self.attempts.remove(&organization) else {
            return;
        };
        if let Some(cancel) = attempt.cancel {
            let _ = cancel.send(());
        }
        let _ = attempt.outcome.await;
    }
}

/// `<state>/<organization>`, which holds every agent home of the organization, when it is a real
/// directory, inspected off the async runtime. Anything else in its place is refused, so nothing
/// done to it reaches outside the state root.
async fn agent_state(config: &Config, organization: Uuid) -> Result<Option<PathBuf>, Error> {
    let home = config.agents.home(organization, AgentKind::Codex);
    let Some(directory) = home.parent().map(Path::to_path_buf) else {
        return Ok(None);
    };
    tokio::task::spawn_blocking(move || inspect(directory))
        .await
        .unwrap_or_else(|error| {
            Err(Error::Internal(format!(
                "Inspecting organization {organization}'s agent state stopped: {error}"
            )))
        })
}

fn inspect(directory: PathBuf) -> Result<Option<PathBuf>, Error> {
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(directory)),
        Ok(_) => Err(Error::Internal(format!(
            "{} is not a directory, so it was left in place",
            directory.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Internal(format!(
            "Could not inspect {}: {error}",
            directory.display()
        ))),
    }
}

/// Runs `codex logout` in the organization's codex home, when it has one. Codex revokes its login
/// upstream and deletes it; when it cannot, the login is deleted here, so the sign-out holds.
async fn log_out(config: &Config, organization: Uuid) -> Result<(), Error> {
    let home = config.agents.home(organization, AgentKind::Codex);
    if !home.is_dir() {
        return Ok(());
    }
    let executable = config.agent_executable(AgentKind::Codex);
    let Err(error) = codex::logout(&executable, &home, &variables(AgentKind::Codex)).await else {
        return Ok(());
    };
    tracing::warn!(%organization, error = %said(&error), "codex could not log out; deleting its login");
    match fs::remove_file(home.join(CREDENTIALS)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "Could not delete codex's login: {error}"
        ))),
    }
}

/// How codex describes the login it saved in the organization's home, such as "ChatGPT".
async fn describe(config: &Config, organization: Uuid) -> Option<String> {
    let home = config.agents.home(organization, AgentKind::Codex);
    let mut variables = variables(AgentKind::Codex);
    variables.insert(
        AgentKind::Codex.home().to_string(),
        home.to_str()?.to_string(),
    );
    let executable = config.agent_executable(AgentKind::Codex);
    match probe::check(AgentKind::Codex, &executable, &variables).await {
        Ok(probe) => probe.label,
        Err(error) => {
            tracing::warn!(%organization, error = %said(&error), "could not ask codex how it signed in");
            None
        }
    }
}

/// How the organization's attempt ends, as its status shows it. Zone's own failure to save the
/// login is logged, and shows as [`UNSAVED`].
fn ending(organization: Uuid, outcome: JoinHandle<Result<(), codex::Error>>) -> Outcome {
    outcome
        .map(move |finished| match finished {
            Ok(Ok(())) => Ok(()),
            Ok(Err(codex::Error::Filesystem(message))) => {
                tracing::error!(%organization, %message, "could not save codex's new login");
                Err(UNSAVED.to_string())
            }
            Ok(Err(error)) => Err(said(&error)),
            Err(_) => Err(STOPPED.to_string()),
        })
        .boxed()
        .shared()
}

fn refusal(error: codex::Error) -> Error {
    match error {
        codex::Error::Unavailable {
            executable,
            message,
        } => {
            tracing::warn!(%executable, %message, "codex could not be started");
            Error::Unavailable(AgentKind::Codex)
        }
        error => Error::Refused(said(&error)),
    }
}

/// What codex said, with anything that looks like a credential blanked: its stderr reaches the
/// sign-in panel as it is.
fn said(error: &codex::Error) -> String {
    redact(&error.to_string()).into_owned()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::Duration;

    use sqlx::PgPool;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use super::*;
    use crate::config::{AgentConfig, ModelBackend};
    use crate::db::agent_logins::AgentLoginRow;
    use crate::services::login::caller::Caller;
    use crate::services::login::claude::{Redirect, Scope};
    use crate::services::login::codex::testing::{PROMPT, capture, script};
    use crate::services::login::console::Console;

    const EMAIL: &str = "admin@example.com";
    const CALLBACK_PORT: u16 = 54_545;
    const CONSOLE: &str = "http://localhost:3000";
    const WAIT: Duration = Duration::from_secs(20);
    const PAUSE: Duration = Duration::from_millis(50);
    const ATTEMPTS: u128 = WAIT.as_millis() / PAUSE.as_millis();
    const APPROVE: &str = "approve";
    const HOLD: &str = "hold";
    const PROBING: &str = "probing";
    const RELEASE: &str = "release";
    const STARTED: &str = "started";
    const LOGOUTS: &str = "logouts";
    const LASTING: &str = "expires in 15 minutes";
    const LAPSED: &str = "expires in 0 minutes";

    /// Codex as far as a sign-in needs it. `login --device-auth` counts itself in `started`,
    /// prints the recorded prompt and saves a login once `approve` appears, consuming it;
    /// `login status` marks `probing` and holds while `hold` is there without `release`; `logout`
    /// adds the home it logged out of to `logouts`.
    fn stand_in(control: &Path, prompt: &str) -> String {
        let control = control.display();
        let polls = WAIT.as_millis() / PAUSE.as_millis();
        let pause = PAUSE.as_secs_f64();
        format!(
            r#"case "$*" in
'login --device-auth')
    echo started >> '{control}/{STARTED}'
    cat '{prompt}'
    waited=0
    while [ ! -e '{control}/{APPROVE}' ] && [ -d '{control}' ] && [ "$waited" -lt {polls} ]; do
        sleep {pause}
        waited=$((waited + 1))
    done
    rm '{control}/{APPROVE}' || exit 1
    printf '{{}}' > "$CODEX_HOME/{CREDENTIALS}"
    exit 0
    ;;
'login status')
    touch '{control}/{PROBING}'
    waited=0
    while [ -e '{control}/{HOLD}' ] && [ ! -e '{control}/{RELEASE}' ] && [ "$waited" -lt {polls} ]; do
        sleep {pause}
        waited=$((waited + 1))
    done
    if [ -f "$CODEX_HOME/{CREDENTIALS}" ]; then
        echo 'Logged in using ChatGPT' >&2
        exit 0
    fi
    echo 'Not logged in' >&2
    exit 1
    ;;
logout)
    printf '%s\n' "$CODEX_HOME" >> '{control}/{LOGOUTS}'
    rm -f "$CODEX_HOME/{CREDENTIALS}"
    echo 'Successfully logged out' >&2
    exit 0
    ;;
*)
    exit 64
    ;;
esac"#
        )
    }

    /// An organization, its admin, and a server whose codex is the stand-in.
    struct Scene {
        pool: PgPool,
        state: AppState,
        organization: Uuid,
        user: Uuid,
        directory: TempDir,
    }

    impl Scene {
        async fn new(prompt: &[u8]) -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("isolated test database"),
            )
            .await
            .expect("the test database accepts connections");
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Device sign-ins', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to sign in");
            let user = crate::db::users::create_user(
                &pool,
                &format!("device-{organization}@example.com"),
                "password_hash",
                None,
                false,
            )
            .await
            .expect("an admin to sign it in")
            .id;
            let directory = TempDir::new().expect("a temporary directory");
            let prompt = capture(&directory, "prompt", prompt);
            let codex = script(&directory, &stand_in(directory.path(), &prompt));
            let state = AppState::new(
                Config {
                    model_backend: ModelBackend::Cli {
                        agent: AgentKind::Codex,
                        executable: Some(codex),
                    },
                    agents: AgentConfig {
                        state: directory.path().join("agents"),
                        host_login: false,
                        ..AgentConfig::default()
                    },
                    ..crate::state::test_config()
                },
                pool.clone(),
                None,
            );
            state.disable_mcp();
            Self {
                pool,
                state,
                organization,
                user,
                directory,
            }
        }

        fn control(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        fn touch(&self, name: &str) {
            std::fs::write(self.control(name), b"").expect("a marker for the stand-in codex");
        }

        fn lines(&self, name: &str) -> Vec<String> {
            std::fs::read_to_string(self.control(name))
                .map(|text| text.lines().map(str::to_string).collect())
                .unwrap_or_default()
        }

        fn home(&self) -> PathBuf {
            self.state
                .config()
                .agents
                .home(self.organization, AgentKind::Codex)
        }

        async fn start(&self) -> (Prompt, Option<Completion>) {
            DEVICES
                .start(&self.state, self.organization, self.user, EMAIL)
                .await
                .expect("codex printed its prompt")
        }

        async fn sign_out(&self) {
            sign_out(
                &self.state,
                self.organization,
                AgentKind::Codex,
                self.user,
                EMAIL,
            )
            .await
            .expect("the sign-out");
        }

        fn attempt(&self) -> Option<Uuid> {
            DEVICES
                .attempts
                .get(&self.organization)
                .map(|attempt| attempt.id)
        }

        /// Whether the organization's pending sign-in shows its prompt, and to whom it belongs.
        fn shown(&self) -> Option<(bool, Uuid)> {
            pending(self.organization).map(|(prompt, initiator)| (prompt.is_some(), initiator))
        }

        async fn login(&self) -> Option<AgentLoginRow> {
            agent_logins::get(&self.pool, self.organization, AgentKind::Codex.as_str())
                .await
                .expect("the logins are readable")
        }

        async fn delete_organization(&self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization can be deleted");
        }

        async fn remove(self) {
            self.delete_organization().await;
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(self.user)
                .execute(&self.pool)
                .await
                .expect("the admin can be deleted");
        }
    }

    async fn appeared(path: &Path) -> bool {
        until(|| path.exists()).await
    }

    async fn gone(path: &Path) -> bool {
        until(|| !path.exists()).await
    }

    async fn until(condition: impl Fn() -> bool) -> bool {
        for _ in 0..ATTEMPTS {
            if condition() {
                return true;
            }
            tokio::time::sleep(PAUSE).await;
        }
        false
    }

    /// A removal that says when it starts, then waits to be released, for [`WAIT`] at most.
    fn slow(
        started: oneshot::Sender<()>,
        released: mpsc::Receiver<()>,
    ) -> impl FnOnce(&Path) -> io::Result<()> + Send + 'static {
        move |directory: &Path| {
            let _ = started.send(());
            let _ = released.recv_timeout(WAIT);
            fs::remove_dir_all(directory)
        }
    }

    async fn finished(completion: Option<Completion>) {
        timeout(
            WAIT,
            completion.expect("a new sign-in is watched until it ends"),
        )
        .await
        .expect("how the sign-in ended to be recorded");
    }

    #[test]
    fn every_sign_in_command_runs_with_the_agents_defaults_and_no_agents_home() {
        for agent in AgentKind::ALL {
            let variables = variables(agent);

            for (name, value) in agent.defaults() {
                assert_eq!(
                    variables.get(*name).map(String::as_str),
                    Some(*value),
                    "{name}"
                );
            }
            for home in AgentKind::ALL.map(AgentKind::home) {
                assert!(
                    !variables.contains_key(home),
                    "{home} would point {agent}'s host sign-in check at another home"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_sign_in_that_lands_after_a_sign_out_is_never_recorded() {
        let scene = Scene::new(PROMPT).await;
        scene.touch(HOLD);

        let (_, first) = scene.start().await;
        scene.touch(APPROVE);
        assert!(
            appeared(&scene.control(PROBING)).await,
            "codex never finished the first sign-in"
        );
        scene.sign_out().await;
        let (_, second) = scene.start().await;
        scene.touch(RELEASE);
        finished(first).await;

        assert!(
            scene.login().await.is_none(),
            "a sign-in that ended after the sign-out was recorded"
        );
        assert!(
            pending(scene.organization).is_some(),
            "the first sign-in's ending ended the second one"
        );
        assert_eq!(failure(scene.organization), None);
        assert!(
            !scene.home().join(CREDENTIALS).exists(),
            "codex's login outlived the sign-out"
        );

        scene.sign_out().await;
        finished(second).await;
        assert!(pending(scene.organization).is_none());
        assert!(
            !DEVICES.locks.kept(scene.organization),
            "an idle organization's lock was kept"
        );
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_finished_sign_in_that_cannot_be_recorded_is_logged_out() {
        let scene = Scene::new(PROMPT).await;
        let (_, completion) = scene.start().await;

        scene.delete_organization().await;
        scene.touch(APPROVE);
        finished(completion).await;

        assert!(
            !scene.home().join(CREDENTIALS).exists(),
            "codex's login outlived a sign-in Zone could not record"
        );
        assert_eq!(scene.lines(LOGOUTS), [scene.home().display().to_string()]);
        assert_eq!(failure(scene.organization).as_deref(), Some(UNSAVED));
        assert!(pending(scene.organization).is_none());
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_prompt_past_its_expiry_is_never_handed_out_again() {
        let lapsed = String::from_utf8_lossy(PROMPT).replace(LASTING, LAPSED);
        assert_ne!(
            lapsed.as_bytes(),
            PROMPT,
            "the recorded prompt no longer says when it expires"
        );
        let scene = Scene::new(lapsed.as_bytes()).await;
        let (_, first) = scene.start().await;
        let attempt = scene.attempt();

        let shown = scene.shown();
        let (_, second) = scene.start().await;

        assert_eq!(
            shown,
            Some((false, scene.user)),
            "a prompt past its expiry was shown"
        );
        assert_eq!(
            scene.lines(STARTED).len(),
            2,
            "a prompt past its expiry was handed out again"
        );
        assert_ne!(scene.attempt(), attempt);
        finished(first).await;
        scene.sign_out().await;
        finished(second).await;
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_prompt_whose_sign_in_already_succeeded_is_never_handed_out_again() {
        let scene = Scene::new(PROMPT).await;
        scene.touch(HOLD);
        let (_, first) = scene.start().await;
        let attempt = scene.attempt();
        scene.touch(APPROVE);
        assert!(
            appeared(&scene.control(PROBING)).await,
            "codex never finished the first sign-in"
        );

        let shown = scene.shown();
        let again = tokio::spawn({
            let state = scene.state.clone();
            let (organization, user) = (scene.organization, scene.user);
            async move { DEVICES.start(&state, organization, user, EMAIL).await }
        });
        scene.touch(RELEASE);
        let (_, second) = timeout(WAIT, again)
            .await
            .expect("the second start to finish")
            .expect("the second start not to panic")
            .expect("codex printed a second prompt");

        assert_eq!(
            shown,
            Some((false, scene.user)),
            "the prompt of a sign-in that already succeeded was shown"
        );
        assert_eq!(
            scene.lines(STARTED).len(),
            2,
            "the prompt of a sign-in that already succeeded was handed out again"
        );
        assert_ne!(scene.attempt(), attempt);
        assert!(
            scene.login().await.is_some(),
            "the sign-in that succeeded was never recorded"
        );
        finished(first).await;
        scene.sign_out().await;
        finished(second).await;
        scene.remove().await;
    }

    #[tokio::test]
    async fn forgetting_an_organization_stops_its_sign_in_and_leaves_nothing_of_it() {
        let scene = Scene::new(PROMPT).await;
        let (_, completion) = scene.start().await;
        DEVICES
            .failures
            .insert(scene.organization, "an earlier sign-in failed".to_string());
        let claude = oauth::start(
            scene.organization,
            &Caller {
                user: scene.user,
                email: EMAIL.to_string(),
                session: Uuid::new_v4(),
            },
            Scope::Inference,
            Redirect::Loopback(CALLBACK_PORT),
            Console::at(CONSOLE),
        )
        .await;
        let claude_state = reqwest::Url::parse(&claude.url)
            .expect("an authorize URL")
            .query_pairs()
            .find(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .expect("the sign-in's state");
        let organization_state = scene
            .home()
            .parent()
            .expect("the codex home is inside the organization's state")
            .to_path_buf();
        scene.delete_organization().await;

        forget(&scene.state, scene.organization)
            .await
            .expect("the organization to be forgotten");
        finished(completion).await;

        assert_eq!(scene.lines(LOGOUTS), [scene.home().display().to_string()]);
        assert!(
            gone(&organization_state).await,
            "the deleted organization's agent state was left"
        );
        assert!(pending(scene.organization).is_none());
        assert_eq!(failure(scene.organization), None);
        assert!(
            crate::services::login::pending::claim_loopback(&claude_state).is_none(),
            "a deleted organization's Claude sign-in could still finish"
        );
        assert!(
            until(|| !DEVICES.locks.kept(scene.organization)).await,
            "a deleted organization's lock was kept"
        );
        scene.remove().await;
    }

    #[tokio::test]
    async fn forgetting_an_organization_answers_once_codex_logged_out_while_its_state_is_removed() {
        let scene = Scene::new(PROMPT).await;
        let (_, completion) = scene.start().await;
        scene.touch(APPROVE);
        finished(completion).await;
        assert!(scene.login().await.is_some(), "codex never signed in");
        let organization_state = scene
            .home()
            .parent()
            .expect("the codex home is inside the organization's state")
            .to_path_buf();
        scene.delete_organization().await;
        let (started, starting) = oneshot::channel();
        let (release, released) = mpsc::channel();

        forget_with(&scene.state, scene.organization, slow(started, released))
            .await
            .expect("the organization to be forgotten");
        timeout(WAIT, starting)
            .await
            .expect("the removal to start")
            .expect("the removal to say so");

        assert!(
            organization_state.exists(),
            "forgetting the organization waited for its agent state to be removed"
        );
        assert_eq!(scene.lines(LOGOUTS), [scene.home().display().to_string()]);
        assert!(
            !codex::signed_in(&scene.home()),
            "the agent state being removed still holds a codex login"
        );
        assert!(
            DEVICES.locks.kept(scene.organization),
            "the organization's lock was let go before its agent state was removed"
        );
        let restart = tokio::spawn({
            let state = scene.state.clone();
            let (organization, user) = (scene.organization, scene.user);
            async move { DEVICES.start(&state, organization, user, EMAIL).await.err() }
        });
        release
            .send(())
            .expect("the removal to wait for its release");
        let restarted = timeout(WAIT, restart)
            .await
            .expect("the start to finish")
            .expect("the start not to panic");

        assert!(matches!(restarted, Some(Error::Deleted)), "{restarted:?}");
        assert_eq!(
            scene.lines(STARTED).len(),
            1,
            "codex ran for a deleted organization"
        );
        assert!(
            gone(&organization_state).await,
            "the deleted organization's agent state was left"
        );
        assert!(
            until(|| !DEVICES.locks.kept(scene.organization)).await,
            "a deleted organization's lock was kept"
        );
        scene.remove().await;
    }

    #[tokio::test]
    async fn a_start_that_waited_out_its_organizations_deletion_starts_nothing() {
        let scene = Scene::new(PROMPT).await;
        scene.delete_organization().await;

        let started = DEVICES
            .start(&scene.state, scene.organization, scene.user, EMAIL)
            .await;

        let error = started.err();
        assert!(matches!(error, Some(Error::Deleted)), "{error:?}");
        assert!(
            scene.lines(STARTED).is_empty(),
            "codex ran for a deleted organization"
        );
        assert!(
            !scene.home().exists(),
            "a deleted organization's codex home was made again"
        );
        assert_eq!(failure(scene.organization), None);
        scene.remove().await;
    }

    #[tokio::test]
    async fn an_organizations_state_that_is_a_link_is_never_followed_when_it_is_forgotten() {
        let scene = Scene::new(PROMPT).await;
        let elsewhere = scene.control("elsewhere");
        std::fs::create_dir_all(elsewhere.join(AgentKind::Codex.as_str()))
            .expect("a directory outside the state root");
        std::fs::write(elsewhere.join("kept"), b"").expect("a file outside the state root");
        let root = scene.state.config().agents.state.clone();
        std::fs::create_dir_all(&root).expect("the state root");
        let linked = root.join(scene.organization.to_string());
        symlink(&elsewhere, &linked).expect("a link in place of the organization's state");

        let forgotten = forget(&scene.state, scene.organization).await;

        assert!(
            forgotten.is_err(),
            "a link in place of the organization's state was taken for it"
        );
        assert!(
            elsewhere.join("kept").exists(),
            "forgetting followed a link out of the state root"
        );
        assert!(
            std::fs::symlink_metadata(&linked)
                .is_ok_and(|metadata| metadata.file_type().is_symlink()),
            "the link was removed"
        );
        assert!(
            scene.lines(LOGOUTS).is_empty(),
            "codex logged out of a home behind a link"
        );
        scene.remove().await;
    }
}
