use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Serialize;
use zone_enterprise::{Edition, Feature, Status, status};

use super::providers::ProviderResponse;
use crate::auth::ServerAdmin;
use crate::db::sso::{providers, settings};
use crate::error::ServerError;
use crate::services::sso::console::redirect_uri;
use crate::state::AppState;

const LOAD_FAILED: &str = "Could not load single sign-on";

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub entitled: bool,
    pub edition: Edition,
    pub status: Status,
    pub required: bool,
    pub redirect_uri: String,
    pub providers: Vec<ProviderResponse>,
}

pub async fn get(State(state): State<AppState>, _admin: ServerAdmin) -> Response {
    let license = state.license().await;
    let now = Utc::now();
    let required = match settings::required(state.db()).await {
        Ok(required) => required,
        Err(error) => return unavailable(&error),
    };
    let records = match providers::list(state.db()).await {
        Ok(records) => records,
        Err(error) => return unavailable(&error),
    };
    Json(SettingsResponse {
        entitled: Feature::Sso.entitled(license.as_ref(), now),
        edition: Edition::from(license.as_ref()),
        status: status(license.as_ref(), now),
        required,
        redirect_uri: redirect_uri(&state.config().app_base_url),
        providers: records.iter().map(ProviderResponse::from).collect(),
    })
    .into_response()
}

fn unavailable(error: &dyn std::error::Error) -> Response {
    tracing::error!(%error, "Failed to load single sign-on");
    ServerError::Internal(LOAD_FAILED.to_owned()).into_response()
}
