//! An organization's Claude sign-in, from its authorize link to the sealed tokens Zone keeps.

use std::future::Future;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use sqlx::PgConnection;
use uuid::Uuid;
use zone_core::llm::AgentKind;

use super::attempts::{self, Attempt};
use super::caller::Caller;
use super::claude::{self, Authorization, Client, Code, Flow, Redirect, Reply, Scope, Tokens};
use super::console::Console;
use super::error::Error;
use super::pending::{self, Pending, WINDOW};
use super::{audit, devices, receipts};
use crate::config::Config;
use crate::db::agent_logins::{self, AgentLoginRow, Upsert};
use crate::db::ai_settings::{self, AccessError};
use crate::db::organization_members::OrgRole;
use crate::db::{organizations, sessions};
use crate::state::AppState;

const UNKNOWN: &str =
    "That code is not from a sign-in you started here, or the sign-in expired. Start again.";
const UNKNOWN_CALLBACK: &str = "Zone is not waiting for this sign-in: it expired, it already \
                                finished, or it was started to paste a code. Start again in Zone.";
const UNKNOWN_RECEIPT: &str =
    "Zone is not waiting for this sign-in: it expired, or it already finished. Start again.";
const NO_CALLBACK: &str = "This server has no sign-in callback, so a Claude sign-in finishes \
                           with the code claude.com shows";
const NOT_LOCAL: &str = "claude.com can send a sign-in back to Zone only when this browser runs \
                         on the machine Zone runs on and opens Zone at a localhost address the \
                         server lists in ZONE_CONSOLE_ORIGINS. Paste the code instead.";
const ENDED: &str = "This sign-in was cancelled, or another one started after it.";
const DEMOTED: &str = "Only organization admins can sign in to coding agents, and whoever \
                       started this sign-in no longer is one. Start again.";
const SESSION_ENDED: &str =
    "The Zone session that started this sign-in has ended. Sign in to Zone and start again.";
const STARTED_ELSEWHERE: &str = "Someone else started this Claude sign-in, or it was started in \
                                 another browser, so Zone did not finish it.";
const RETURNED_ELSEWHERE: &str = "This sign-in came back to someone else, or to another browser, \
                                  so Zone did not finish it. Start again, and approve it in this \
                                  browser.";
const STOPPED: &str = "The Claude sign-in stopped before it finished";

/// A Claude sign-in waiting for claude.com to hand back its code.
#[derive(Debug)]
pub struct Started {
    /// Where the user approves the sign-in.
    pub url: String,
    /// When Zone stops waiting for the code.
    pub expires_at: DateTime<Utc>,
    /// How the code comes back to Zone.
    pub flow: Flow,
    /// Names the sign-in when its panel asks how it went.
    pub attempt: Uuid,
}

/// Where claude.com sends the browser for a sign-in that asks for `flow`, and for one sent to the
/// server's callback, the console at `origin` the callback then returns the browser to. A sign-in
/// uses the callback when the server has one and was started from a console the operator listed,
/// on this machine, unless the admin asked to paste the code.
pub fn redirect(
    config: &Config,
    flow: Option<Flow>,
    origin: Option<&str>,
) -> Result<(Redirect, Option<Console>), Error> {
    let console = origin.and_then(|origin| Console::at(origin, &config.agents.consoles));
    match (flow, config.agents.callback, console) {
        (Some(Flow::Paste), _, _) | (None, None, _) | (None, Some(_), None) => {
            Ok((Redirect::Paste, None))
        }
        (Some(Flow::Loopback), None, _) => Err(Error::Invalid(NO_CALLBACK)),
        (Some(Flow::Loopback), Some(_), None) => Err(Error::Invalid(NOT_LOCAL)),
        (Some(Flow::Loopback) | None, Some(callback), Some(console)) => {
            Ok((Redirect::Loopback(callback.port), Some(console)))
        }
    }
}

