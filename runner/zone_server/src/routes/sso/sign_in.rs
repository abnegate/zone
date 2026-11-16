use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_enterprise::sso::Error;

use super::refusal::{self, Stage};
use super::unreadable;
use crate::db::sso::{Listing, providers, settings};
use crate::error::ServerError;
use crate::services::sso::{Callback, SignInFailure, sign_in};
use crate::state::AppState;

pub const CALLBACK_PATH: &str = "/sso/callback";

const LOAD_FAILED: &str = "Could not load single sign-on";
const SIGN_IN_FAILED: &str = "Could not complete single sign-on";

#[derive(Debug, Serialize)]
pub struct PublicProvidersResponse {
    pub required: bool,
    pub callback_origin: String,
    pub providers: Vec<PublicProvider>,
}

#[derive(Debug, Serialize)]
pub struct PublicProvider {
    pub id: Uuid,
    pub display_name: String,
}

impl From<Listing> for PublicProvider {
    fn from(listing: Listing) -> Self {
        Self {
            id: listing.id,
            display_name: listing.display_name,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AuthorizationResponse {
    pub url: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct CallbackRequest {
    pub state: String,
    pub code: String,
}

/// Where identity providers send people back: the console's callback page, which hands the code
/// to the server.
pub fn redirect_uri(app_base_url: &str) -> String {
    format!("{}{CALLBACK_PATH}", app_base_url.trim_end_matches('/'))
}

/// The scheme, host and port of the console. A sign-in has to start on this origin, because the
/// callback page checks the state the browser kept when it began.
pub fn callback_origin(app_base_url: &str) -> String {
    Url::parse(app_base_url)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|_| app_base_url.trim_end_matches('/').to_owned())
}

/// GET /api/auth/sso/providers
pub async fn providers(State(state): State<AppState>) -> Response {
    let required = match settings::required(state.db()).await {
        Ok(required) => required,
        Err(error) => return unavailable(&error),
    };
    let listings = match providers::list_enabled(state.db()).await {
        Ok(listings) => listings,
        Err(error) => return unavailable(&error),
    };
    Json(PublicProvidersResponse {
        required,
        callback_origin: callback_origin(&state.config().app_base_url),
        providers: listings.into_iter().map(PublicProvider::from).collect(),
    })
    .into_response()
}

/// POST /api/auth/sso/providers/{provider_id}/authorizations
pub async fn authorize(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(provider_id)) = path else {
        return refusal::respond(&Error::ProviderNotFound, Stage::SignIn);
    };
    match sign_in::authorize(&state, provider_id).await {
        Ok(authorization) => (
            StatusCode::CREATED,
            Json(AuthorizationResponse {
                url: authorization.url.into(),
                state: authorization.state,
            }),
        )
            .into_response(),
        Err(failure) => failed(failure),
    }
}

/// POST /api/auth/sso/callbacks
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CallbackRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(body) => body,
        Err(rejection) => return unreadable(&rejection),
    };
    let callback = Callback {
        state: &request.state,
        code: &request.code,
    };
    match sign_in::complete(&state, &callback, &headers).await {
        Ok(response) => Json(response).into_response(),
        Err(failure) => failed(failure),
    }
}

fn failed(failure: SignInFailure) -> Response {
    match failure {
        SignInFailure::Refused(error) => refusal::respond(&error, Stage::SignIn),
        SignInFailure::Session(refusal) => refusal.into_response(),
        failure => {
            tracing::error!(%failure, "Failed to complete single sign-on");
            ServerError::Internal(SIGN_IN_FAILED.to_owned()).into_response()
        }
    }
}

fn unavailable(error: &sqlx::Error) -> Response {
    tracing::error!(%error, "Failed to load single sign-on providers");
    ServerError::Internal(LOAD_FAILED.to_owned()).into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_redirect_uri_is_the_console_callback_under_the_app_base_url() {
        for base in ["https://zone.example.com", "https://zone.example.com/"] {
            assert_eq!(
                redirect_uri(base),
                "https://zone.example.com/sso/callback",
                "{base}"
            );
        }
        assert_eq!(
            redirect_uri("https://example.com/zone"),
            "https://example.com/zone/sso/callback"
        );
    }

    #[test]
    fn the_callback_origin_is_the_scheme_host_and_port_of_the_console() {
        let cases = [
            ("https://zone.example.com", "https://zone.example.com"),
            ("https://zone.example.com/", "https://zone.example.com"),
            ("https://example.com/zone", "https://example.com"),
            (
                "https://zone.example.com:8443/app/",
                "https://zone.example.com:8443",
            ),
            ("https://zone.example.com:443", "https://zone.example.com"),
            ("http://localhost:3000", "http://localhost:3000"),
        ];
        for (base, origin) in cases {
            assert_eq!(callback_origin(base), origin, "{base}");
        }
    }

    #[test]
    fn the_public_listing_carries_the_contract_fields() {
        let body = serde_json::to_value(PublicProvidersResponse {
            required: true,
            callback_origin: "https://zone.example.com".into(),
            providers: vec![PublicProvider::from(Listing {
                id: Uuid::nil(),
                display_name: "Okta".into(),
            })],
        })
        .unwrap();
        assert_eq!(
            body,
            json!({
                "required": true,
                "callback_origin": "https://zone.example.com",
                "providers": [{ "id": Uuid::nil(), "display_name": "Okta" }],
            })
        );
    }

    #[test]
    fn a_callback_carries_the_state_and_code() {
        let request: CallbackRequest =
            serde_json::from_value(json!({ "state": "s", "code": "c" })).unwrap();
        assert_eq!((request.state.as_str(), request.code.as_str()), ("s", "c"));
    }
}
