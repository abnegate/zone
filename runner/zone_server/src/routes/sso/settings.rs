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
use crate::services::sso::console::{redirect_uri, service_provider};
use crate::state::AppState;

const LOAD_FAILED: &str = "Could not load single sign-on";

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub entitled: bool,
    pub edition: Edition,
    pub status: Status,
    pub required: bool,
    pub redirect_uri: String,
    pub service_provider: ServiceProviderResponse,
    pub providers: Vec<ProviderResponse>,
}

/// What an admin registers at a SAML identity provider. Zone serves its metadata at its own
/// entity ID, so those two values are the same address.
#[derive(Debug, Serialize)]
pub struct ServiceProviderResponse {
    pub entity_id: String,
    pub acs_url: String,
    pub metadata_url: String,
}

impl ServiceProviderResponse {
    pub fn new(app_base_url: &str) -> Self {
        let service = service_provider(app_base_url);
        Self {
            metadata_url: service.entity_id.clone(),
            entity_id: service.entity_id,
            acs_url: service.acs_url,
        }
    }
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
    let app_base_url = &state.config().app_base_url;
    Json(SettingsResponse {
        entitled: Feature::Sso.entitled(license.as_ref(), now),
        edition: Edition::from(license.as_ref()),
        status: status(license.as_ref(), now),
        required,
        redirect_uri: redirect_uri(app_base_url),
        service_provider: ServiceProviderResponse::new(app_base_url),
        providers: records.iter().map(ProviderResponse::from).collect(),
    })
    .into_response()
}

fn unavailable(error: &dyn std::error::Error) -> Response {
    tracing::error!(%error, "Failed to load single sign-on");
    ServerError::Internal(LOAD_FAILED.to_owned()).into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_service_provider_lives_on_the_app_base_url_origin() {
        for base in [
            "https://zone.example.com",
            "https://zone.example.com/",
            "https://zone.example.com/console/",
        ] {
            assert_eq!(
                serde_json::to_value(ServiceProviderResponse::new(base)).unwrap(),
                json!({
                    "entity_id": "https://zone.example.com/api/auth/sso/saml/metadata",
                    "acs_url": "https://zone.example.com/api/auth/sso/saml/acs",
                    "metadata_url": "https://zone.example.com/api/auth/sso/saml/metadata",
                }),
                "{base}"
            );
        }
    }
}
