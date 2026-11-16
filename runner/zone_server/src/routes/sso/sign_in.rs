use std::fmt;

use abnegate_secret::REDACTED;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_enterprise::sso::Error;

use super::refusal::{self, Stage};
use super::unreadable;
use crate::db::sso::{Listing, providers, settings};
use crate::error::ServerError;
use crate::services::sso::console::callback_origin;
use crate::services::sso::{Callback, SignInFailure, sign_in};
use crate::state::AppState;

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

#[derive(Serialize)]
pub struct AuthorizationResponse {
    pub url: String,
    pub state: String,
}

impl fmt::Debug for AuthorizationResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationResponse")
            .field("url", &REDACTED)
            .field("state", &REDACTED)
            .finish()
    }
}

#[derive(Deserialize)]
pub struct CallbackRequest {
    pub state: String,
    pub code: String,
}

impl fmt::Debug for CallbackRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallbackRequest")
            .field("state", &REDACTED)
            .field("code", &REDACTED)
            .finish()
    }
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

    #[test]
    fn debug_never_shows_a_state_or_code() {
        let callback = CallbackRequest {
            state: "state-secret".into(),
            code: "code-secret".into(),
        };
        let authorization = AuthorizationResponse {
            url: "https://idp.example.com/authorize?state=state-secret".into(),
            state: "state-secret".into(),
        };
        for shown in [format!("{callback:?}"), format!("{authorization:?}")] {
            assert!(!shown.contains("secret"), "{shown}");
            assert!(shown.contains(REDACTED), "{shown}");
        }
    }
}
