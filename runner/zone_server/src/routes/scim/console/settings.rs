use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::{Edition, Feature, Status, status};

use super::failure_response::FailureResponse;
use super::group_response::GroupResponse;
use super::refusal::Refusal;
use super::respond;
use super::token_response::TokenResponse;
use crate::auth::{OrganizationPermit, Permission};
use crate::db::scim::{errors, groups, tokens};
use crate::services::sso::console::callback_origin;
use crate::state::AppState;

const SCIM_PATH: &str = "/api/scim/v2/organizations";

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub entitled: bool,
    pub edition: Edition,
    pub status: Status,
    pub base_url: String,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub tokens: Vec<TokenResponse>,
    pub groups: Vec<GroupResponse>,
    pub errors: Vec<FailureResponse>,
}

/// GET /api/organizations/{org_id}/scim
pub async fn get(
    State(state): State<AppState>,
    OrganizationPermit(member): OrganizationPermit<{ Permission::ScimUpdate.index() }>,
) -> Response {
    respond(read(&state, member.organization_id).await)
}

async fn read(state: &AppState, organization_id: Uuid) -> Result<Response, Refusal> {
    let license = state.license().await;
    let now = Utc::now();
    let last_synced_at = tokens::last_used(state.db(), organization_id).await?;
    let issued = tokens::list(state.db(), organization_id).await?;
    let pushed = groups::list(state.db(), organization_id, &[], 0, i64::MAX).await?;
    let failures = errors::recent(state.db(), organization_id).await?;
    Ok(Json(SettingsResponse {
        entitled: Feature::Scim.entitled(license.as_ref(), now),
        edition: Edition::from(license.as_ref()),
        status: status(license.as_ref(), now),
        base_url: base_url(&state.config().app_base_url, organization_id),
        last_synced_at,
        tokens: issued.into_iter().map(TokenResponse::from).collect(),
        groups: pushed.items.into_iter().map(GroupResponse::from).collect(),
        errors: failures.into_iter().map(FailureResponse::from).collect(),
    })
    .into_response())
}

fn base_url(app_base_url: &str, organization_id: Uuid) -> String {
    format!(
        "{}{SCIM_PATH}/{organization_id}",
        callback_origin(app_base_url)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_is_the_organizations_scim_path_on_the_console_origin() {
        let organization = Uuid::from_u128(7);
        for base in [
            "https://zone.example.com",
            "https://zone.example.com/",
            "https://zone.example.com/console/",
        ] {
            assert_eq!(
                base_url(base, organization),
                format!("https://zone.example.com/api/scim/v2/organizations/{organization}"),
                "{base}"
            );
        }
    }
}
