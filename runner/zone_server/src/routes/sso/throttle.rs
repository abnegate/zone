//! Limits single sign-on starts and SAML posts per client address before a body is read or a
//! provider looked up, so a flood from one network can't hold every response check.

use std::time::Instant;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::header::RETRY_AFTER;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use zone_enterprise::sso::RATE_LIMITED;

use super::saml::{redirect, refused};
use crate::auth::client_address::ClientAddress;
use crate::routes::common::ErrorResponse;
use crate::services::sso::console::redirect_uri;
use crate::services::sso::throttle::WINDOW;
use crate::state::AppState;

pub const RATE_LIMITED_MESSAGE: &str =
    "Too many sign-in attempts from your network. Wait a minute, then try again.";

pub async fn authorizations(
    State(state): State<AppState>,
    address: ClientAddress,
    request: Request,
    next: Next,
) -> Response {
    let decision = state.sso_throttle().check(address.0);
    if decision.allowed {
        return next.run(request).await;
    }
    tracing::debug!(address = ?address.0, "Refused a single sign-on start over the limit");
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(RETRY_AFTER, retry_after(decision.reset_at, Instant::now()))],
        Json(ErrorResponse::with_code(RATE_LIMITED_MESSAGE, RATE_LIMITED)),
    )
        .into_response()
}

/// The browser is sent back without a state, because the post's `RelayState` is never read.
pub async fn acs(
    State(state): State<AppState>,
    address: ClientAddress,
    request: Request,
    next: Next,
) -> Response {
    if state.sso_throttle().check(address.0).allowed {
        return next.run(request).await;
    }
    tracing::debug!(address = ?address.0, "Refused a SAML response over the limit");
    redirect(&refused(
        &redirect_uri(&state.config().app_base_url),
        None,
        RATE_LIMITED,
    ))
}

fn retry_after(reset_at: Option<Instant>, now: Instant) -> u64 {
    let remaining = reset_at
        .map(|reset_at| reset_at.saturating_duration_since(now))
        .filter(|remaining| !remaining.is_zero())
        .unwrap_or(WINDOW);
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_retry_waits_until_the_window_resets_rounded_up() {
        let now = Instant::now();

        assert_eq!(
            retry_after(Some(now + Duration::from_millis(12_300)), now),
            13
        );
        assert_eq!(retry_after(Some(now + Duration::from_secs(45)), now), 45);
        assert_eq!(retry_after(Some(now + Duration::from_millis(200)), now), 1);
    }

    #[test]
    fn a_window_with_no_reset_or_one_already_past_waits_a_whole_window() {
        let now = Instant::now();

        assert_eq!(retry_after(None, now), 60);
        assert_eq!(retry_after(Some(now), now), 60);
        assert_eq!(
            retry_after(now.checked_sub(Duration::from_secs(1)), now),
            60
        );
    }

    #[test]
    fn the_refusal_explains_itself_in_one_sentence_pair() {
        assert_eq!(
            RATE_LIMITED_MESSAGE,
            "Too many sign-in attempts from your network. Wait a minute, then try again."
        );
    }
}
