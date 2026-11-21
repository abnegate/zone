use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::get_invitation;
use crate::state::{AppState, test_config, unreachable_pool};

#[tokio::test]
async fn a_database_error_answers_a_generic_internal_error() {
    let state = AppState::new(test_config(), unreachable_pool(), None);
    state.disable_mcp();

    let Err((status, Json(body))) = get_invitation(State(state), Path("token".to_owned())).await
    else {
        panic!("an unreachable database cannot find an invitation");
    };

    assert_eq!(
        (status, body.error.as_str(), body.code),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error",
            None
        )
    );
}
