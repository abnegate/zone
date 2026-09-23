//! An organization's Claude sign-in. Zone runs the authorization code grant with PKCE itself, as
//! `claude setup-token` does, and keeps only the sealed tokens it is granted.

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use uuid::Uuid;
use zone_core::llm::AgentKind;

use super::audit;
use super::claude::{self, Authorization, Client, Code, Scope};
use super::error::Error;
use super::pending::{self, Pending, WINDOW};
use crate::db::agent_logins::{self, Upsert};
use crate::state::AppState;

const UNKNOWN: &str =
    "That code is not from a sign-in you started here, or the sign-in expired. Start again.";

/// A Claude sign-in waiting for the code Claude shows once the user approves it.
#[derive(Debug)]
pub struct Started {
    /// Where the user approves the sign-in.
    pub url: String,
    /// When Zone stops waiting for the code.
    pub expires_at: DateTime<Utc>,
}

/// Starts a sign-in to `organization` that only `user` can finish.
pub fn start(organization: Uuid, user: Uuid, scope: Scope) -> Started {
    let Authorization {
        url,
        state,
        verifier,
        scope,
    } = Authorization::new(scope);
    pending::hold(
        state,
        Pending {
            organization,
            user,
            verifier,
            scope,
        },
    );
    let window = TimeDelta::from_std(WINDOW).expect("the sign-in window fits a TimeDelta");
    Started {
        url,
        expires_at: (Utc::now() + window).trunc_subsecs(0),
    }
}

/// Finishes the sign-in `user` started to `organization` with the code Claude showed them, and
/// records it. A code is spent once it is tried, whoever tried it.
pub async fn finish(
    state: &AppState,
    organization: Uuid,
    user: Uuid,
    email: &str,
    pasted: &str,
) -> Result<(), Error> {
    let code: Code = pasted.parse().map_err(unreadable)?;
    let pending = pending::claim(&code.state)
        .filter(|pending| pending.organization == organization && pending.user == user)
        .ok_or(Error::Invalid(UNKNOWN))?;
    let endpoint = claude::token_endpoint(&state.config().agents.claude_token_url)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let tokens = Client::new(endpoint)
        .exchange(&code, &pending.verifier, pending.scope)
        .await
        .map_err(|error| Error::Refused(error.to_string()))?;
    let sealed = tokens
        .seal(state.encryption_key())
        .map_err(|error| Error::Internal(error.to_string()))?;
    let label = tokens.label();
    let login = agent_logins::upsert(
        state.db(),
        &Upsert {
            organization_id: organization,
            agent: AgentKind::Claude.as_str(),
            credential: Some(&sealed),
            label: label.as_deref(),
            expires_at: Some(tokens.expires_at),
        },
    )
    .await?;
    audit::signed_in(state.db(), organization, user, email, &login).await;
    Ok(())
}

fn unreadable(error: claude::Error) -> Error {
    match error {
        claude::Error::Malformed(reason) => Error::Unreadable(reason),
        error => Error::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use reqwest::Url;
    use sha2::{Digest, Sha256};

    use super::*;

    fn parameter(url: &str, name: &str) -> String {
        Url::parse(url)
            .expect("an absolute URL")
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap_or_else(|| panic!("{url} has no {name}"))
    }

    #[test]
    fn a_started_sign_in_is_held_for_whoever_started_it_with_the_verifier_its_link_challenges() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let before = Utc::now();

        let started = start(organization, user, Scope::Full);

        let held = pending::claim(&parameter(&started.url, "state")).expect("a held sign-in");
        assert_eq!(
            (held.organization, held.user, held.scope),
            (organization, user, Scope::Full)
        );
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

    #[tokio::test]
    async fn a_paste_that_is_no_code_is_refused_before_any_sign_in_is_spent() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let started = start(organization, user, Scope::Inference);
        let state = parameter(&started.url, "state");

        let error = finish(
            &AppState::for_tests(),
            organization,
            user,
            "",
            "no-separator",
        )
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
        let state = parameter(&start(organization, user, Scope::Inference).url, "state");
        let pasted = format!("fake-code#{state}");

        for (organization, user) in [(organization, Uuid::new_v4()), (Uuid::new_v4(), user)] {
            let error = finish(&AppState::for_tests(), organization, user, "", &pasted)
                .await
                .expect_err("someone else's sign-in");

            assert!(matches!(error, Error::Invalid(UNKNOWN)), "{error:?}");
        }
        assert!(
            pending::claim(&state).is_none(),
            "a code tried by someone else stayed usable"
        );
    }
}
