use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::response::Response;
use zone_enterprise::scim::{Error, discovery};

use super::response;
use crate::services::scim::{Failure, ScimClient, resource};
use crate::state::AppState;

pub const NAME_PARAMETER: &str = "name";
pub const URN_PARAMETER: &str = "urn";

pub async fn service_provider_config(
    client: ScimClient,
    State(state): State<AppState>,
) -> Response {
    let base = resource::base(&state, client.organization_id);
    response::ok(&discovery::service_provider_config(&base))
}

pub async fn resource_types(client: ScimClient, State(state): State<AppState>) -> Response {
    let base = resource::base(&state, client.organization_id);
    response::ok(&discovery::resource_types(&base))
}

pub async fn resource_type(
    client: ScimClient,
    State(state): State<AppState>,
    Path(parameters): Path<HashMap<String, String>>,
) -> Result<Response, Failure> {
    let base = resource::base(&state, client.organization_id);
    parameters
        .get(NAME_PARAMETER)
        .and_then(|name| discovery::resource_type(&base, name))
        .map(|found| response::ok(&found))
        .ok_or(Failure::Protocol(Error::NotFound))
}

pub async fn schemas(_client: ScimClient) -> Response {
    response::ok(&discovery::schemas())
}

pub async fn schema(
    _client: ScimClient,
    Path(parameters): Path<HashMap<String, String>>,
) -> Result<Response, Failure> {
    parameters
        .get(URN_PARAMETER)
        .and_then(|urn| discovery::schema(urn))
        .map(|found| response::ok(&found))
        .ok_or(Failure::Protocol(Error::NotFound))
}
