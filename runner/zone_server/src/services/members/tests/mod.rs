use std::time::Duration;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::{COMMUNITY_MEMBERS, MEMBER_ADMISSION_BUSY, MEMBER_CAP_REACHED};

use super::{ADMISSION_WAIT, AdmissionError, Cap, cap, check, lock_timeout};
use crate::state::{AppState, test_config, unreachable_pool};

async fn answered(error: AdmissionError) -> (StatusCode, Value) {
    let response = error.into_response();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn unreachable() -> AppState {
    let state = AppState::new(test_config(), unreachable_pool(), None);
    state.disable_mcp();
    state
}

#[tokio::test]
async fn a_full_server_refuses_with_the_code_and_a_message_naming_the_cap() {
    assert_eq!(
        answered(AdmissionError::Full {
            cap: COMMUNITY_MEMBERS
        })
        .await,
        (
            StatusCode::FORBIDDEN,
            json!({
                "error": "This server allows 3 active members without an active Zone Team or Enterprise license. Add a license to add more people.",
                "code": MEMBER_CAP_REACHED,
            })
        )
    );
}

#[tokio::test]
async fn a_seat_lock_held_too_long_is_unavailable_with_the_busy_code() {
    assert_eq!(
        answered(AdmissionError::Busy).await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({
                "error": "Too many people are joining this server at once. Try again in a moment.",
                "code": MEMBER_ADMISSION_BUSY,
            })
        )
    );
}

#[tokio::test]
async fn a_database_error_is_an_internal_error_that_names_nothing() {
    assert_eq!(
        answered(AdmissionError::Database(sqlx::Error::PoolClosed)).await,
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": "Internal server error" })
        )
    );
}

#[tokio::test]
async fn a_state_that_never_opted_in_is_uncapped_without_reading_the_database() {
    let state = unreachable();

    assert!(!state.members_capped());
    let cap = cap(&state).await.unwrap();
    assert_eq!(
        cap,
        Cap {
            limit: None,
            wait: state.config().admission_wait,
        }
    );
    check(&state, cap, None).await.unwrap();
    check(&state, cap, Some(Uuid::new_v4())).await.unwrap();
}

#[tokio::test]
async fn an_opted_in_state_reads_the_license_and_fails_closed_when_it_cannot() {
    let state = unreachable();
    state.cap_members();

    assert!(state.members_capped());
    assert!(matches!(
        cap(&state).await,
        Err(AdmissionError::Database(_))
    ));
}

#[tokio::test]
async fn a_capped_check_counts_members_and_fails_closed_when_it_cannot() {
    let state = unreachable();
    let cap = Cap {
        limit: Some(COMMUNITY_MEMBERS),
        wait: state.config().admission_wait,
    };

    assert!(matches!(
        check(&state, cap, None).await,
        Err(AdmissionError::Database(_))
    ));
}

#[test]
fn the_lock_timeout_is_the_wait_in_milliseconds_and_never_zero() {
    assert_eq!(lock_timeout(ADMISSION_WAIT), "5000ms");
    assert_eq!(lock_timeout(Duration::from_millis(100)), "100ms");
    assert_eq!(lock_timeout(Duration::from_micros(10)), "1ms");
    assert_eq!(lock_timeout(Duration::ZERO), "1ms");
}
