//! Codex's own device-code sign-in, run against one organization's `CODEX_HOME`.

mod device;
mod error;
mod limits;
mod output;
mod process;
mod prompt;
mod staging;
#[cfg(test)]
pub(super) mod testing;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;
use zone_core::llm::AgentKind;

pub use device::Device;
pub use error::Error;
pub use limits::Limits;
pub use prompt::Prompt;

pub(super) use output::Output;

pub(super) const CREDENTIALS: &str = "auth.json";
const LOGIN: &[&str] = &["login", "--device-auth"];
const LOGOUT: &[&str] = &["logout"];

/// Starts `codex login --device-auth` for the organization whose `CODEX_HOME` is `home`, which the
/// caller has created, and returns once codex has printed the link and code a person must enter.
///
/// `environment` is the child's whole environment. Codex runs in a staging directory inside
/// `home`, because it deletes any login in its `CODEX_HOME` before printing a code: the
/// organization's existing login is replaced only when the new one succeeds. One device sign-in
/// may run per home at a time.
pub async fn device(
    executable: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<Device, Error> {
    device_with(executable, home, environment, Limits::default()).await
}

pub async fn device_with(
    executable: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
    limits: Limits,
) -> Result<Device, Error> {
    Device::start(executable, home, environment, limits).await
}

/// Runs `codex logout` against `home`, which revokes the login upstream and deletes it.
pub async fn logout(
    executable: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<(), Error> {
    let mut command = command(executable, LOGOUT, environment);
    command.env(AgentKind::Codex.home(), home);
    let output = Output::capture(command, executable).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(output.failed(executable))
    }
}

/// Whether `home` holds a Zone-managed codex login: its `auth.json` is a regular file. Codex never
/// checks the credentials in it until a turn needs them.
pub fn signed_in(home: &Path) -> bool {
    fs::symlink_metadata(home.join(CREDENTIALS))
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

pub(super) fn command(
    executable: &Path,
    arguments: &[&str],
    environment: &BTreeMap<String, String>,
) -> Command {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    command
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;
    use std::time::Duration;

    use chrono::{TimeDelta, Utc};
    use tempfile::TempDir;
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use super::testing::{
        EXCHANGE_FAILED, EXCHANGE_FAILED_PROMPT, POLL_FAILED, POLL_FAILED_PROMPT, PROMPT, REFUSED,
        RELOGIN_ABANDONED_PROMPT, RELOGIN_REFUSED, SUCCESS, SUCCESS_PROMPT, THROTTLED, UNSUPPORTED,
        capture, ended, environment, fake, fixture, process, recorded,
    };
    use super::*;

    const LOGGED_OUT: &[u8] = fixture!("fake-issuer/codex-logout-chatgpt.stderr");
    const SIGNED_OUT: &[u8] = fixture!("codex-logout-signed-out.stderr");
    const UNCONFIGURED: &[u8] = fixture!("fake-issuer/codex-login-status-missing-home.stderr");
    const EXISTING: &str = "{\"auth_mode\":\"chatgpt\",\"tokens\":{\"refresh_token\":\"the organization's existing login\"}}\n";
    const WRITTEN: &str = "{\"auth_mode\":\"chatgpt\",\"OPENAI_API_KEY\":null,\"tokens\":{\"id_token\":\"fixture\",\"access_token\":\"fixture\",\"refresh_token\":\"fixture\",\"account_id\":\"00000000-0000-4000-8000-000000000000\"},\"last_refresh\":\"2026-09-23T07:27:26.827371Z\"}\n";
    const REFUSAL: &str =
        "Error logging in with device code: device code request failed with status 403 Forbidden";
    /// What the real codex does on a re-login: it deletes the login in its `CODEX_HOME` before it
    /// asks for a code.
    const REVOKE: &str = "rm -f \"$CODEX_HOME/auth.json\"";
    const WAIT: Duration = Duration::from_secs(20);

    fn home(directory: &TempDir) -> PathBuf {
        let home = directory.path().join("home");
        fs::create_dir(&home).expect("the organization's home");
        home
    }

    fn staging(home: &Path) -> PathBuf {
        home.join(".login")
    }

    fn login() -> String {
        LOGIN.join(" ")
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).trim().to_string()
    }

    /// The environment Zone hands a codex child: its home is the organization's.
    fn organization(home: &Path) -> BTreeMap<String, String> {
        let mut environment = environment();
        environment.insert(
            AgentKind::Codex.home().to_string(),
            home.display().to_string(),
        );
        environment
    }

    async fn finish(outcome: JoinHandle<Result<(), Error>>) -> Result<(), Error> {
        timeout(WAIT, outcome)
            .await
            .expect("the sign-in to finish")
            .expect("the sign-in task not to panic")
    }

    #[tokio::test]
    async fn a_device_login_returns_the_link_and_code_codex_printed() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let prompt = capture(&directory, "prompt", PROMPT);
        let codex = fake(
            &directory,
            &login(),
            &format!("cat '{prompt}'\nexec sleep 60"),
        );
        let before = Utc::now();

        let device = device(&codex, &home, &environment())
            .await
            .expect("a prompt");
        let after = Utc::now();

        assert_eq!(
            device.prompt.verification_url,
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(device.prompt.user_code, "ABCD-EFGHI");
        assert!(
            device.prompt.expires_at >= before + TimeDelta::minutes(15)
                && device.prompt.expires_at <= after + TimeDelta::minutes(15),
            "the code's expiry is not 15 minutes from when it was printed: {}",
            device.prompt.expires_at
        );
        drop(device.cancel);
        let _ = finish(device.outcome).await;
    }

    #[tokio::test]
    async fn a_refused_code_request_fails_with_codexs_own_words() {
        for refusal in [REFUSED, UNSUPPORTED, THROTTLED] {
            let directory = TempDir::new().expect("a temporary directory");
            let home = home(&directory);
            let stderr = capture(&directory, "stderr", refusal);
            let codex = fake(&directory, &login(), &format!("cat '{stderr}' >&2\nexit 1"));

            let error = device(&codex, &home, &environment())
                .await
                .expect_err("a refusal");

            assert!(
                matches!(&error, Error::Failed(message) if *message == text(refusal)),
                "{error:?}"
            );
            assert!(!staging(&home).exists(), "the staging directory was left");
            assert!(!home.join(CREDENTIALS).exists());
        }
        assert_eq!(text(REFUSED), REFUSAL);
    }

    #[tokio::test]
    async fn a_completed_login_moves_codexs_credentials_into_the_home() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let prompt = capture(&directory, "prompt", SUCCESS_PROMPT);
        let stderr = capture(&directory, "stderr", SUCCESS);
        let written = capture(&directory, "auth.json", WRITTEN.as_bytes());
        let record = directory.path().join("home-used");
        let codex = fake(
            &directory,
            &login(),
            &format!(
                "printf '%s' \"$CODEX_HOME\" > '{}'\ncat '{prompt}'\ncp '{written}' \"$CODEX_HOME/auth.json\"\nchmod 600 \"$CODEX_HOME/auth.json\"\ncat '{stderr}' >&2\nexit 0",
                record.display()
            ),
        );

        let Device {
            prompt,
            outcome,
            cancel,
        } = device(&codex, &home, &organization(&home))
            .await
            .expect("a prompt");
        let result = finish(outcome).await;
        drop(cancel);

        assert_eq!(prompt.user_code, "FXTR-9Z9Z9");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            fs::read_to_string(record).expect("the home codex was given"),
            staging(&home).display().to_string(),
            "codex saved its login straight into the organization's home"
        );
        assert_eq!(
            fs::read_to_string(home.join(CREDENTIALS)).expect("the promoted login"),
            WRITTEN
        );
        let mode = fs::metadata(home.join(CREDENTIALS))
            .expect("the promoted login")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!staging(&home).exists(), "the staging directory was left");
        assert!(signed_in(&home));
    }

    #[tokio::test]
    async fn a_refused_relogin_leaves_the_organizations_login_in_place() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        fs::write(home.join(CREDENTIALS), EXISTING).expect("an existing login");
        let stderr = capture(&directory, "stderr", RELOGIN_REFUSED);
        let codex = fake(
            &directory,
            &login(),
            &format!("{REVOKE}\ncat '{stderr}' >&2\nexit 1"),
        );

        let error = device(&codex, &home, &organization(&home))
            .await
            .expect_err("a refusal");

        assert!(
            matches!(&error, Error::Failed(message) if message == REFUSAL),
            "{error:?}"
        );
        assert_eq!(
            fs::read_to_string(home.join(CREDENTIALS)).ok().as_deref(),
            Some(EXISTING),
            "a refused re-login deleted the organization's login"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_cancelled_relogin_stops_codex_and_leaves_the_organizations_login_in_place() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        fs::write(home.join(CREDENTIALS), EXISTING).expect("an existing login");
        let prompt = capture(&directory, "prompt", RELOGIN_ABANDONED_PROMPT);
        let leader = directory.path().join("leader");
        let forked = directory.path().join("forked");
        let codex = fake(
            &directory,
            &login(),
            &format!(
                "{REVOKE}\necho $$ > '{}'\nsleep 60 >/dev/null 2>&1 &\necho $! > '{}'\ncat '{prompt}'\nexec sleep 60",
                leader.display(),
                forked.display()
            ),
        );

        let device = device(&codex, &home, &organization(&home))
            .await
            .expect("a prompt");
        let mode = fs::metadata(staging(&home)).map(|metadata| metadata.permissions().mode());
        device.cancel.send(()).expect("the sign-in to be running");
        let result = finish(device.outcome).await;

        assert!(result.is_err(), "a cancelled sign-in reported success");
        assert!(ended(process(&leader)).await, "codex was left running");
        assert!(
            ended(process(&forked)).await,
            "a process codex forked was left running"
        );
        assert_eq!(
            fs::read_to_string(home.join(CREDENTIALS)).ok().as_deref(),
            Some(EXISTING),
            "an abandoned re-login deleted the organization's login"
        );
        assert_eq!(
            mode.ok().map(|mode| mode & 0o777),
            Some(0o700),
            "codex did not run in a private staging directory"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn dropping_the_cancel_handle_stops_codex() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let prompt = capture(&directory, "prompt", PROMPT);
        let leader = directory.path().join("leader");
        let codex = fake(
            &directory,
            &login(),
            &format!(
                "echo $$ > '{}'\ncat '{prompt}'\nexec sleep 60",
                leader.display()
            ),
        );

        let Device {
            outcome, cancel, ..
        } = device(&codex, &home, &environment())
            .await
            .expect("a prompt");
        drop(cancel);
        let result = finish(outcome).await;

        assert!(result.is_err(), "a cancelled sign-in reported success");
        assert!(ended(process(&leader)).await, "codex was left running");
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_code_nobody_enters_expires_and_codex_is_stopped() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        fs::write(home.join(CREDENTIALS), EXISTING).expect("an existing login");
        let expiring = String::from_utf8_lossy(PROMPT)
            .replace("expires in 15 minutes", "expires in 0 minutes");
        let prompt = capture(&directory, "prompt", expiring.as_bytes());
        let leader = directory.path().join("leader");
        let forked = directory.path().join("forked");
        let codex = fake(
            &directory,
            &login(),
            &format!(
                "{REVOKE}\necho $$ > '{}'\nsleep 60 >/dev/null 2>&1 &\necho $! > '{}'\ncat '{prompt}'\nexec sleep 60",
                leader.display(),
                forked.display()
            ),
        );
        let limits = Limits {
            grace: Duration::from_millis(200),
            ..Limits::default()
        };

        let Device {
            outcome, cancel, ..
        } = device_with(&codex, &home, &organization(&home), limits)
            .await
            .expect("a prompt");
        let result = finish(outcome).await;
        drop(cancel);

        assert!(matches!(result, Err(Error::Expired)), "{result:?}");
        assert!(ended(process(&leader)).await, "codex was left running");
        assert!(
            ended(process(&forked)).await,
            "a process codex forked was left running"
        );
        assert_eq!(
            fs::read_to_string(home.join(CREDENTIALS)).ok().as_deref(),
            Some(EXISTING),
            "an expired re-login deleted the organization's login"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_login_that_fails_after_its_prompt_reports_codexs_own_words() {
        for (prompt, failure) in [
            (POLL_FAILED_PROMPT, POLL_FAILED),
            (EXCHANGE_FAILED_PROMPT, EXCHANGE_FAILED),
        ] {
            let directory = TempDir::new().expect("a temporary directory");
            let home = home(&directory);
            let prompt = capture(&directory, "prompt", prompt);
            let stderr = capture(&directory, "stderr", failure);
            let codex = fake(
                &directory,
                &login(),
                &format!("cat '{prompt}'\ncat '{stderr}' >&2\nexit 1"),
            );

            let Device {
                outcome, cancel, ..
            } = device(&codex, &home, &environment())
                .await
                .expect("a prompt");
            let result = finish(outcome).await;
            drop(cancel);

            assert!(
                matches!(&result, Err(Error::Failed(message)) if *message == text(failure)),
                "{result:?}"
            );
            assert!(!home.join(CREDENTIALS).exists());
            assert!(!staging(&home).exists(), "the staging directory was left");
        }
    }

    #[tokio::test]
    async fn a_clean_exit_that_saved_nothing_never_promotes_a_stale_login() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        fs::create_dir(staging(&home)).expect("a staging directory left by a crash");
        fs::write(staging(&home).join(CREDENTIALS), EXISTING).expect("a stale login");
        let prompt = capture(&directory, "prompt", SUCCESS_PROMPT);
        let stderr = capture(&directory, "stderr", SUCCESS);
        let codex = fake(
            &directory,
            &login(),
            &format!("cat '{prompt}'\ncat '{stderr}' >&2\nexit 0"),
        );

        let Device {
            outcome, cancel, ..
        } = device(&codex, &home, &environment())
            .await
            .expect("a prompt");
        let result = finish(outcome).await;
        drop(cancel);

        assert!(matches!(result, Err(Error::Failed(_))), "{result:?}");
        assert!(
            !home.join(CREDENTIALS).exists(),
            "a stale login was promoted"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_codex_that_prints_no_prompt_is_stopped() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let leader = directory.path().join("leader");
        let codex = fake(
            &directory,
            &login(),
            &format!(
                "echo $$ > '{}'\necho 'Welcome to Codex'\nexec sleep 60",
                leader.display()
            ),
        );
        let limits = Limits {
            prompt: Duration::from_secs(1),
            ..Limits::default()
        };

        let error = device_with(&codex, &home, &environment(), limits)
            .await
            .expect_err("no prompt");

        assert!(
            matches!(&error, Error::Unreadable(message) if message.contains(prompt::NO_LINK)),
            "{error:?}"
        );
        assert!(ended(process(&leader)).await, "codex was left running");
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_codex_that_prints_a_line_too_long_to_be_a_prompt_is_refused() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let codex = fake(
            &directory,
            &login(),
            "head -c 5000 /dev/zero | tr '\\0' 'x'\necho\nexec sleep 60",
        );

        let error = device(&codex, &home, &environment())
            .await
            .expect_err("no prompt");

        assert!(
            matches!(&error, Error::Unreadable(message) if message.contains("longer than 4096 bytes")),
            "{error:?}"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn a_codex_that_exits_without_a_prompt_or_a_reason_says_how_it_exited() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let codex = fake(&directory, &login(), "exit 3");

        let error = device(&codex, &home, &environment())
            .await
            .expect_err("no prompt");

        assert!(
            matches!(&error, Error::Failed(message) if message.contains("exit status: 3")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_device_login_runs_with_exactly_the_given_environment() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let record = directory.path().join("environment");
        let stderr = capture(&directory, "stderr", REFUSED);
        let codex = fake(
            &directory,
            &login(),
            &format!("env > '{}'\ncat '{stderr}' >&2\nexit 1", record.display()),
        );
        let mut given = organization(&home);
        given.insert("ZONE_LOGIN_TEST".to_string(), "kept".to_string());

        let _ = device(&codex, &home, &given).await;

        let mut expected = given.clone();
        expected.insert(
            AgentKind::Codex.home().to_string(),
            staging(&home).display().to_string(),
        );
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "the test runner no longer sets CARGO_MANIFEST_DIR, so its absence below proves nothing"
        );
        assert_eq!(recorded(&record), expected);
    }

    #[tokio::test]
    async fn a_missing_codex_is_unavailable() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let missing = Path::new("/nonexistent/zone/codex");

        let error = device(missing, &home, &environment())
            .await
            .expect_err("a missing codex");

        assert!(
            matches!(&error, Error::Unavailable { executable, .. } if executable == "/nonexistent/zone/codex"),
            "{error:?}"
        );
        assert!(!staging(&home).exists(), "the staging directory was left");
    }

    #[tokio::test]
    async fn logout_runs_codex_logout_against_the_organizations_home() {
        for said in [LOGGED_OUT, SIGNED_OUT] {
            let directory = TempDir::new().expect("a temporary directory");
            let home = home(&directory);
            let record = directory.path().join("environment");
            let stderr = capture(&directory, "stderr", said);
            let codex = fake(
                &directory,
                "logout",
                &format!("env > '{}'\ncat '{stderr}' >&2\nexit 0", record.display()),
            );
            let mut given = environment();
            given.insert(
                AgentKind::Codex.home().to_string(),
                "/somewhere/else".to_string(),
            );

            let result = logout(&codex, &home, &given).await;

            assert!(result.is_ok(), "{result:?}");
            let mut expected = given.clone();
            expected.insert(
                AgentKind::Codex.home().to_string(),
                home.display().to_string(),
            );
            assert_eq!(recorded(&record), expected);
        }
    }

    #[tokio::test]
    async fn a_logout_codex_refuses_reports_codexs_own_words() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        let stderr = capture(&directory, "stderr", UNCONFIGURED);
        let codex = fake(&directory, "logout", &format!("cat '{stderr}' >&2\nexit 1"));

        let error = logout(&codex, &home, &environment())
            .await
            .expect_err("a refused logout");

        assert!(
            matches!(&error, Error::Failed(message) if *message == text(UNCONFIGURED)),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn logout_without_codex_is_unavailable() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);

        let error = logout(Path::new("/nonexistent/zone/codex"), &home, &environment())
            .await
            .expect_err("a missing codex");

        assert!(matches!(error, Error::Unavailable { .. }), "{error:?}");
    }

    #[test]
    fn only_a_regular_auth_file_is_a_login() {
        let directory = TempDir::new().expect("a temporary directory");
        let home = home(&directory);
        assert!(!signed_in(&home), "an empty home");

        fs::create_dir(home.join(CREDENTIALS)).expect("a directory in its place");
        assert!(!signed_in(&home), "a directory");
        fs::remove_dir(home.join(CREDENTIALS)).expect("the directory removed");

        let elsewhere = directory.path().join("elsewhere.json");
        fs::write(&elsewhere, EXISTING).expect("a login elsewhere");
        symlink(elsewhere, home.join(CREDENTIALS)).expect("a link in its place");
        assert!(!signed_in(&home), "a symbolic link");
        fs::remove_file(home.join(CREDENTIALS)).expect("the link removed");

        fs::write(home.join(CREDENTIALS), EXISTING).expect("a login");
        assert!(signed_in(&home), "a regular file");
    }
}
