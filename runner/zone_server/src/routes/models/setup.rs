//! GET/POST `/api/models/setup` — feature catalog, RAM/disk gates, pull list.

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use super::ErrorResponse;
use crate::auth::AuthUser;
use crate::setup::{
    FeatureSelect, Plan, PlanError, PlanInput, SetupPlan, comfy_present, make_plan,
    parse_feature_list, parse_features_csv, ram_bytes, view_plan,
};
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct SetupQuery {
    pub features: Option<String>,
    pub chat_preset: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SetupRequest {
    pub features: Option<Vec<String>>,
    pub chat_preset: Option<String>,
}

#[derive(Debug, Serialize)]
struct SetupRefusal {
    error: String,
    code: &'static str,
    plan: SetupPlan,
}

/// GET /api/models/setup
pub async fn get_setup(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<SetupQuery>,
) -> impl IntoResponse {
    match plan_from_select(
        &state,
        parse_features_csv(query.features.as_deref()),
        query.chat_preset.as_deref(),
    )
    .await
    {
        Ok(plan) => Json(view_plan(&plan)).into_response(),
        Err(error) if error.code == "unknown-feature" || error.code == "preset" => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(error.message)),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(error.message)),
        )
            .into_response(),
    }
}

/// POST /api/models/setup
pub async fn start_setup(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(body): Json<SetupRequest>,
) -> impl IntoResponse {
    let select = parse_feature_list(body.features.as_deref());
    match plan_from_select(&state, select, body.chat_preset.as_deref()).await {
        Ok(plan) => {
            let view = view_plan(&plan);
            if let Some(gate) = &view.gate {
                return (
                    StatusCode::CONFLICT,
                    Json(SetupRefusal {
                        error: gate.message.clone(),
                        code: gate.code,
                        plan: view,
                    }),
                )
                    .into_response();
            }
            Json(view).into_response()
        }
        Err(error) if error.code == "unknown-feature" || error.code == "preset" => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(error.message)),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(error.message)),
        )
            .into_response(),
    }
}

async fn plan_from_select(
    state: &AppState,
    select: Result<FeatureSelect, PlanError>,
    chat_preset: Option<&str>,
) -> Result<Plan, PlanError> {
    let features = select?;
    let models_dir = state.config().comfyui.models_dir.clone();
    let tags = ollama_tags(&state.config().ollama_host).await;
    make_plan(PlanInput {
        features,
        preset_id: chat_preset.map(str::to_string),
        ram_bytes: ram_bytes(),
        disk_free_bytes: disk_free(&models_dir),
        ollama_tags: tags,
        comfy_present: comfy_present(&models_dir),
    })
}

fn disk_free(models_dir: &Path) -> u64 {
    if let Ok(value) = std::env::var("ZONE_SETUP_DISK_FREE_BYTES")
        && let Ok(parsed) = value.trim().parse()
    {
        return parsed;
    }
    let path = if models_dir.exists() {
        models_dir
    } else {
        models_dir.parent().unwrap_or(models_dir)
    };
    super::filesystem_usage(&path.to_string_lossy())
        .map(|usage| usage.available_bytes)
        .unwrap_or(0)
}

async fn ollama_tags(host: &str) -> HashSet<String> {
    let url = format!("{}/api/tags", host.trim_end_matches('/'));
    let Ok(response) = super::OLLAMA_HTTP_CLIENT.get(url).send().await else {
        return HashSet::new();
    };
    if !response.status().is_success() {
        return HashSet::new();
    }
    let Ok(body) = response.json::<OllamaTags>().await else {
        return HashSet::new();
    };
    body.models.into_iter().map(|model| model.name).collect()
}

#[derive(Debug, Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaTag>,
}

#[derive(Debug, Deserialize)]
struct OllamaTag {
    name: String,
}
