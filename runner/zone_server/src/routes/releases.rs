mod create_request;
mod created_response;
mod listing_response;
mod options_response;
mod pipeline_response;
mod preview_request;
mod preview_response;
mod refresh_query;
mod refusal_response;
mod rejection;
mod window_query;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::auth::WorkspaceMember;
use crate::auth::WorkspaceWriter;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::routes::common::AuditEvent;
use crate::routes::common::audit;
use crate::services::github::releases;
use crate::state::AppState;
use create_request::CreateRequest;
use created_response::CreatedResponse;
use listing_response::ListingResponse;
use options_response::OptionsResponse;
use pipeline_response::PipelineResponse;
use preview_request::PreviewRequest;
use preview_response::PreviewResponse;
use refresh_query::RefreshQuery;
use rejection::Rejection;
use window_query::WindowQuery;

pub async fn list(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Query(query): Query<WindowQuery>,
) -> Result<Json<ListingResponse>, Rejection> {
    let recent = releases::recent(&state, member.workspace_id, query.days, query.refresh()).await?;
    Ok(Json(ListingResponse::new(
        recent,
        state.github_snapshots().releases.ttl(),
    )))
}

pub async fn options(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Query(query): Query<RefreshQuery>,
) -> Result<Json<OptionsResponse>, Rejection> {
    let offered = releases::options(&state, member.workspace_id, query.refresh()).await?;
    Ok(Json(OptionsResponse::from(offered)))
}

pub async fn pipeline(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, release_id)): Path<(Uuid, Uuid, u64)>,
    Query(query): Query<RefreshQuery>,
) -> Result<Json<PipelineResponse>, Rejection> {
    let pipeline = releases::pipeline(
        &state,
        member.workspace_id,
        source_id,
        release_id,
        query.refresh(),
    )
    .await?;
    Ok(Json(PipelineResponse::from(pipeline)))
}

pub async fn preview(
    State(state): State<AppState>,
    writer: WorkspaceWriter,
    Json(request): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, Rejection> {
    let preview = releases::preview(&state, writer.workspace_id, &request.into()).await?;
    Ok(Json(PreviewResponse { preview }))
}

pub async fn create(
    State(state): State<AppState>,
    writer: WorkspaceWriter,
    Json(request): Json<CreateRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), Rejection> {
    let release = releases::create(
        &state,
        writer.workspace_id,
        &request.proposal(),
        request.prerelease,
        &request.digest,
    )
    .await?;
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: Some(writer.workspace_id),
            actor_id: writer.user_id,
            actor_email: &writer.email,
            action: actions::RELEASE_CREATED,
            resource_type: resources::RELEASE,
            resource_id: None,
            old_values: None,
            new_values: Some(json!({
                "source_id": release.source_id,
                "release_id": release.id,
                "tag": release.tag,
                "url": release.url,
                "prerelease": request.prerelease,
            })),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(CreatedResponse { release })))
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::Value;

    use super::*;
    use crate::services::github::releases::Failure;
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

    #[tokio::test]
    async fn a_partly_created_release_names_exactly_what_was_written() {
        let (status, body) = answered(Failure::Refused(Refusal::ReconciliationRequired(
            Reconciliation {
                tag: "v1.4.1".into(),
                tag_object: OBJECT.into(),
                reference_created: true,
                release_id: None,
            },
        )))
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["success"], false);
        assert_eq!(body["code"], "release_manual_reconciliation_required");
        assert_eq!(body["tag"], "v1.4.1");
        assert_eq!(body["tag_object"], OBJECT);
        assert_eq!(body["reference_created"], true);
        assert_eq!(body["release_id"], Value::Null);
        assert!(body.get("release_id").is_some(), "{body}");
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
            let mut keys: Vec<&str> = body
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, ["code", "error", "success"], "{code}");
        }
        let (_, body) = answered(Failure::Refused(Refusal::TagExists {
            tag: "v1.4.1".into(),
            tag_object: None,
        }))
        .await;
        assert_eq!(body["code"], "tag_exists");
        assert_eq!(body["tag"], "v1.4.1");
        assert!(body.get("tag_object").is_none(), "{body}");
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
            (Failure::Invalid("tag is invalid."), StatusCode::BAD_REQUEST),
            (Failure::Missing("gone"), StatusCode::NOT_FOUND),
            (
                Failure::Upstream("GitHub request failed or timed out.".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                Failure::Unauthenticated("GitHub returned HTTP 401.".into()),
                StatusCode::BAD_GATEWAY,
            ),
        ] {
            let (status, body) = answered(failure).await;
            assert_eq!(status, expected);
            assert_eq!(body["success"], false);
            assert!(body.get("code").is_none(), "{body}");
        }
    }

    #[test]
    fn only_an_explicit_refresh_skips_the_cache() {
        for (refresh, expected) in [
            (None, false),
            (Some("1"), true),
            (Some("true"), true),
            (Some("0"), false),
            (Some("yes"), false),
        ] {
            assert_eq!(RefreshQuery::requested(refresh), expected, "{refresh:?}");
        }
    }
}
