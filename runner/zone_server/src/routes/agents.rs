//! Coding agent sign-in endpoints. An organization's admins sign it in to Claude Code and Codex,
//! and every member sees how it is signed in.

mod failure;
mod kind;
mod receipt_request;
mod status_query;

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header::ORIGIN},
};
use chrono::{DateTime, Utc};
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_core::SecretValue;
use zone_core::llm::AgentKind;

use crate::auth::jwt::Claims;
use crate::auth::{AuthSession, AuthUser};
use crate::db::ai_settings::{self, AccessError};
use crate::db::organization_members::OrgRole;
use crate::services::login::caller::Caller;
use crate::services::login::claude::{Flow, Scope};
use crate::services::login::status::{AgentStatus, Viewer};
use crate::services::login::{devices, oauth};
use crate::state::AppState;
use failure::Failure;
use receipt_request::ReceiptRequest;
use status_query::StatusQuery;

const UNKNOWN_AGENT: &str = "No coding agent has that name";
const NO_CODE: &str = "Codex signs in with a device code; only a Claude sign-in takes a pasted one";
const NO_RECEIPT: &str =
    "Codex signs in with a device code; only a Claude sign-in comes back with a receipt";
const NO_ATTEMPT: &str = "A codex sign-in ends when codex is signed out";
const INVALID_USER: &str = "Invalid user ID in token";

#[derive(Debug, Default, Deserialize)]
pub struct StartRequest {
    /// How much of a Claude account the sign-in asks for. A codex sign-in has no scope.
    #[serde(default)]
    pub scope: Scope,
    /// How a Claude sign-in's code comes back: the server's callback when it has one and the
    /// console asking runs on the same machine, unless this asks to paste it. A codex sign-in
    /// always shows a device code.
    #[serde(default)]
    pub flow: Option<Flow>,
}

#[derive(Debug, Deserialize)]
pub struct CodeRequest {
    /// `code#state` as Claude shows it, or the whole callback URL.
    pub code: SecretValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Statuses {
    pub agents: Vec<AgentStatus>,
}

/// A started sign-in: what the admin opens, and for codex the code they enter there.
#[derive(Serialize)]
#[serde(tag = "agent", rename_all = "lowercase")]
pub enum Login {
    Claude {
        authorize_url: String,
        expires_at: DateTime<Utc>,
        flow: Flow,
        attempt: Uuid,
    },
    Codex {
        verification_url: String,
        user_code: String,
        expires_at: DateTime<Utc>,
    },
}

/// GET /api/organizations/{org_id}/agents
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(organization): Path<Uuid>,
) -> Result<Json<Statuses>, Failure> {
    let viewer = viewer(&state, &auth.0, organization).await?;
    let agents = try_join_all(
        AgentKind::ALL.map(|agent| AgentStatus::read(&state, organization, agent, viewer, None)),
    )
    .await
    .map_err(Failure::database)?;
    Ok(Json(Statuses { agents }))
}

/// GET /api/organizations/{org_id}/agents/{agent}?attempt={attempt}
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<AgentStatus>, Failure> {
    let agent = named(&agent)?;
    let viewer = viewer(&state, &auth.0, organization).await?;
    let status = AgentStatus::read(&state, organization, agent, viewer, query.attempt)
        .await
        .map_err(Failure::database)?;
    Ok(Json(status))
}

/// POST /api/organizations/{org_id}/agents/{agent}/login
pub async fn start(
    State(state): State<AppState>,
    session: AuthSession,
    headers: HeaderMap,
    Path((organization, agent)): Path<(Uuid, String)>,
    request: Result<Option<Json<StartRequest>>, JsonRejection>,
) -> Result<Json<Login>, Failure> {
    let agent = named(&agent)?;
    let user = admin(&state, &session.claims, organization).await?;
    let request = request
        .map_err(Failure::unreadable)?
        .map(|Json(request)| request)
        .unwrap_or_default();
    let login = match agent {
        AgentKind::Claude => {
            let origin = headers.get(ORIGIN).and_then(|origin| origin.to_str().ok());
            let (redirect, console) = oauth::redirect(state.config(), request.flow, origin)?;
            let caller = Caller {
                user,
                email: session.claims.email.clone(),
                session: session.session_id,
            };
            let started =
                oauth::start(organization, &caller, request.scope, redirect, console).await;
            Login::Claude {
                authorize_url: started.url,
                expires_at: started.expires_at,
                flow: started.flow,
                attempt: started.attempt,
            }
        }
        AgentKind::Codex => {
            let prompt = devices::start(&state, organization, user, &session.claims.email).await?;
            Login::Codex {
                verification_url: prompt.verification_url,
                user_code: prompt.user_code,
                expires_at: prompt.expires_at,
            }
        }
    };
    Ok(Json(login))
}

/// POST /api/organizations/{org_id}/agents/{agent}/login/code
pub async fn finish(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
    request: Result<Json<CodeRequest>, JsonRejection>,
) -> Result<Json<AgentStatus>, Failure> {
    let agent = named(&agent)?;
    let user = admin(&state, &auth.0, organization).await?;
    if agent != AgentKind::Claude {
        return Err(Failure::new(StatusCode::BAD_REQUEST, NO_CODE));
    }
    let Json(request) = request.map_err(Failure::unreadable)?;
    oauth::finish(&state, organization, user, request.code.expose())
        .await
        .map_err(Failure::exchange)?;
    managed(&state, organization, agent, user).await
}

