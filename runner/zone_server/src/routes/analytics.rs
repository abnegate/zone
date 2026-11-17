mod period_query;

use axum::Json;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::rejection::QueryRejection;
use chrono::Utc;

use crate::auth::WorkspaceMember;
use crate::error::ServerError;
use crate::services::analytics;
use crate::services::analytics::Overview;
use crate::state::AppState;
use period_query::INVALID_PERIOD;
use period_query::PeriodQuery;

pub async fn overview(
    State(state): State<AppState>,
    member: WorkspaceMember,
    query: Result<Query<PeriodQuery>, QueryRejection>,
) -> Result<Json<Overview>, ServerError> {
    let Query(query) = query.map_err(|_| ServerError::BadRequest(INVALID_PERIOD.to_string()))?;
    let period = query.period();
    let overview = analytics::overview(
        state.db(),
        member.workspace_id,
        period,
        Utc::now().naive_utc(),
    )
    .await?;
    Ok(Json(overview))
}
