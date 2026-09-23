//! Codex device sign-ins in flight, at most one per organization, and the lock that orders every
//! change to an organization's agent sign-ins.
//!
//! A sign-in finishes on its own, long after the request that started it. The task that records
//! it takes the organization's lock and acts only while its attempt is still the current one, and
//! a sign-out takes the same lock and ends the attempt first, so a sign-in that finishes around a
//! sign-out can never record a login after it.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, LazyLock};

use chrono::SubsecRound;
use dashmap::DashMap;
use futures::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::llm::provider::environment;
use zone_core::secret::redact;

use super::codex::{self, CREDENTIALS, Device, Prompt};
use super::error::Error;
use super::{audit, probe};
use crate::config::Config;
use crate::db::agent_logins::{self, Upsert};
use crate::state::AppState;

const STOPPED: &str = "The codex sign-in stopped before it finished";
const UNSAVED: &str = "Codex signed in, but Zone could not record the sign-in. Start again.";

static DEVICES: LazyLock<Devices> = LazyLock::new(Devices::default);

/// How a pending sign-in ends: `Err` carries why it failed, in codex's own words.
type Outcome = Shared<BoxFuture<'static, Result<(), String>>>;

struct Attempt {
    id: Uuid,
    initiator: Uuid,
    email: String,
    prompt: Prompt,
    cancel: oneshot::Sender<()>,
    outcome: Outcome,
}