/// Starts a sign-in to `organization` that only `caller` can finish, whose code claude.com hands
/// back through `redirect`. It ends the caller's earlier sign-in to the organization.
pub async fn start(
    organization: Uuid,
    caller: &Caller,
    scope: Scope,
    redirect: Redirect,
    console: Option<Console>,
) -> Started {
    let Authorization {
        url,
        state,
        verifier,
        scope,
        redirect,
    } = Authorization::new(scope, redirect);
    let attempt = Attempt {
        id: Uuid::new_v4(),
        organization,
        user: caller.user,
    };
    let _guard = devices::lock(organization).await;
    attempts::begin(attempt);
    receipts::cancel(organization, caller.user);
    pending::hold(
        state,
        Pending {
            organization,
            user: caller.user,
            email: caller.email.clone(),
            session: caller.session,
            attempt: attempt.id,
            verifier,
            scope,
            redirect,
            console,
        },
    );
    let window = TimeDelta::from_std(WINDOW).expect("the sign-in window fits a TimeDelta");
    Started {
        url,
        expires_at: (Utc::now() + window).trunc_subsecs(0),
        flow: redirect.flow(),
        attempt: attempt.id,
    }
}

/// Finishes the sign-in `user` started to `organization` with the code claude.com showed them,
/// and records it. A code is spent once it is tried, whoever tried it.
pub async fn finish(
    state: &AppState,
    organization: Uuid,
    user: Uuid,
    pasted: &str,
) -> Result<(), Error> {
    let code: Code = pasted.parse().map_err(unreadable)?;
    let pending = pending::claim(&code.state).ok_or(Error::Invalid(UNKNOWN))?;
    if (pending.organization, pending.user) != (organization, user) {
        let _ = tokio::spawn(abandon(pending.organization, pending.attempt)).await;
        return Err(Error::Invalid(UNKNOWN));
    }
    detached(complete(state.clone(), pending, code)).await
}

/// Parks the code claude.com sent a browser back to Zone's callback listener with, and names
/// where that browser goes next: the console that started the sign-in, which hands the receipt
/// back. A sign-in claude.com did not approve ends, and says why.
pub async fn receive(reply: Reply) -> Result<String, Error> {
    detached(park(reply)).await
}

/// Finishes the sign-in parked under `receipt` when `caller` started it to `organization`, in
/// this session. Anyone else only spends the receipt, and the code is discarded.
pub async fn redeem(
    state: &AppState,
    organization: Uuid,
    caller: &Caller,
    receipt: &str,
) -> Result<(), Error> {
    let (pending, code) = receipts::take(receipt).ok_or(Error::Invalid(UNKNOWN_RECEIPT))?;
    let started = (pending.organization, pending.user, pending.session)
        == (organization, caller.user, caller.session);
    if !started {
        drop(code);
        tracing::warn!(
            organization = %pending.organization,
            "A Claude sign-in came back to a browser that did not start it; its code was discarded"
        );
        let _ = tokio::spawn(abandon(pending.organization, pending.attempt)).await;
        return Err(Error::Forbidden(STARTED_ELSEWHERE));
    }
    detached(complete(state.clone(), pending, code)).await
}

/// Ends `user`'s Claude sign-in to `organization`, wherever its code is.
pub async fn cancel(organization: Uuid, user: Uuid) {
    let _guard = devices::lock(organization).await;
    pending::cancel(organization, user);
    receipts::cancel(organization, user);
    attempts::cancel(organization, user);
}

/// Ends every Claude sign-in to `organization`, wherever its code is, and forgets why any failed.
/// Its caller holds the organization's lock.
pub fn forget(organization: Uuid) {
    pending::forget(organization);
    receipts::forget(organization);
    attempts::forget(organization);
}

/// Why `user`'s sign-in `attempt` to `organization` failed, once it has.
pub fn failure(attempt: Uuid, organization: Uuid, user: Uuid) -> Option<String> {
    attempts::failure(attempt, organization, user)
}

async fn park(reply: Reply) -> Result<String, Error> {
    let (pending, expires) =
        pending::claim_loopback(reply.state()).ok_or(Error::Invalid(UNKNOWN_CALLBACK))?;
    let _guard = devices::lock(pending.organization).await;
    let Some(console) = pending
        .console
        .clone()
        .filter(|_| attempts::live(pending.attempt))
    else {
        return Err(Error::Invalid(UNKNOWN_CALLBACK));
    };
    match reply {
        Reply::Approved(code) => {
            let organization = pending.organization;
            let receipt = receipts::park(pending, code, expires);
            tracing::info!(%organization, "claude.com sent a Claude sign-in back to the callback");
            Ok(console.receipt(&receipt, organization))
        }
        Reply::Refused { refusal, .. } => {
            tracing::info!(
                organization = %pending.organization,
                ?refusal,
                "claude.com did not approve a Claude sign-in"
            );
            attempts::fail(pending.attempt, refusal.explained().to_string());
            Err(Error::Invalid(refusal.explained()))
        }
    }
}

