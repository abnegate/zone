//! What chats spend in tokens, read by anyone who can read them.
//!
//! Two shapes, because token work asks two questions. One chat's endpoint
//! answers "what did this conversation cost, and which turn cost it", which is
//! what you want while changing a prompt. The workspace rollup answers "which
//! chats and which models are the expensive ones over this window", which is
//! what you want when deciding what to change next.

mod chat_totals_response;
mod chat_usage_response;
mod model_totals_response;
mod totals_response;
mod turn_response;
mod workspace_usage_response;

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

/// Turns one chat request lists at most. A long chat has hundreds of turns and
/// the totals already cover all of them, so the list is the recent detail
/// rather than the record.
const DEFAULT_TURNS: i64 = 50;
const MAX_TURNS: i64 = 500;

/// Chats one rollup ranks at most.
const DEFAULT_CHATS: i64 = 20;
const MAX_CHATS: i64 = 200;

/// Days one rollup covers by default, and the longest it will cover. A year
/// bounds the scan on a workspace that has been running a while.
const DEFAULT_DAYS: i64 = 7;
const MAX_DAYS: i64 = 365;

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
    let limit = bounded(query.limit, DEFAULT_TURNS, MAX_TURNS)?;
    let totals = chat_token_usage::chat_totals(state.db(), chat.id).await?;
    // One more than asked for, so "there is more" is observed rather than
    // inferred from a full page, which a chat with exactly `limit` turns would
    // make a lie.
    let mut turns = chat_token_usage::chat_turns(state.db(), chat.id, limit + 1).await?;
    let truncated = turns.len() as i64 > limit;
    turns.truncate(limit as usize);
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
    let days = bounded(query.days, DEFAULT_DAYS, MAX_DAYS)?;
    let limit = bounded(query.limit, DEFAULT_CHATS, MAX_CHATS)?;
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

/// `asked`, or `default` when it was not asked for. A value outside the range
/// is refused rather than clamped: a caller that asked for 10,000 turns has
/// misunderstood the endpoint, and silently handing back 500 hides that.
fn bounded(asked: Option<i64>, default: i64, max: i64) -> Result<i64, ServerError> {
    match asked {
        None => Ok(default),
        Some(value) if (1..=max).contains(&value) => Ok(value),
        Some(value) => Err(ServerError::BadRequest(format!(
            "limit must be between 1 and {max}, not {value}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_limit_takes_the_default() {
        assert_eq!(bounded(None, DEFAULT_TURNS, MAX_TURNS).unwrap(), 50);
    }

    #[test]
    fn a_limit_inside_the_range_is_taken_as_asked() {
        assert_eq!(bounded(Some(10), DEFAULT_TURNS, MAX_TURNS).unwrap(), 10);
        assert_eq!(bounded(Some(500), DEFAULT_TURNS, MAX_TURNS).unwrap(), 500);
    }

    #[test]
    fn a_limit_outside_the_range_is_refused_rather_than_clamped() {
        for asked in [0, -1, MAX_TURNS + 1] {
            let refused = bounded(Some(asked), DEFAULT_TURNS, MAX_TURNS);
            assert!(
                matches!(refused, Err(ServerError::BadRequest(_))),
                "{asked} should be refused, not clamped: {refused:?}"
            );
        }
    }
}
