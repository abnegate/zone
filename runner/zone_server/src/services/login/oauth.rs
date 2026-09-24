//! An organization's Claude sign-in. Zone runs the authorization code grant with PKCE itself, as
//! `claude setup-token` does, and keeps only the sealed tokens it is granted.
//!
//! claude.com hands the code back one of two ways. With a callback configured, it sends the
//! admin's browser to Zone's callback listener, which finishes the sign-in with no session to go
//! on: the state is its only authority, so the listener finishes only a state issued for that
//! flow, and only while the admin who started it still manages the organization. Otherwise, or
//! when the admin asks for it, claude.com shows the code and the admin pastes it.

use std::sync::LazyLock;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use dashmap::DashMap;
use uuid::Uuid;
use zone_core::llm::AgentKind;

use super::audit;
use super::claude::{self, Authorization, Client, Code, Flow, Redirect, Refusal, Reply, Scope};
use super::error::Error;
use super::pending::{self, Pending, WINDOW};
use crate::config::Config;
use crate::db::agent_logins::{self, Upsert};
use crate::db::ai_settings::{self, AccessError};
use crate::db::organization_members::OrgRole;
use crate::state::AppState;

const UNKNOWN: &str =
    "That code is not from a sign-in you started here, or the sign-in expired. Start again.";
const UNKNOWN_CALLBACK: &str = "Zone is not waiting for this sign-in: it expired, it already \
                                finished, or it was started to paste a code. Start again in Zone.";
const NO_CALLBACK: &str = "This server has no sign-in callback, so a Claude sign-in finishes \
                           with the code claude.com shows";
const DECLINED: &str = "Claude did not approve the sign-in. Start again.";
const SCOPE_REFUSED: &str =
    "claude.com would not grant the access Zone asked for. Try again with full access.";
const DEMOTED: &str = "Only organization admins can sign in to coding agents, and whoever \
                       started this sign-in no longer is one. Start again.";
const UNSAVED: &str = "Claude approved the sign-in, but Zone could not record it. Start again.";

/// Why an organization's last sign-in through the callback listener failed, until the next one
/// starts. The admin is watching the sign-in panel, not the tab that request answered.
static FAILURES: LazyLock<DashMap<Uuid, String>> = LazyLock::new(DashMap::new);

/// A Claude sign-in waiting for claude.com to hand back its code.
#[derive(Debug)]
pub struct Started {
    /// Where the user approves the sign-in.
    pub url: String,
    /// When Zone stops waiting for the code.
    pub expires_at: DateTime<Utc>,
    /// How the code comes back to Zone.
    pub flow: Flow,
}

/// Where claude.com sends the browser for a sign-in that asks for `flow`: the server's callback,
/// unless the admin asked to paste the code or the server has no callback.
pub fn redirect(config: &Config, flow: Option<Flow>) -> Result<Redirect, Error> {
    match (flow, config.agents.callback) {
        (Some(Flow::Paste), _) | (None, None) => Ok(Redirect::Paste),
        (Some(Flow::Loopback) | None, Some(callback)) => Ok(Redirect::Loopback(callback.port)),
        (Some(Flow::Loopback), None) => Err(Error::Invalid(NO_CALLBACK)),
    }
}

/// Starts a sign-in to `organization` that only `user` can finish, whose code claude.com hands
/// back through `redirect`.
pub fn start(
    organization: Uuid,
    user: Uuid,
    email: &str,
    scope: Scope,
    redirect: Redirect,
) -> Started {
    let Authorization {
        url,
        state,
        verifier,
        scope,
        redirect,
    } = Authorization::new(scope, redirect);
    FAILURES.remove(&organization);
    pending::hold(
        state,
        Pending {
            organization,
            user,
            email: email.to_string(),
            verifier,
            scope,
            redirect,
        },
    );
    let window = TimeDelta::from_std(WINDOW).expect("the sign-in window fits a TimeDelta");
    Started {
        url,
        expires_at: (Utc::now() + window).trunc_subsecs(0),
        flow: redirect.flow(),
    }
}

/// Finishes the sign-in `user` started to `organization` with the code Claude showed them, and
/// records it. A code is spent once it is tried, whoever tried it.
pub async fn finish(
    state: &AppState,
    organization: Uuid,
    user: Uuid,
    pasted: &str,
) -> Result<(), Error> {
    let code: Code = pasted.parse().map_err(unreadable)?;
    let pending = pending::claim(&code.state)
        .filter(|pending| pending.organization == organization && pending.user == user)
        .ok_or(Error::Invalid(UNKNOWN))?;
    complete(state, pending, &code).await
}

