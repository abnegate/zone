use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use zone_enterprise::Feature;

use super::{failed, unreadable};
use crate::auth::{ServerAdmin, entitlement};
use crate::services::sso::enforcement;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct EnforcementRequest {
    pub required: bool,
}

#[derive(Debug, Serialize)]
pub struct EnforcementResponse {
    pub required: bool,
}

pub async fn put(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Result<Json<EnforcementRequest>, JsonRejection>,
) -> Response {
    if let Err(unentitled) = entitlement::require(&state, Feature::Sso).await {
        return unentitled.into_response();
    }
    let Json(request) = match body {
        Ok(body) => body,
        Err(rejection) => return unreadable(&rejection),
    };
    match enforcement::set(&state, &admin, request.required).await {
        Ok(required) => Json(EnforcementResponse { required }).into_response(),
        Err(failure) => failed(failure),
    }
}
