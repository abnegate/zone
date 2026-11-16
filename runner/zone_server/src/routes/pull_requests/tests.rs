use axum::body::to_bytes;
use serde_json::Value;
use serde_json::json;

use super::*;
use crate::services::github::fault::Fault;
use crate::services::github::pull_requests::Refusal;
use crate::services::github::readiness::BlockerCode;
use crate::services::github::readiness::BlockerReport;

const HEAD: &str = "cccccccccccccccccccccccccccccccccccccccc";

async fn answered(error: Error) -> (StatusCode, Value) {
    let response = error.into_response();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn a_moved_head_is_refused_with_its_code_and_the_head_it_moved_to() {
    let (status, body) = answered(Error::Refused(Refusal::HeadMoved {
        current_head: HEAD.into(),
    }))
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["success"], json!(false));
    assert_eq!(body["code"], json!("head_moved"));
    assert_eq!(body["current_head"], json!(HEAD));
    assert!(body.get("blockers").is_none(), "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn a_pull_request_that_is_not_ready_is_refused_with_its_blockers() {
    let blockers = vec![BlockerReport {
        code: BlockerCode::ChecksFailed,
        detail: "1 of 2 checks failed".into(),
    }];

    let (status, body) = answered(Error::Refused(Refusal::NotReady { blockers })).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], json!("not_ready"));
    assert_eq!(
        body["blockers"],
        json!([{"code": "checks_failed", "detail": "1 of 2 checks failed"}])
    );
    assert!(body.get("current_head").is_none(), "{body}");
}

#[tokio::test]
async fn conflicts_and_protection_are_refused_by_code_alone() {
    for (refusal, code) in [
        (Refusal::Conflict, "conflict"),
        (Refusal::Protected, "protected"),
        (Refusal::Merged, "merged"),
        (Refusal::Closed, "closed"),
    ] {
        let (status, body) = answered(Error::Refused(refusal)).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], json!(code));
        assert!(body.get("current_head").is_none(), "{body}");
        assert!(body.get("blockers").is_none(), "{body}");
    }
}

#[tokio::test]
async fn every_other_failure_answers_with_its_status_and_no_code() {
    for (error, expected) in [
        (Error::Missing("gone".into()), StatusCode::NOT_FOUND),
        (Error::Invalid("bad".into()), StatusCode::BAD_REQUEST),
        (
            Error::Disconnected("reconnect".into()),
            StatusCode::CONFLICT,
        ),
        (
            Error::Internal("database".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            Error::Upstream(Fault::upstream("GitHub")),
            StatusCode::BAD_GATEWAY,
        ),
        (
            Error::Upstream(Fault::Unauthenticated),
            StatusCode::BAD_GATEWAY,
        ),
        (
            Error::Upstream(Fault::RateLimited {
                retry_after: None,
                exhausted: true,
            }),
            StatusCode::BAD_GATEWAY,
        ),
    ] {
        let (status, body) = answered(error).await;

        assert_eq!(status, expected);
        assert_eq!(body["success"], json!(false));
        assert!(body.get("code").is_none(), "{body}");
    }
}

#[test]
fn a_head_is_a_whole_commit_or_nothing() {
    let read = |head: &str| {
        HeadQuery {
            head: head.to_string(),
        }
        .head()
    };

    assert_eq!(
        read(&HEAD.to_ascii_uppercase()).map(|head| head.as_str().to_string()),
        Some(HEAD.to_string())
    );
    for refused in ["", "main", &HEAD[..39], "../../etc/passwd"] {
        assert_eq!(read(refused), None, "{refused}");
    }
}

#[test]
fn a_merge_defaults_to_a_merge_commit_and_refuses_an_unknown_method() {
    let request: MergeRequest = serde_json::from_value(json!({"expected_head": HEAD})).unwrap();
    assert_eq!(request.method, pull_requests::Method::Merge);
    for (method, expected) in [
        ("squash", pull_requests::Method::Squash),
        ("rebase", pull_requests::Method::Rebase),
        ("merge", pull_requests::Method::Merge),
    ] {
        let request: MergeRequest =
            serde_json::from_value(json!({"expected_head": HEAD, "method": method})).unwrap();
        assert_eq!(request.method, expected);
    }
    assert!(
        serde_json::from_value::<MergeRequest>(json!({"expected_head": HEAD, "method": "octopus"}))
            .is_err()
    );
}