/// Exchanges `code` for the tokens of the sign-in it answers and records them, unless the
/// sign-in ended meanwhile or whoever started it may no longer finish it. Why a sign-in that had
/// not ended failed is kept for its panel.
async fn complete(state: AppState, pending: Pending, code: Code) -> Result<(), Error> {
    let granted = grant(&state, &pending, &code).await;
    let _guard = devices::lock(pending.organization).await;
    if !attempts::live(pending.attempt) {
        return Err(Error::Invalid(ENDED));
    }
    let recorded = match granted {
        Ok(tokens) => record(&state, &pending, &tokens).await,
        Err(error) => Err(error),
    };
    match recorded {
        Ok(login) => {
            attempts::end(pending.attempt);
            audit::signed_in(
                state.db(),
                pending.organization,
                pending.user,
                &pending.email,
                &login,
            )
            .await;
            Ok(())
        }
        Err(error) => {
            report(pending.organization, &error);
            attempts::fail(pending.attempt, error.shown().into_owned());
            Err(error)
        }
    }
}

/// The tokens claude.com grants for `code` at the redirect its sign-in started with, asked for
/// only while whoever started the sign-in manages the organization still.
async fn grant(state: &AppState, pending: &Pending, code: &Code) -> Result<Tokens, Error> {
    if !attempts::claim(pending.attempt) {
        return Err(Error::Invalid(ENDED));
    }
    let mut connection = state.db().acquire().await?;
    authorize(state, &mut connection, pending).await?;
    drop(connection);
    let endpoint = claude::token_endpoint(&state.config().agents.claude_token_url)
        .map_err(|error| Error::Internal(error.to_string()))?;
    Client::new(endpoint)
        .exchange(code, &pending.verifier, pending.scope, pending.redirect)
        .await
        .map_err(|error| match error {
            claude::Error::Transport(_) => Error::Unreachable(error.to_string()),
            claude::Error::Sealing => Error::Internal(error.to_string()),
            claude::Error::Malformed(_) | claude::Error::Rejected { .. } => {
                Error::Refused(error.to_string())
            }
        })
}

