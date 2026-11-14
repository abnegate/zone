mod create_request;
mod created_response;
mod creating;
mod listing_response;
mod options_response;
mod pipeline_response;
mod preview_request;
mod preview_response;
mod reconciliation_detail;
mod refusal_detail;
mod refusal_response;
mod rejection;
mod tag_detail;
mod window_query;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::PathRejection;
use axum::extract::rejection::QueryRejection;
use axum::http::StatusCode;
use uuid::Uuid;

use crate::auth::ReleaseCreator;
use crate::auth::WorkspaceMember;
use crate::routes::refresh_query::RefreshQuery;
use crate::services::github::releases;
use crate::services::github::releases::Failure;
use crate::state::AppState;
use create_request::CreateRequest;
use created_response::CreatedResponse;
use creating::Creating;
use listing_response::ListingResponse;
use options_response::OptionsResponse;
use pipeline_response::PipelineResponse;
use preview_request::PreviewRequest;
use preview_response::PreviewResponse;
use rejection::Rejection;
use window_query::WindowQuery;

const OUTCOME_UNKNOWN: &str = "The release stopped before it reported back, so what was written is unknown. Reload the releases before trying again.";

pub async fn list(
    State(state): State<AppState>,
    member: WorkspaceMember,
    query: Result<Query<WindowQuery>, QueryRejection>,
) -> Result<Json<ListingResponse>, Rejection> {
    let Query(query) = query?;
    let recent = releases::recent(&state, member.workspace_id, query.days, query.refresh()).await?;
    Ok(Json(ListingResponse::new(
        recent,
        state.github_snapshots().releases.ttl(),
    )))
}

pub async fn options(
    State(state): State<AppState>,
    member: WorkspaceMember,
    query: Result<Query<RefreshQuery>, QueryRejection>,
) -> Result<Json<OptionsResponse>, Rejection> {
    let Query(query) = query?;
    let offered = releases::options(&state, member.workspace_id, query.requested()).await?;
    Ok(Json(OptionsResponse::from(offered)))
}

pub async fn pipeline(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: Result<Path<(Uuid, Uuid, u64)>, PathRejection>,
    query: Result<Query<RefreshQuery>, QueryRejection>,
) -> Result<Json<PipelineResponse>, Rejection> {
    let Path((_, source_id, release_id)) = path?;
    let Query(query) = query?;
    let pipeline = releases::pipeline(
        &state,
        member.workspace_id,
        source_id,
        release_id,
        query.requested(),
    )
    .await?;
    Ok(Json(PipelineResponse::from(pipeline)))
}

pub async fn preview(
    State(state): State<AppState>,
    ReleaseCreator(writer): ReleaseCreator,
    request: Result<Json<PreviewRequest>, JsonRejection>,
) -> Result<Json<PreviewResponse>, Rejection> {
    let Json(request) = request?;
    let preview = releases::preview(&state, writer.workspace_id, &request.into()).await?;
    Ok(Json(PreviewResponse { preview }))
}