/// POST /api/organizations/{org_id}/agents/{agent}/login/receipt
///
/// Anyone signed in to Zone may hand a receipt in; only the admin who started its sign-in, in
/// the session they started it in, finishes it.
pub async fn redeem(
    State(state): State<AppState>,
    session: AuthSession,
    Path((organization, agent)): Path<(Uuid, String)>,
    request: Result<Json<ReceiptRequest>, JsonRejection>,
) -> Result<Json<AgentStatus>, Failure> {
    let agent = named(&agent)?;
    if agent != AgentKind::Claude {
        return Err(Failure::new(StatusCode::BAD_REQUEST, NO_RECEIPT));
    }
    let Json(request) = request.map_err(Failure::unreadable)?;
    let caller = Caller {
        user: user_of(&session.claims)?,
        email: session.claims.email.clone(),
        session: session.session_id,
    };
    oauth::redeem(&state, organization, &caller, &request.receipt)
        .await
        .map_err(Failure::exchange)?;
    managed(&state, organization, agent, caller.user).await
}

/// DELETE /api/organizations/{org_id}/agents/{agent}/login/attempt
///
/// Ends the caller's own Claude sign-in, wherever its code is.
pub async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
) -> Result<StatusCode, Failure> {
    let agent = named(&agent)?;
    let viewer = viewer(&state, &auth.0, organization).await?;
    if agent != AgentKind::Claude {
        return Err(Failure::new(StatusCode::BAD_REQUEST, NO_ATTEMPT));
    }
    oauth::cancel(organization, viewer.user).await;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/organizations/{org_id}/agents/{agent}/login
pub async fn sign_out(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
) -> Result<StatusCode, Failure> {
    let agent = named(&agent)?;
    let user = admin(&state, &auth.0, organization).await?;
    devices::sign_out(&state, organization, agent, user, &auth.0.email).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn named(agent: &str) -> Result<AgentKind, Failure> {
    AgentKind::named(agent).ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, UNKNOWN_AGENT))
}

fn user_of(claims: &Claims) -> Result<Uuid, Failure> {
    claims
        .user_id()
        .map_err(|_| Failure::new(StatusCode::UNAUTHORIZED, INVALID_USER))
}

/// Who is asking, and whether they manage the organization. Anyone outside it finds nothing.
async fn viewer(state: &AppState, claims: &Claims, organization: Uuid) -> Result<Viewer, Failure> {
    let user = user_of(claims)?;
    let mut connection = state.db().acquire().await.map_err(Failure::database)?;
    let manages = match ai_settings::authorize_organization(
        &mut connection,
        organization,
        user,
        OrgRole::Admin,
    )
    .await
    {
        Ok(()) => true,
        Err(AccessError::Forbidden(_)) => false,
        Err(error) => return Err(error.into()),
    };
    Ok(Viewer { user, manages })
}

async fn admin(state: &AppState, claims: &Claims, organization: Uuid) -> Result<Uuid, Failure> {
    let viewer = viewer(state, claims, organization).await?;
    if viewer.manages {
        Ok(viewer.user)
    } else {
        Err(Failure::admins_only())
    }
}

/// `agent`'s status as the admin who just signed the organization in sees it.
async fn managed(
    state: &AppState,
    organization: Uuid,
    agent: AgentKind,
    user: Uuid,
) -> Result<Json<AgentStatus>, Failure> {
    let viewer = Viewer {
        user,
        manages: true,
    };
    let status = AgentStatus::read(state, organization, agent, viewer, None)
        .await
        .map_err(Failure::database)?;
    Ok(Json(status))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a valid timestamp")
    }

    #[test]
    fn a_started_sign_in_names_its_agent_beside_what_to_open() {
        let attempt =
            Uuid::parse_str("6f1b1f63-5a3e-4c8e-9d0e-2b7f7c1d9a10").expect("a valid UUID");
        let claude = Login::Claude {
            authorize_url: "https://claude.com/cai/oauth/authorize?code=true".to_string(),
            expires_at: at(1_790_137_500),
            flow: Flow::Loopback,
            attempt,
        };
        let codex = Login::Codex {
            verification_url: "https://auth.openai.com/codex/device".to_string(),
            user_code: "ABCD-EFGHI".to_string(),
            expires_at: at(1_790_136_900),
        };

        assert_eq!(
            serde_json::to_value(claude).expect("serialise"),
            json!({
                "agent": "claude",
                "authorize_url": "https://claude.com/cai/oauth/authorize?code=true",
                "expires_at": "2026-09-23T04:25:00Z",
                "flow": "loopback",
                "attempt": "6f1b1f63-5a3e-4c8e-9d0e-2b7f7c1d9a10",
            })
        );
        assert_eq!(
            serde_json::to_value(codex).expect("serialise"),
            json!({
                "agent": "codex",
                "verification_url": "https://auth.openai.com/codex/device",
                "user_code": "ABCD-EFGHI",
                "expires_at": "2026-09-23T04:15:00Z",
            })
        );
    }

    #[test]
    fn a_start_asks_for_inference_unless_it_names_full_access() {
        for (body, scope) in [
            (json!({}), Scope::Inference),
            (json!({ "scope": "inference" }), Scope::Inference),
            (json!({ "scope": "full" }), Scope::Full),
        ] {
            let request: StartRequest = serde_json::from_value(body).expect("a start request");

            assert_eq!(request.scope, scope);
        }
        assert!(serde_json::from_value::<StartRequest>(json!({ "scope": "everything" })).is_err());
    }

    #[test]
    fn a_start_leaves_the_flow_to_the_server_unless_it_names_one() {
        for (body, flow) in [
            (json!({}), None),
            (json!({ "flow": "paste" }), Some(Flow::Paste)),
            (
                json!({ "flow": "loopback", "scope": "full" }),
                Some(Flow::Loopback),
            ),
        ] {
            let request: StartRequest = serde_json::from_value(body).expect("a start request");

            assert_eq!(request.flow, flow);
        }
        assert!(serde_json::from_value::<StartRequest>(json!({ "flow": "device" })).is_err());
    }
}
