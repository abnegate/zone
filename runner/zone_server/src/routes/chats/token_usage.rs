mod chat_totals_response;
mod chat_usage_response;
mod model_totals_response;
mod totals_response;
mod turn_response;
mod workspace_usage_response;

use std::ops::RangeInclusive;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use super::{check_workspace_read_access, get_chat_with_access};
use crate::auth::AuthUser;
use crate::db::chat_token_usage;
use crate::error::ServerError;
use crate::state::AppState;
use chat_totals_response::ChatTotalsResponse;
use chat_usage_response::ChatUsageResponse;
use model_totals_response::ModelTotalsResponse;
use turn_response::TurnResponse;
use workspace_usage_response::WorkspaceUsageResponse;

const DEFAULT_TURNS: i64 = 50;
/// Zero asks for the totals alone.
const TURNS: RangeInclusive<i64> = 0..=500;

const DEFAULT_CHATS: i64 = 20;
const CHATS: RangeInclusive<i64> = 1..=200;

const DEFAULT_DAYS: i64 = 7;
const DAYS: RangeInclusive<i64> = 1..=365;

const LIMIT_PARAMETER: &str = "limit";
const DAYS_PARAMETER: &str = "days";

#[derive(Debug, Deserialize)]
pub struct ChatUsageQuery {
    /// How many of the chat's turns to list, newest first.
    pub limit: Option<i64>,
}

/// GET /api/chats/{id}/token-usage
pub async fn chat(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<ChatUsageQuery>,
) -> Result<Json<ChatUsageResponse>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let limit = bounded(LIMIT_PARAMETER, query.limit, DEFAULT_TURNS, TURNS)?;
    let totals = chat_token_usage::chat_totals(state.db(), chat.id).await?;
    let (turns, truncated) = match limit {
        0 => (Vec::new(), totals.turns > 0),
        _ => {
            let mut turns = chat_token_usage::chat_turns(state.db(), chat.id, limit + 1).await?;
            let truncated = turns.len() as i64 > limit;
            turns.truncate(limit as usize);
            (turns, truncated)
        }
    };
    Ok(Json(ChatUsageResponse {
        chat_id: chat.id.to_string(),
        totals: totals.into(),
        turns: turns.into_iter().map(TurnResponse::from).collect(),
        truncated,
    }))
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceUsageQuery {
    pub workspace_id: Uuid,
    /// How far back the window reaches.
    pub days: Option<i64>,
    /// How many chats to rank.
    pub limit: Option<i64>,
}

/// GET /api/chats/token-usage
pub async fn workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<WorkspaceUsageQuery>,
) -> Result<Json<WorkspaceUsageResponse>, ServerError> {
    check_workspace_read_access(&state, &auth, query.workspace_id).await?;
    let days = bounded(DAYS_PARAMETER, query.days, DEFAULT_DAYS, DAYS)?;
    let limit = bounded(LIMIT_PARAMETER, query.limit, DEFAULT_CHATS, CHATS)?;
    let since = Utc::now() - chrono::Duration::days(days);
    let chats = chat_token_usage::workspace_chats(state.db(), query.workspace_id, since, limit)
        .await?
        .into_iter()
        .map(ChatTotalsResponse::from)
        .collect();
    let models = chat_token_usage::workspace_models(state.db(), query.workspace_id, since)
        .await?
        .into_iter()
        .map(ModelTotalsResponse::from)
        .collect();
    Ok(Json(WorkspaceUsageResponse {
        workspace_id: query.workspace_id.to_string(),
        since: since.to_rfc3339(),
        chats,
        models,
    }))
}

/// `asked`, or `default` when it was not asked for. A value outside `range`
/// is refused rather than clamped, so a caller that misread the endpoint
/// hears about it.
fn bounded(
    parameter: &str,
    asked: Option<i64>,
    default: i64,
    range: RangeInclusive<i64>,
) -> Result<i64, ServerError> {
    match asked {
        None => Ok(default),
        Some(value) if range.contains(&value) => Ok(value),
        Some(value) => Err(ServerError::BadRequest(format!(
            "{parameter} must be between {} and {}, not {value}",
            range.start(),
            range.end()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_value_takes_the_default() {
        assert_eq!(
            bounded(LIMIT_PARAMETER, None, DEFAULT_TURNS, TURNS).unwrap(),
            50
        );
    }

    #[test]
    fn a_value_inside_the_range_is_taken_as_asked() {
        assert_eq!(
            bounded(LIMIT_PARAMETER, Some(0), DEFAULT_TURNS, TURNS).unwrap(),
            0
        );
        assert_eq!(
            bounded(LIMIT_PARAMETER, Some(500), DEFAULT_TURNS, TURNS).unwrap(),
            500
        );
        assert_eq!(
            bounded(DAYS_PARAMETER, Some(365), DEFAULT_DAYS, DAYS).unwrap(),
            365
        );
    }

    #[test]
    fn a_value_outside_the_range_is_refused_rather_than_clamped() {
        for asked in [-1, 501] {
            let refused = bounded(LIMIT_PARAMETER, Some(asked), DEFAULT_TURNS, TURNS);
            assert!(
                matches!(refused, Err(ServerError::BadRequest(_))),
                "{asked} should be refused, not clamped: {refused:?}"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_parameter_that_was_wrong() {
        let Err(ServerError::BadRequest(message)) =
            bounded(DAYS_PARAMETER, Some(0), DEFAULT_DAYS, DAYS)
        else {
            panic!("zero days is refused");
        };
        assert_eq!(message, "days must be between 1 and 365, not 0");
    }
}