/// Finishes the sign-in claude.com sent a browser back to Zone's callback listener for, and says
/// which organization it signed in. Why a sign-in Zone was waiting for failed is kept for that
/// organization's status.
pub async fn receive(state: &AppState, reply: Reply) -> Result<Uuid, Error> {
    let pending = pending::claim_loopback(reply.state()).ok_or(Error::Invalid(UNKNOWN_CALLBACK))?;
    let organization = pending.organization;
    let outcome = match reply {
        Reply::Approved(code) => approve(state, pending, &code).await,
        Reply::Refused { refusal, .. } => Err(Error::Invalid(match refusal {
            Refusal::Declined => DECLINED,
            Refusal::Scope => SCOPE_REFUSED,
        })),
    };
    match &outcome {
        Ok(()) => {
            FAILURES.remove(&organization);
        }
        Err(error) => {
            tracing::warn!(%organization, %error, "A Claude sign-in returned to Zone did not finish");
            FAILURES.insert(organization, shown(error));
        }
    }
    outcome.map(|()| organization)
}

/// Why the organization's last sign-in through the callback listener failed, until another one
/// starts.
pub fn failure(organization: Uuid) -> Option<String> {
    FAILURES
        .get(&organization)
        .map(|failure| failure.value().clone())
}

/// Finishes a sign-in the callback listener received, once the admin who started it is found to
/// manage the organization still.
async fn approve(state: &AppState, pending: Pending, code: &Code) -> Result<(), Error> {
    let mut connection = state.db().acquire().await?;
    let access = ai_settings::authorize_organization(
        &mut connection,
        pending.organization,
        pending.user,
        OrgRole::Admin,
    )
    .await;
    drop(connection);
    match access {
        Ok(()) => complete(state, pending, code).await,
        Err(AccessError::Forbidden(_) | AccessError::NotFound(_)) => Err(Error::Forbidden(DEMOTED)),
        Err(AccessError::Invalid(message)) => Err(Error::Internal(message)),
        Err(AccessError::Database(error)) => Err(Error::Database(error)),
    }
}