pub async fn create(
    State(state): State<AppState>,
    ReleaseCreator(writer): ReleaseCreator,
    request: Result<Json<CreateRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CreatedResponse>), Rejection> {
    let Json(request) = request?;
    let source_id = request.source_id;
    let release = tokio::spawn(Creating { writer, request }.run(state))
        .await
        .map_err(|error| {
            tracing::error!(%error, source = %source_id, "A release did not report back");
            Failure::Internal(OUTCOME_UNKNOWN.to_string())
        })??;
    Ok((StatusCode::CREATED, Json(CreatedResponse { release })))
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::Value;

    use super::*;
    use crate::services::github::fault::Fault;
    use crate::services::github::releases::Reconciliation;
    use crate::services::github::releases::Refusal;
    use crate::services::github::unavailable::Unavailable;
    use crate::services::github::unconnected::Unconnected;

    const OBJECT: &str = "cccccccccccccccccccccccccccccccccccccccc";

    async fn answered(failure: Failure) -> (StatusCode, Value) {
        let response = Rejection::from(failure).into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body reads");
        (status, serde_json::from_slice(&body).expect("a JSON body"))
    }

    fn keys(body: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = body
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    #[tokio::test]
    async fn a_partly_created_release_names_exactly_what_was_written() {
        let marker = Uuid::new_v4();
        let (status, body) = answered(Failure::Refused(Refusal::ReconciliationRequired(
            Reconciliation {
                tag: "v1.4.1".into(),
                tag_object: OBJECT.into(),
                reference_created: None,
                release_id: None,
                release_exists: false,
                marker,
            },
        )))
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["success"], false);
        assert_eq!(body["code"], "release_manual_reconciliation_required");
        assert_eq!(body["tag"], "v1.4.1");
        assert_eq!(body["tag_object"], OBJECT);
        assert_eq!(body["reference_created"], Value::Null);
        assert_eq!(body["release_id"], Value::Null);
        assert_eq!(body["release_exists"], false);
        assert_eq!(body["marker"], marker.to_string());
        assert_eq!(
            keys(&body),
            [
                "code",
                "error",
                "marker",
                "reference_created",
                "release_exists",
                "release_id",
                "success",
                "tag",
                "tag_object",
            ]
        );
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains(OBJECT))
        );
    }

    #[tokio::test]
    async fn a_plain_refusal_carries_only_its_code() {
        for (refusal, code) in [
            (Refusal::PreviewStale, "preview_stale"),
            (Refusal::InProgress, "release_in_progress"),
            (
                Refusal::LatestTagMoved {
                    expected: Some("v1.3.0".into()),
                    latest: Some("v1.4.0".into()),
                },
                "latest_tag_moved",
            ),
        ] {
            let (status, body) = answered(Failure::Refused(refusal)).await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(body["code"], code);
            assert_eq!(keys(&body), ["code", "error", "success"], "{code}");
        }
    }

    #[tokio::test]
    async fn a_refusal_about_a_tag_names_the_tag_and_what_holds_it() {
        let (_, body) = answered(Failure::Refused(Refusal::TagExists {
            tag: "v1.4.1".into(),
            tag_object: None,
        }))
        .await;
        assert_eq!(body["code"], "tag_exists");
        assert_eq!(keys(&body), ["code", "error", "success", "tag"]);

        let (_, body) = answered(Failure::Refused(Refusal::TagNotNewer {
            tag: "v1.3.9".into(),
            latest: "v1.4.0".into(),
        }))
        .await;
        assert_eq!(body["code"], "tag_not_newer");
        assert_eq!(body["latest_tag"], "v1.4.0");
        assert_eq!(
            keys(&body),
            ["code", "error", "latest_tag", "success", "tag"]
        );

        let (_, body) = answered(Failure::Refused(Refusal::ReleaseExists {
            tag: "v1.4.1".into(),
            release_id: 91,
        }))
        .await;
        assert_eq!(body["code"], "release_exists");
        assert_eq!(body["release_id"], 91);
        assert_eq!(
            keys(&body),
            ["code", "error", "release_id", "success", "tag"]
        );
    }

    #[tokio::test]
    async fn every_other_failure_answers_with_its_status() {
        for (failure, expected) in [
            (
                Failure::Unconnected(Unconnected::Unavailable(Unavailable::Missing)),
                StatusCode::NOT_FOUND,
            ),
            (
                Failure::Unconnected(Unconnected::Unavailable(Unavailable::NotGithub)),
                StatusCode::BAD_REQUEST,
            ),
            (
                Failure::Unconnected(Unconnected::Unavailable(Unavailable::Unreadable)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                Failure::Unconnected(Unconnected::InvalidConfiguration),
                StatusCode::CONFLICT,
            ),
            (
                Failure::Invalid("tag is invalid.".into()),
                StatusCode::BAD_REQUEST,
            ),
            (Failure::Missing("gone".into()), StatusCode::NOT_FOUND),
            (
                Failure::Upstream(Fault::upstream("GitHub request failed or timed out.")),
                StatusCode::BAD_GATEWAY,
            ),
            (
                Failure::Upstream(Fault::Unauthenticated),
                StatusCode::BAD_GATEWAY,
            ),
            (
                Failure::Internal(OUTCOME_UNKNOWN.into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ] {
            let (status, body) = answered(failure).await;
            assert_eq!(status, expected);
            assert_eq!(body["success"], false);
            assert!(body.get("code").is_none(), "{body}");
        }
    }
}
