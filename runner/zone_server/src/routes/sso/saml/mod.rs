use std::fmt;

use abnegate_secret::REDACTED;
use axum::Form;
use axum::extract::State;
use axum::extract::rejection::FormRejection;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, REFERRER_POLICY};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use zone_enterprise::sso::saml::service_metadata;
use zone_enterprise::sso::{SIGN_IN_FAILED, STATE_INVALID};

use crate::services::login::callback::page::{NO_REFERRER, NO_STORE};
use crate::services::sso::console::{redirect_uri, service_provider};
use crate::services::sso::{Handoff, Posted, SignInFailure, sign_in};
use crate::state::AppState;

#[cfg(test)]
mod tests;

pub const STATE: &str = "state";
pub const CODE: &str = "code";
pub const ERROR: &str = "error";

/// Identity providers post responses of 5 to 150 KB, so 1 MiB leaves room for a large attribute
/// set while bounding what one unauthenticated post makes the server buffer and parse.
pub const BODY_LIMIT: usize = 1024 * 1024;

/// The HTTP-POST binding's limit on RelayState (SAML 2.0 Bindings, section 3.5.3). Zone's own state
/// is 43 characters.
pub const RELAY_STATE_LIMIT: usize = 80;

#[derive(Deserialize)]
pub struct AcsForm {
    #[serde(rename = "SAMLResponse", default)]
    pub response: String,
    #[serde(rename = "RelayState")]
    pub relay_state: Option<String>,
}

impl fmt::Debug for AcsForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcsForm")
            .field("response", &REDACTED)
            .field("relay_state", &self.relay_state.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// GET /api/auth/sso/saml/metadata
pub async fn metadata(State(state): State<AppState>) -> Response {
    (
        [(CONTENT_TYPE, service_metadata::CONTENT_TYPE)],
        service_metadata::render(&service_provider(&state.config().app_base_url)),
    )
        .into_response()
}

/// POST /api/auth/sso/saml/acs
///
/// A body over [`BODY_LIMIT`] fails to read like any other unreadable form, so it gets the same
/// state-less `sso_state_invalid` redirect.
pub async fn acs(
    State(state): State<AppState>,
    form: Result<Form<AcsForm>, FormRejection>,
) -> Response {
    let callback = redirect_uri(&state.config().app_base_url);
    let form = match form {
        Ok(Form(form)) => form,
        Err(rejection) => {
            tracing::warn!(?rejection, "Refused an unreadable SAML response");
            return redirect(&refused(&callback, None, STATE_INVALID));
        }
    };
    let Some(relay_state) = form
        .relay_state
        .as_deref()
        .filter(|state| !state.is_empty() && state.len() <= RELAY_STATE_LIMIT)
    else {
        return redirect(&refused(&callback, None, STATE_INVALID));
    };
    let posted = Posted {
        state: relay_state,
        response: &form.response,
    };
    match sign_in::assert(&state, &posted).await {
        Ok(handoff) => redirect(&handed_off(&callback, &handoff)),
        Err(failure) => {
            log(&failure);
            redirect(&refused(
                &callback,
                Some(relay_state),
                refusal_code(&failure),
            ))
        }
    }
}

pub fn handed_off(callback: &str, handoff: &Handoff) -> String {
    location(callback, &[(STATE, &handoff.state), (CODE, &handoff.code)])
}

pub fn refused(callback: &str, state: Option<&str>, code: &str) -> String {
    match state {
        Some(state) => location(callback, &[(STATE, state), (ERROR, code)]),
        None => location(callback, &[(ERROR, code)]),
    }
}

pub fn refusal_code(failure: &SignInFailure) -> &'static str {
    match failure {
        SignInFailure::Refused(error) => error.code(),
        _ => SIGN_IN_FAILED,
    }
}

/// A refusal can carry the identity provider's status text, which the poster chooses, so it is
/// logged escaped.
pub fn log(failure: &SignInFailure) {
    match failure {
        SignInFailure::Refused(error) => {
            tracing::warn!(?error, code = error.code(), "Refused a SAML sign-in");
        }
        failure => tracing::error!(%failure, "Failed to complete a SAML sign-in"),
    }
}

fn location(callback: &str, pairs: &[(&str, &str)]) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.extend_pairs(pairs);
    format!("{callback}?{}", query.finish())
}

pub fn redirect(target: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static(NO_REFERRER));
    match HeaderValue::try_from(target) {
        Ok(target) => {
            headers.insert(LOCATION, target);
        }
        Err(_) => {
            tracing::error!("A SAML sign-in built a redirect that is not a valid header value");
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    response
}