/// Exchanges `code` at the redirect its sign-in started with, and records the sealed tokens.
async fn complete(state: &AppState, pending: Pending, code: &Code) -> Result<(), Error> {
    let endpoint = claude::token_endpoint(&state.config().agents.claude_token_url)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let tokens = Client::new(endpoint)
        .exchange(code, &pending.verifier, pending.scope, pending.redirect)
        .await
        .map_err(|error| Error::Refused(error.to_string()))?;
    let sealed = tokens
        .seal(state.encryption_key())
        .map_err(|error| Error::Internal(error.to_string()))?;
    let label = tokens.label();
    let login = agent_logins::upsert(
        state.db(),
        &Upsert {
            organization_id: pending.organization,
            agent: AgentKind::Claude.as_str(),
            credential: Some(&sealed),
            label: label.as_deref(),
            expires_at: Some(tokens.expires_at),
        },
    )
    .await?;
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

/// What the sign-in panel says about a failure. Zone's own failures name only that it failed; the
/// server's log has why.
fn shown(error: &Error) -> String {
    match error {
        Error::Internal(_) | Error::Database(_) => UNSAVED.to_string(),
        error => error.to_string(),
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
    use std::net::{IpAddr, Ipv4Addr};

    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use reqwest::Url;
    use sha2::{Digest, Sha256};
    use zone_core::secret::SecretValue;

    use super::*;
    use crate::config::{AgentConfig, Callback};
    use crate::services::login::claude::REDIRECT_URL;

    const EMAIL: &str = "admin@example.com";
    const PORT: u16 = 54_545;

    fn parameter(url: &str, name: &str) -> String {
        Url::parse(url)
            .expect("an absolute URL")
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap_or_else(|| panic!("{url} has no {name}"))
    }

    fn configured(callback: Option<Callback>) -> Config {
        Config {
            agents: AgentConfig {
                callback,
                ..AgentConfig::default()
            },
            ..crate::state::test_config()
        }
    }

    fn callback() -> Option<Callback> {
        Some(Callback {
            port: PORT,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        })
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

    #[test]
    fn a_sign_in_returns_to_the_callback_when_there_is_one_and_is_pasted_otherwise() {
        let loopback = Redirect::Loopback(PORT);
        for (callback, flow, expected) in [
            (callback(), None, Ok(loopback)),
            (callback(), Some(Flow::Loopback), Ok(loopback)),
            (callback(), Some(Flow::Paste), Ok(Redirect::Paste)),
            (None, None, Ok(Redirect::Paste)),
            (None, Some(Flow::Paste), Ok(Redirect::Paste)),
            (None, Some(Flow::Loopback), Err(NO_CALLBACK)),
        ] {
            let chosen = redirect(&configured(callback), flow).map_err(|error| match error {
                Error::Invalid(message) => message,
                error => panic!("{error:?}"),
            });

            assert_eq!(chosen, expected, "{callback:?} {flow:?}");
        }
    }

    #[test]
    fn a_started_sign_in_is_held_for_whoever_started_it_with_the_verifier_its_link_challenges() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let before = Utc::now();

        let started = start(organization, user, EMAIL, Scope::Full, Redirect::Paste);

        let held = pending::claim(&parameter(&started.url, "state")).expect("a held sign-in");
        assert_eq!(
            (
                held.organization,
                held.user,
                held.email.as_str(),
                held.scope
            ),
            (organization, user, EMAIL, Scope::Full)
        );
        assert_eq!(held.redirect, Redirect::Paste);
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
    }

    #[test]
    fn a_loopback_sign_in_is_held_with_the_redirect_its_link_carries() {
        let started = start(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EMAIL,
            Scope::Inference,
            Redirect::Loopback(PORT),
        );

        assert_eq!(started.flow, Flow::Loopback);
        assert_eq!(
            parameter(&started.url, "redirect_uri"),
            "http://localhost:54545/callback"
        );
        let held = pending::claim_loopback(&parameter(&started.url, "state"))
            .expect("a loopback sign-in the callback can finish");
        assert_eq!(held.redirect, Redirect::Loopback(PORT));
    }

    #[tokio::test]
    async fn a_paste_that_is_no_code_is_refused_before_any_sign_in_is_spent() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let started = start(organization, user, EMAIL, Scope::Inference, Redirect::Paste);
        let state = parameter(&started.url, "state");

        let error = finish(&AppState::for_tests(), organization, user, "no-separator")
            .await
            .expect_err("a paste with no state");

        assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
        assert!(
            pending::claim(&state).is_some(),
            "a malformed paste spent the sign-in"
        );
    }

    #[tokio::test]
    async fn a_code_tried_by_someone_else_is_refused_and_spent() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let state = parameter(
            &start(organization, user, EMAIL, Scope::Inference, Redirect::Paste).url,
            "state",
        );
        let pasted = format!("fake-code#{state}");

        for (organization, user) in [(organization, Uuid::new_v4()), (Uuid::new_v4(), user)] {
            let error = finish(&AppState::for_tests(), organization, user, &pasted)
                .await
                .expect_err("someone else's sign-in");

            assert!(matches!(error, Error::Invalid(UNKNOWN)), "{error:?}");
        }
        assert!(
            pending::claim(&state).is_none(),
            "a code tried by someone else stayed usable"
        );
    }

    #[tokio::test]
    async fn the_callback_refuses_a_state_it_was_never_given_and_blames_no_organization() {
        let organization = Uuid::new_v4();
        FAILURES.insert(organization, "an earlier failure".to_string());

        let error = receive(&AppState::for_tests(), approved("never-issued"))
            .await
            .expect_err("a state Zone never issued");

        assert!(
            matches!(error, Error::Invalid(UNKNOWN_CALLBACK)),
            "{error:?}"
        );
        assert_eq!(
            failure(organization).as_deref(),
            Some("an earlier failure"),
            "a stranger's callback changed an organization's status"
        );
    }

    #[tokio::test]
    async fn the_callback_leaves_a_sign_in_started_for_pasting_to_its_admin() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let state = parameter(
            &start(organization, user, EMAIL, Scope::Inference, Redirect::Paste).url,
            "state",
        );

        let error = receive(&AppState::for_tests(), approved(&state))
            .await
            .expect_err("a state issued for pasting");

        assert!(
            matches!(error, Error::Invalid(UNKNOWN_CALLBACK)),
            "{error:?}"
        );
        assert_eq!(failure(organization), None);
        assert!(
            pending::claim(&state).is_some(),
            "the callback spent a sign-in that was waiting for its pasted code"
        );
    }

    #[tokio::test]
    async fn a_refusal_ends_the_sign_in_and_says_why_until_the_next_one_starts() {
        for (refusal, reason) in [
            (Refusal::Declined, DECLINED),
            (Refusal::Scope, SCOPE_REFUSED),
        ] {
            let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
            let loopback = Redirect::Loopback(PORT);
            let state = parameter(
                &start(organization, user, EMAIL, Scope::Inference, loopback).url,
                "state",
            );

            let error = receive(&AppState::for_tests(), refused(&state, refusal))
                .await
                .expect_err("a sign-in claude.com did not approve");

            assert!(
                matches!(error, Error::Invalid(message) if message == reason),
                "{error:?}"
            );
            assert_eq!(failure(organization).as_deref(), Some(reason));
            assert!(
                pending::claim(&state).is_none(),
                "a refused sign-in stayed open"
            );

            start(organization, user, EMAIL, Scope::Full, loopback);
            assert_eq!(
                failure(organization),
                None,
                "a new sign-in still showed why the last one failed"
            );
        }
    }

    #[test]
    fn the_panel_is_told_that_zone_failed_and_not_how() {
        for error in [
            Error::Internal("/app/agent-state is read-only".to_string()),
            Error::Database(sqlx::Error::PoolTimedOut),
        ] {
            assert_eq!(shown(&error), UNSAVED, "{error:?}");
        }
        assert_eq!(
            shown(&Error::Refused(
                "Claude refused the sign-in (HTTP 400): Invalid code".to_string()
            )),
            "Claude refused the sign-in (HTTP 400): Invalid code"
        );
        assert_eq!(shown(&Error::Forbidden(DEMOTED)), DEMOTED);
    }
}