#[derive(Default)]
struct Devices {
    locks: DashMap<Uuid, Arc<Mutex<()>>>,
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

/// Signs the organization out of `agent`, and records who did. For codex, a pending sign-in is
/// stopped first, and codex then logs out of the organization's home.
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

/// The prompt codex printed for the organization's pending sign-in, and who started it.
pub fn pending(organization: Uuid) -> Option<(Prompt, Uuid)> {
    DEVICES
        .attempts
        .get(&organization)
        .map(|attempt| (attempt.prompt.clone(), attempt.initiator))
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

impl Devices {
    async fn lock(&self, organization: Uuid) -> OwnedMutexGuard<()> {
        let lock = self.locks.entry(organization).or_default().value().clone();
        lock.lock_owned().await
    }

    /// The prompt, and for a new attempt the task that records how it ends.
    async fn start(
        &'static self,
        state: &AppState,
        organization: Uuid,
        user: Uuid,
        email: &str,
    ) -> Result<(Prompt, Option<JoinHandle<()>>), Error> {
        let _guard = self.lock(organization).await;
        let waiting = self
            .attempts
            .get(&organization)
            .map(|attempt| attempt.prompt.clone());
        if let Some(prompt) = waiting {
            return Ok((prompt, None));
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
        let Device {
            prompt,
            outcome,
            cancel,
        } = codex::device(&executable, &home, &variables(AgentKind::Codex))
            .await
            .map_err(|error| {
                let error = refusal(error);
                self.failures.insert(organization, error.to_string());
                error
            })?;

        let id = Uuid::new_v4();
        let outcome = ending(outcome);
        let prompt = Prompt {
            expires_at: prompt.expires_at.trunc_subsecs(0),
            ..prompt
        };
        self.attempts.insert(
            organization,
            Attempt {
                id,
                initiator: user,
                email: email.to_string(),
                prompt: prompt.clone(),
                cancel,
                outcome: outcome.clone(),
            },
        );
        let completion = tokio::spawn(self.complete(state.clone(), organization, id, outcome));
        Ok((prompt, Some(completion)))
    }

    /// Records how attempt `id` ended, unless it is no longer the organization's current one.
    async fn complete(&self, state: AppState, organization: Uuid, id: Uuid, outcome: Outcome) {
        let ended = outcome.await;
        let label = match ended {
            Ok(()) => describe(state.config(), organization).await,
            Err(_) => None,
        };

        let _guard = self.lock(organization).await;
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
                        tracing::error!(%organization, %error, "could not record a finished codex sign-in");
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
        let _guard = self.lock(organization).await;
        if agent == AgentKind::Codex {
            self.stop(organization).await;
            log_out(state.config(), organization).await?;
        }
        let Some(login) = agent_logins::get(state.db(), organization, agent.as_str()).await? else {
            return Ok(());
        };
        agent_logins::delete(state.db(), organization, agent.as_str()).await?;
        audit::signed_out(state.db(), organization, user, email, &login).await;
        Ok(())
    }

    /// Ends the organization's pending sign-in and waits for codex to exit, so a login codex
    /// saved just before is already in place when codex logs out.
    async fn stop(&self, organization: Uuid) {
        let Some((_, attempt)) = self.attempts.remove(&organization) else {
            return;
        };
        let _ = attempt.cancel.send(());
        let _ = attempt.outcome.await;
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
    match std::fs::remove_file(home.join(CREDENTIALS)) {
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

fn ending(outcome: JoinHandle<Result<(), codex::Error>>) -> Outcome {
    outcome
        .map(|finished| match finished {
            Ok(Ok(())) => Ok(()),
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
    use std::path::Path;
    use std::time::Duration;

    use sqlx::PgPool;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use super::*;
    use crate::config::{AgentConfig, ModelBackend};
    use crate::services::login::codex::testing::{PROMPT, capture, script};

    const EMAIL: &str = "admin@example.com";
    const WAIT: Duration = Duration::from_secs(20);
    const PAUSE: Duration = Duration::from_millis(50);
    const ATTEMPTS: u128 = WAIT.as_millis() / PAUSE.as_millis();
    const APPROVE: &str = "approve";
    const HOLD: &str = "hold";
    const PROBING: &str = "probing";
    const RELEASE: &str = "release";

    /// Codex as far as a sign-in needs it. `login --device-auth` prints the recorded prompt and
    /// saves a login once `approve` appears, consuming it; `login status` marks `probing` and
    /// holds while `hold` is there without `release`.
    fn stand_in(control: &Path, prompt: &str) -> String {
        let control = control.display();
        let polls = WAIT.as_millis() / PAUSE.as_millis();
        let pause = PAUSE.as_secs_f64();
        format!(
            r#"case "$*" in
'login --device-auth')
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

    fn touch(path: &Path) {
        std::fs::write(path, b"").expect("a marker for the stand-in codex");
    }

    async fn appeared(path: &Path) -> bool {
        for _ in 0..ATTEMPTS {
            if path.exists() {
                return true;
            }
            tokio::time::sleep(PAUSE).await;
        }
        false
    }

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
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
        let pool = pool().await;
        let organization = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Device race', $1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .expect("an organization to sign in");
        let user = crate::db::users::create_user(
            &pool,
            &format!("device-race-{organization}@example.com"),
            "password_hash",
            None,
            false,
        )
        .await
        .expect("an admin to sign it in")
        .id;
        let directory = TempDir::new().expect("a temporary directory");
        let control = directory.path();
        let prompt = capture(&directory, "prompt", PROMPT);
        let codex = script(&directory, &stand_in(control, &prompt));
        let state = AppState::new(
            Config {
                model_backend: ModelBackend::Cli {
                    agent: AgentKind::Codex,
                    executable: Some(codex),
                },
                agents: AgentConfig {
                    state: control.join("agents"),
                    host_login: false,
                    ..AgentConfig::default()
                },
                ..crate::state::test_config()
            },
            pool.clone(),
            None,
        );
        state.disable_mcp();
        let home = state.config().agents.home(organization, AgentKind::Codex);
        touch(&control.join(HOLD));

        let (_, first) = DEVICES
            .start(&state, organization, user, EMAIL)
            .await
            .expect("codex printed its prompt");
        let first = first.expect("a new sign-in is watched until it ends");
        touch(&control.join(APPROVE));
        assert!(
            appeared(&control.join(PROBING)).await,
            "codex never finished the first sign-in"
        );
        sign_out(&state, organization, AgentKind::Codex, user, EMAIL)
            .await
            .expect("the sign-out");
        DEVICES
            .start(&state, organization, user, EMAIL)
            .await
            .expect("a second sign-in");
        touch(&control.join(RELEASE));
        timeout(WAIT, first)
            .await
            .expect("the first sign-in's ending to be handled")
            .expect("the task that handles it not to panic");

        let login = agent_logins::get(&pool, organization, AgentKind::Codex.as_str())
            .await
            .expect("the logins are readable");
        assert!(
            login.is_none(),
            "a sign-in that ended after the sign-out was recorded"
        );
        assert!(
            pending(organization).is_some(),
            "the first sign-in's ending ended the second one"
        );
        assert_eq!(failure(organization), None);
        assert!(
            !home.join(CREDENTIALS).exists(),
            "codex's login outlived the sign-out"
        );

        sign_out(&state, organization, AgentKind::Codex, user, EMAIL)
            .await
            .expect("the second sign-in to stop");
        assert!(pending(organization).is_none());
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("the organization can be deleted");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .expect("the admin can be deleted");
    }
}