/// Seals `tokens` as the organization's Claude login, checking again, in the transaction that
/// records it, that whoever started the sign-in may finish it.
async fn record(
    state: &AppState,
    pending: &Pending,
    tokens: &Tokens,
) -> Result<AgentLoginRow, Error> {
    let sealed = tokens
        .seal(state.encryption_key())
        .map_err(|error| Error::Internal(error.to_string()))?;
    let label = tokens.label();
    let mut transaction = state.db().begin().await?;
    authorize(state, &mut transaction, pending).await?;
    let login = agent_logins::upsert(
        &mut *transaction,
        &Upsert {
            organization_id: pending.organization,
            agent: AgentKind::Claude.as_str(),
            credential: Some(&sealed),
            label: label.as_deref(),
            expires_at: Some(tokens.expires_at),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(login)
}

/// Whether whoever started the sign-in still manages the organization, in the session they
/// started it in. Their role stays locked until `connection`'s transaction ends.
async fn authorize(
    state: &AppState,
    connection: &mut PgConnection,
    pending: &Pending,
) -> Result<(), Error> {
    let access = ai_settings::authorize_organization(
        &mut *connection,
        pending.organization,
        pending.user,
        OrgRole::Admin,
    )
    .await;
    match access {
        Ok(()) => {}
        Err(AccessError::Forbidden(_)) => return Err(Error::Forbidden(DEMOTED)),
        Err(AccessError::NotFound(_)) => {
            return match organizations::get_organization(state.db(), pending.organization).await? {
                Some(_) => Err(Error::Forbidden(DEMOTED)),
                None => Err(Error::Deleted),
            };
        }
        Err(AccessError::Invalid(message)) => return Err(Error::Internal(message)),
        Err(AccessError::Database(error)) => return Err(Error::Database(error)),
    }
    if sessions::is_active_user_session(&mut *connection, pending.session, pending.user).await? {
        Ok(())
    } else {
        Err(Error::Forbidden(SESSION_ENDED))
    }
}

/// Ends the attempt of a sign-in whose code came back to someone else, and keeps that as why.
async fn abandon(organization: Uuid, attempt: Uuid) {
    let _guard = devices::lock(organization).await;
    attempts::fail(attempt, RETURNED_ELSEWHERE.to_string());
}

/// Runs `work` to its end even when the request waiting on it goes away, so a sign-in whose code
/// was claimed is always recorded and audited, or why it failed kept.
async fn detached<T: Send + 'static>(
    work: impl Future<Output = Result<T, Error>> + Send + 'static,
) -> Result<T, Error> {
    tokio::spawn(work)
        .await
        .unwrap_or_else(|error| Err(Error::Internal(format!("{STOPPED}: {error}"))))
}

fn report(organization: Uuid, error: &Error) {
    match error {
        Error::Unreachable(reason) => {
            tracing::warn!(%organization, %reason, "Could not reach claude.com to finish a Claude sign-in");
        }
        error if error.internal() => {
            tracing::error!(%organization, %error, "Zone could not finish a Claude sign-in");
        }
        error => {
            tracing::info!(%organization, reason = %error, "A Claude sign-in did not finish");
        }
    }
}

fn unreadable(error: claude::Error) -> Error {
    match error {
        claude::Error::Malformed(reason) => Error::Unreadable(reason),
        error => Error::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use reqwest::Url;
    use sha2::{Digest, Sha256};
    use zone_core::secret::SecretValue;

    use super::*;
    use crate::config::{AgentConfig, Callback};
    use crate::services::login::claude::{REDIRECT_URL, Refusal};
    use crate::services::login::console::{ORGANIZATION, RECEIPT, handed};

    const EMAIL: &str = "admin@example.com";
    const PORT: u16 = 54_545;
    const CONSOLE: &str = "http://localhost:3000";
    /// A console the operator listed that runs on another machine.
    const REMOTE: &str = "https://zone.example.com";

    fn parameter(url: &str, name: &str) -> String {
        Url::parse(url)
            .expect("an absolute URL")
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap_or_else(|| panic!("{url} has no {name}"))
    }

    fn configured(callback: Option<Callback>, consoles: &[&str]) -> Config {
        Config {
            agents: AgentConfig {
                callback,
                consoles: consoles.iter().map(|console| console.to_string()).collect(),
                ..AgentConfig::default()
            },
            ..crate::state::test_config()
        }
    }

    fn callback() -> Option<Callback> {
        Some(Callback {
            port: PORT,
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), PORT),
        })
    }

    fn caller() -> Caller {
        Caller {
            user: Uuid::new_v4(),
            email: EMAIL.to_string(),
            session: Uuid::new_v4(),
        }
    }

    fn console() -> Option<Console> {
        Console::at(CONSOLE, &[CONSOLE.to_string()])
    }

    async fn looping(organization: Uuid, caller: &Caller) -> Started {
        start(
            organization,
            caller,
            Scope::Inference,
            Redirect::Loopback(PORT),
            console(),
        )
        .await
    }

    async fn pasting(organization: Uuid, caller: &Caller) -> Started {
        start(
            organization,
            caller,
            Scope::Inference,
            Redirect::Paste,
            None,
        )
        .await
    }

    fn state_of(started: &Started) -> String {
        parameter(&started.url, "state")
    }

    fn approved(state: &str) -> Reply {
        Reply::Approved(Code {
            value: SecretValue::new("fake-code"),
            state: state.to_string(),
        })
    }

    fn refused(state: &str, refusal: Refusal) -> Reply {
        Reply::Refused {
            state: state.to_string(),
            refusal,
        }
    }

    fn invalid(error: Error) -> &'static str {
        match error {
            Error::Invalid(message) => message,
            error => panic!("{error:?}"),
        }
    }

    #[test]
    fn a_sign_in_returns_to_the_callback_only_from_a_listed_console_on_this_machine() {
        let loopback = Ok((Redirect::Loopback(PORT), console()));
        let paste = Ok((Redirect::Paste, None));
        for (callback, flow, origin, expected) in [
            (callback(), None, Some(CONSOLE), loopback.clone()),
            (callback(), Some(Flow::Loopback), Some(CONSOLE), loopback),
            (callback(), Some(Flow::Paste), Some(CONSOLE), paste.clone()),
            (callback(), None, Some(REMOTE), paste.clone()),
            (
                callback(),
                None,
                Some("http://localhost:9999"),
                paste.clone(),
            ),
            (
                callback(),
                None,
                Some("http://evil.localhost"),
                paste.clone(),
            ),
            (
                callback(),
                None,
                Some("http://127.0.0.1:53123"),
                paste.clone(),
            ),
            (callback(), None, None, paste.clone()),
            (
                callback(),
                Some(Flow::Loopback),
                Some(REMOTE),
                Err(NOT_LOCAL),
            ),
            (
                callback(),
                Some(Flow::Loopback),
                Some("http://localhost:9999"),
                Err(NOT_LOCAL),
            ),
            (callback(), Some(Flow::Loopback), None, Err(NOT_LOCAL)),
            (None, None, Some(CONSOLE), paste.clone()),
            (None, Some(Flow::Paste), None, paste.clone()),
            (None, Some(Flow::Loopback), Some(CONSOLE), Err(NO_CALLBACK)),
        ] {
            let chosen =
                redirect(&configured(callback, &[CONSOLE, REMOTE]), flow, origin).map_err(invalid);

            assert_eq!(chosen, expected, "{callback:?} {flow:?} {origin:?}");
        }

        let unlisted = configured(callback(), &[]);
        assert_eq!(
            redirect(&unlisted, None, Some(CONSOLE)).map_err(invalid),
            paste,
            "a server that lists no console sent a sign-in's browser back to one"
        );
        assert_eq!(
            redirect(&unlisted, Some(Flow::Loopback), Some(CONSOLE)).map_err(invalid),
            Err(NOT_LOCAL)
        );
    }

    #[tokio::test]
    async fn a_started_sign_in_is_held_for_whoever_started_it_with_the_verifier_its_link_challenges()
     {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let before = Utc::now();

        let started = start(organization, &caller, Scope::Full, Redirect::Paste, None).await;

        let held = pending::claim(&state_of(&started)).expect("a held sign-in");
        assert_eq!(
            (
                held.organization,
                held.user,
                held.email.as_str(),
                held.session,
                held.attempt,
                held.scope
            ),
            (
                organization,
                caller.user,
                EMAIL,
                caller.session,
                started.attempt,
                Scope::Full
            )
        );
        assert_eq!((held.redirect, held.console), (Redirect::Paste, None));
        assert_eq!(started.flow, Flow::Paste);
        assert_eq!(parameter(&started.url, "redirect_uri"), REDIRECT_URL);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(held.verifier.expose())),
            parameter(&started.url, "code_challenge")
        );
        let window = TimeDelta::minutes(10);
        assert!(
            started.expires_at >= (before + window).trunc_subsecs(0)
                && started.expires_at <= Utc::now() + window,
            "{}",
            started.expires_at
        );
        assert_eq!(started.expires_at.timestamp_subsec_nanos(), 0);
        assert!(attempts::live(started.attempt));
    }

    #[tokio::test]
    async fn a_loopback_sign_in_is_held_with_the_redirect_its_link_carries_and_its_console() {
        let started = looping(Uuid::new_v4(), &caller()).await;

        assert_eq!(started.flow, Flow::Loopback);
        assert_eq!(
            parameter(&started.url, "redirect_uri"),
            "http://localhost:54545/callback"
        );
        let (held, _) = pending::claim_loopback(&state_of(&started))
            .expect("a loopback sign-in the callback can finish");
        assert_eq!(
            (held.redirect, held.console),
            (Redirect::Loopback(PORT), console())
        );
    }

    #[tokio::test]
    async fn starting_again_ends_the_callers_earlier_sign_in_wherever_its_code_is() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let first = looping(organization, &caller).await;
        let receipt = receive(approved(&state_of(&first)))
            .await
            .map(|url| handed(&url, RECEIPT))
            .expect("a parked code");

        let second = looping(organization, &caller).await;

        assert!(
            !attempts::live(first.attempt),
            "the earlier sign-in could still finish"
        );
        assert!(
            receipts::take(&receipt).is_none(),
            "the earlier sign-in's code was kept"
        );
        assert!(attempts::live(second.attempt));
    }

    #[tokio::test]
    async fn a_paste_that_is_no_code_is_refused_before_any_sign_in_is_spent() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let started = pasting(organization, &caller).await;

        let error = finish(
            &AppState::for_tests(),
            organization,
            caller.user,
            "no-separator",
        )
        .await
        .expect_err("a paste with no state");

        assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
        assert!(
            pending::claim(&state_of(&started)).is_some(),
            "a malformed paste spent the sign-in"
        );
    }

    #[tokio::test]
    async fn a_code_tried_by_someone_else_is_refused_and_ends_the_sign_in() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let started = looping(organization, &caller).await;
        let pasted = format!("fake-code#{}", state_of(&started));

        for (organization, user) in [
            (organization, Uuid::new_v4()),
            (Uuid::new_v4(), caller.user),
        ] {
            let error = finish(&AppState::for_tests(), organization, user, &pasted)
                .await
                .expect_err("someone else's sign-in");

            assert_eq!(invalid(error), UNKNOWN);
        }
        assert!(
            pending::claim(&state_of(&started)).is_none(),
            "a code tried by someone else stayed usable"
        );
        assert_eq!(
            failure(started.attempt, organization, caller.user).as_deref(),
            Some(RETURNED_ELSEWHERE),
            "the admin's panel was left waiting for a sign-in that can no longer finish"
        );
    }

    #[tokio::test]
    async fn the_callback_parks_an_approved_code_and_sends_the_browser_to_its_console() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let started = looping(organization, &caller).await;

        let url = receive(approved(&state_of(&started)))
            .await
            .expect("a parked code");

        let returned = Url::parse(&url).expect("an absolute URL");
        assert_eq!(
            (returned.origin().ascii_serialization(), returned.path()),
            (CONSOLE.to_string(), "/agent-sign-in")
        );
        assert_eq!(handed(&url, ORGANIZATION), organization.to_string());
        let (parked, code) = receipts::take(&handed(&url, RECEIPT)).expect("the parked code");
        assert_eq!(
            (parked.organization, parked.user, parked.session),
            (organization, caller.user, caller.session)
        );
        assert_eq!(code.value.expose(), "fake-code");
        assert!(
            attempts::live(started.attempt),
            "parking a code ended the sign-in"
        );
        assert!(
            receive(approved(&state_of(&started))).await.is_err(),
            "a callback replayed a state that was already spent"
        );
    }

    #[tokio::test]
    async fn the_callback_refuses_a_state_it_was_never_given_and_blames_no_sign_in() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let started = looping(organization, &caller).await;

        let error = receive(approved("never-issued"))
            .await
            .expect_err("a state Zone never issued");

        assert_eq!(invalid(error), UNKNOWN_CALLBACK);
        assert!(attempts::live(started.attempt));
        assert_eq!(failure(started.attempt, organization, caller.user), None);
    }

    #[tokio::test]
    async fn the_callback_leaves_a_sign_in_started_for_pasting_to_its_admin() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let started = pasting(organization, &caller).await;

        let error = receive(approved(&state_of(&started)))
            .await
            .expect_err("a state issued for pasting");

        assert_eq!(invalid(error), UNKNOWN_CALLBACK);
        assert!(
            pending::claim(&state_of(&started)).is_some(),
            "the callback spent a sign-in that was waiting for its pasted code"
        );
    }

    #[tokio::test]
    async fn a_refusal_ends_the_sign_in_and_tells_its_own_panel_why() {
        for refusal in [
            Refusal::Declined,
            Refusal::Scope,
            Refusal::OnHold,
            Refusal::Unavailable,
        ] {
            let (organization, caller) = (Uuid::new_v4(), caller());
            let started = looping(organization, &caller).await;

            let error = receive(refused(&state_of(&started), refusal))
                .await
                .expect_err("a sign-in claude.com did not approve");

            assert_eq!(invalid(error), refusal.explained());
            assert_eq!(
                failure(started.attempt, organization, caller.user).as_deref(),
                Some(refusal.explained())
            );
            assert_eq!(
                failure(started.attempt, organization, Uuid::new_v4()),
                None,
                "someone else read why the sign-in failed"
            );
            assert!(!attempts::live(started.attempt));

            let again = looping(organization, &caller).await;
            assert_eq!(
                failure(started.attempt, organization, caller.user),
                None,
                "a new sign-in still showed why the last one failed"
            );
            assert_eq!(failure(again.attempt, organization, caller.user), None);
        }
    }

    #[tokio::test]
    async fn a_cancelled_sign_in_can_no_longer_finish_wherever_its_code_is() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        let colleague = self::caller();
        let waiting = looping(organization, &caller).await;
        let theirs = looping(organization, &colleague).await;

        cancel(organization, caller.user).await;
        assert!(
            receive(approved(&state_of(&waiting))).await.is_err(),
            "the callback finished a cancelled sign-in"
        );
        assert!(!attempts::live(waiting.attempt));

        let parked = looping(organization, &caller).await;
        let receipt = receive(approved(&state_of(&parked)))
            .await
            .map(|url| handed(&url, RECEIPT))
            .expect("a parked code");
        cancel(organization, caller.user).await;
        let error = redeem(&AppState::for_tests(), organization, &caller, &receipt)
            .await
            .expect_err("a cancelled sign-in's receipt");
        assert_eq!(invalid(error), UNKNOWN_RECEIPT);

        assert!(
            attempts::live(theirs.attempt),
            "cancelling ended a colleague's sign-in"
        );
    }

    #[tokio::test]
    async fn a_receipt_handed_in_by_anyone_else_spends_it_and_discards_the_code() {
        let (organization, caller) = (Uuid::new_v4(), caller());
        for (path, stranger) in [
            (organization, self::caller()),
            (
                organization,
                Caller {
                    session: Uuid::new_v4(),
                    ..caller.clone()
                },
            ),
            (Uuid::new_v4(), caller.clone()),
        ] {
            let started = looping(organization, &caller).await;
            let receipt = receive(approved(&state_of(&started)))
                .await
                .map(|url| handed(&url, RECEIPT))
                .expect("a parked code");

            let error = redeem(&AppState::for_tests(), path, &stranger, &receipt)
                .await
                .expect_err("a receipt handed in by someone else");

            assert!(
                matches!(error, Error::Forbidden(STARTED_ELSEWHERE)),
                "{error:?}"
            );
            assert!(
                receipts::take(&receipt).is_none(),
                "a receipt handed in by someone else stayed usable"
            );
            assert_eq!(
                failure(started.attempt, organization, caller.user).as_deref(),
                Some(RETURNED_ELSEWHERE),
                "the admin's panel was left waiting for a sign-in that can no longer finish"
            );
        }
    }

    #[tokio::test]
    async fn a_receipt_zone_never_gave_finishes_nothing() {
        let error = redeem(
            &AppState::for_tests(),
            Uuid::new_v4(),
            &caller(),
            "never-issued",
        )
        .await
        .expect_err("a receipt Zone never gave");

        assert_eq!(invalid(error), UNKNOWN_RECEIPT);
    }

    #[tokio::test]
    async fn forgetting_an_organization_ends_its_sign_ins_wherever_their_codes_are() {
        let (organization, caller, colleague) = (Uuid::new_v4(), caller(), caller());
        let waiting = looping(organization, &colleague).await;
        let parked = looping(organization, &caller).await;
        let receipt = receive(approved(&state_of(&parked)))
            .await
            .map(|url| handed(&url, RECEIPT))
            .expect("a parked code");
        let elsewhere = looping(Uuid::new_v4(), &caller).await;

        forget(organization);

        assert!(
            pending::claim_loopback(&state_of(&waiting)).is_none(),
            "a forgotten organization's sign-in could still finish"
        );
        assert!(receipts::take(&receipt).is_none());
        assert!(!attempts::live(waiting.attempt) && !attempts::live(parked.attempt));
        assert!(
            pending::claim_loopback(&state_of(&elsewhere)).is_some(),
            "another organization's sign-in was dropped"
        );
    }
}
