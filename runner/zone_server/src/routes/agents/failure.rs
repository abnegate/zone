//! Why a coding agent sign-in request failed, sent as `{"error": "…"}`.

use std::borrow::Cow;
use std::fmt::Display;

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use zone_enterprise::policy::Denial;

use super::kind::Kind;
use crate::auth::Permission;
use crate::db::ai_settings::AccessError;
use crate::services::login::error::Error;

const INTERNAL: &str = "Internal server error";

#[derive(Debug)]
pub struct Failure {
    status: StatusCode,
    message: Cow<'static, str>,
    kind: Option<Kind>,
    code: Option<&'static str>,
}

#[derive(Serialize)]
struct Body<'a> {
    error: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

impl Failure {
    pub(super) fn new(status: StatusCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            message: message.into(),
            kind: None,
            code: None,
        }
    }

    pub(super) fn forbidden(permission: Permission) -> Self {
        Self::new(StatusCode::FORBIDDEN, permission.refusal())
    }

    pub(super) fn denied(denial: Denial) -> Self {
        Self {
            code: Some(denial.code()),
            ..Self::new(StatusCode::FORBIDDEN, denial.to_string())
        }
    }

    /// Why a pasted Claude code did not finish its sign-in, and whether it can be pasted again.
    pub(super) fn exchange(error: Error) -> Self {
        let kind = match &error {
            Error::Unreadable(_) => Some(Kind::InvalidCode),
            Error::Invalid(_)
            | Error::Forbidden(_)
            | Error::Demoted(_)
            | Error::Refused(_)
            | Error::Unreachable(_) => Some(Kind::StartAgain),
            Error::Unavailable(_) | Error::Deleted | Error::Internal(_) | Error::Database(_) => {
                None
            }
        };
        Self {
            kind,
            ..Self::from(error)
        }
    }

    pub(super) fn database(error: sqlx::Error) -> Self {
        Self::internal(error)
    }

    pub(super) fn unreadable(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), rejection.body_text())
    }

    fn internal(detail: impl Display) -> Self {
        tracing::error!(%detail, "A coding agent sign-in request failed");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL)
    }
}

impl From<AccessError> for Failure {
    fn from(error: AccessError) -> Self {
        match error {
            AccessError::Forbidden(permission) => Self::forbidden(permission),
            AccessError::NotFound(message) => Self::new(StatusCode::NOT_FOUND, message),
            AccessError::Invalid(message) => Self::new(StatusCode::BAD_REQUEST, message),
            AccessError::Database(error) => Self::database(error),
        }
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        match error {
            Error::Unreadable(message) | Error::Invalid(message) => {
                Self::new(StatusCode::BAD_REQUEST, message)
            }
            Error::Forbidden(message) => Self::new(StatusCode::FORBIDDEN, message),
            Error::Demoted(_) => Self::new(StatusCode::FORBIDDEN, error.to_string()),
            Error::Unavailable(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
            Error::Deleted => Self::new(StatusCode::NOT_FOUND, error.to_string()),
            Error::Refused(message) | Error::Unreachable(message) => {
                Self::new(StatusCode::BAD_GATEWAY, message)
            }
            Error::Internal(message) => Self::internal(message),
            Error::Database(error) => Self::database(error),
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = Body {
            error: &self.message,
            kind: self.kind,
            code: self.code,
        };
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::AgentKind;
    use serde_json::{Value, json};

    use super::*;

    const DEMOTED: &str = "This sign-in needs the agent_logins:update permission, and whoever \
                           started it no longer holds it. Start again.";

    fn refusals() -> [Error; 9] {
        [
            Error::Unreadable("bad paste"),
            Error::Invalid("no such sign-in"),
            Error::Forbidden("no longer an admin"),
            Error::Demoted(Permission::AgentLoginsUpdate),
            Error::Unavailable(AgentKind::Codex),
            Error::Deleted,
            Error::Refused("device code request failed".to_string()),
            Error::Unreachable("connection refused".to_string()),
            Error::Internal("/app/agent-state is read-only".to_string()),
        ]
    }

    async fn sent(failure: Failure) -> (StatusCode, Value) {
        let response = failure.into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body");
        (status, serde_json::from_slice(&body).expect("a JSON body"))
    }

    #[test]
    fn each_failure_has_the_status_the_sign_in_panel_expects() {
        for (error, (status, message)) in refusals().into_iter().zip([
            (StatusCode::BAD_REQUEST, "bad paste"),
            (StatusCode::BAD_REQUEST, "no such sign-in"),
            (StatusCode::FORBIDDEN, "no longer an admin"),
            (StatusCode::FORBIDDEN, DEMOTED),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "The codex CLI is not installed on this server",
            ),
            (StatusCode::NOT_FOUND, "Organization not found"),
            (StatusCode::BAD_GATEWAY, "device code request failed"),
            (StatusCode::BAD_GATEWAY, "connection refused"),
            (StatusCode::INTERNAL_SERVER_ERROR, INTERNAL),
        ]) {
            let failure = Failure::from(error);

            assert_eq!(
                (failure.status, failure.message.as_ref(), failure.kind),
                (status, message, None)
            );
        }
        let forbidden = Failure::from(AccessError::Forbidden(Permission::AgentLoginsUpdate));
        assert_eq!(
            (forbidden.status, forbidden.message.as_ref()),
            (
                StatusCode::FORBIDDEN,
                Permission::AgentLoginsUpdate.refusal().as_str()
            )
        );
    }

    #[test]
    fn a_failed_code_says_whether_it_can_be_pasted_again() {
        for (error, (status, kind)) in refusals().into_iter().zip([
            (StatusCode::BAD_REQUEST, Some(Kind::InvalidCode)),
            (StatusCode::BAD_REQUEST, Some(Kind::StartAgain)),
            (StatusCode::FORBIDDEN, Some(Kind::StartAgain)),
            (StatusCode::FORBIDDEN, Some(Kind::StartAgain)),
            (StatusCode::SERVICE_UNAVAILABLE, None),
            (StatusCode::NOT_FOUND, None),
            (StatusCode::BAD_GATEWAY, Some(Kind::StartAgain)),
            (StatusCode::BAD_GATEWAY, Some(Kind::StartAgain)),
            (StatusCode::INTERNAL_SERVER_ERROR, None),
        ]) {
            let failure = Failure::exchange(error);

            assert_eq!((failure.status, failure.kind), (status, kind));
        }
    }

    #[tokio::test]
    async fn a_kind_travels_beside_the_error_and_is_left_out_when_there_is_none() {
        assert_eq!(
            sent(Failure::exchange(Error::Unreadable("bad paste"))).await,
            (
                StatusCode::BAD_REQUEST,
                json!({ "error": "bad paste", "kind": "invalid_code" })
            )
        );
        assert_eq!(
            sent(Failure::forbidden(Permission::AgentLoginsUpdate)).await,
            (
                StatusCode::FORBIDDEN,
                json!({ "error": Permission::AgentLoginsUpdate.refusal() })
            )
        );
    }

    #[tokio::test]
    async fn a_sign_in_the_policy_refuses_carries_the_denials_code() {
        let denial = Denial::Agent(zone_enterprise::policy::Agent::Claude);
        assert_eq!(
            sent(Failure::denied(denial)).await,
            (
                StatusCode::FORBIDDEN,
                json!({ "error": denial.to_string(), "code": "policy_agent_denied" })
            )
        );
    }
}
