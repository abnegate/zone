//! Coding agent sign-in endpoints. An organization's admins sign it in to Claude Code and Codex,
//! and every member sees how it is signed in.

mod failure;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_core::SecretValue;
use zone_core::llm::AgentKind;

use crate::auth::AuthUser;
use crate::db::ai_settings::{self, AccessError};
use crate::db::organization_members::OrgRole;
use crate::services::login::claude::Scope;
use crate::services::login::status::{AgentStatus, Viewer};
use crate::services::login::{devices, oauth};
use crate::state::AppState;
use failure::Failure;

const UNKNOWN_AGENT: &str = "No coding agent has that name";
const NO_CODE: &str = "Codex signs in with a device code; only a Claude sign-in takes a pasted one";
const INVALID_USER: &str = "Invalid user ID in token";

#[derive(Debug, Default, Deserialize)]
pub struct StartRequest {
    /// How much of a Claude account the sign-in asks for. A codex sign-in has no scope.
    #[serde(default)]
    pub scope: Scope,
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
    let viewer = viewer(&state, &auth, organization).await?;
    let agents = try_join_all(
        AgentKind::ALL.map(|agent| AgentStatus::read(&state, organization, agent, viewer)),
    )
    .await
    .map_err(Failure::database)?;
    Ok(Json(Statuses { agents }))
}

/// GET /api/organizations/{org_id}/agents/{agent}
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
) -> Result<Json<AgentStatus>, Failure> {
    let agent = named(&agent)?;
    let viewer = viewer(&state, &auth, organization).await?;
    let status = AgentStatus::read(&state, organization, agent, viewer)
        .await
        .map_err(Failure::database)?;
    Ok(Json(status))
}

/// POST /api/organizations/{org_id}/agents/{agent}/login
pub async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
    request: Result<Option<Json<StartRequest>>, JsonRejection>,
) -> Result<Json<Login>, Failure> {
    let agent = named(&agent)?;
    let user = admin(&state, &auth, organization).await?;
    let request = request
        .map_err(Failure::unreadable)?
        .map(|Json(request)| request)
        .unwrap_or_default();
    let login = match agent {
        AgentKind::Claude => {
            let started = oauth::start(organization, user, request.scope);
            Login::Claude {
                authorize_url: started.url,
                expires_at: started.expires_at,
            }
        }
        AgentKind::Codex => {
            let prompt = devices::start(&state, organization, user, &auth.0.email).await?;
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
    let user = admin(&state, &auth, organization).await?;
    if agent != AgentKind::Claude {
        return Err(Failure::new(StatusCode::BAD_REQUEST, NO_CODE));
    }
    let Json(request) = request.map_err(Failure::unreadable)?;
    oauth::finish(
        &state,
        organization,
        user,
        &auth.0.email,
        request.code.expose(),
    )
    .await?;
    let viewer = Viewer {
        user,
        manages: true,
    };
    let status = AgentStatus::read(&state, organization, agent, viewer)
        .await
        .map_err(Failure::database)?;
    Ok(Json(status))
}

/// DELETE /api/organizations/{org_id}/agents/{agent}/login
pub async fn sign_out(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((organization, agent)): Path<(Uuid, String)>,
) -> Result<StatusCode, Failure> {
    let agent = named(&agent)?;
    let user = admin(&state, &auth, organization).await?;
    devices::sign_out(&state, organization, agent, user, &auth.0.email).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn named(agent: &str) -> Result<AgentKind, Failure> {
    AgentKind::named(agent).ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, UNKNOWN_AGENT))
}

/// Who is asking, and whether they manage the organization. Anyone outside it finds nothing.
async fn viewer(state: &AppState, auth: &AuthUser, organization: Uuid) -> Result<Viewer, Failure> {
    let user = auth
        .0
        .user_id()
        .map_err(|_| Failure::new(StatusCode::UNAUTHORIZED, INVALID_USER))?;
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

async fn admin(state: &AppState, auth: &AuthUser, organization: Uuid) -> Result<Uuid, Failure> {
    let viewer = viewer(state, auth, organization).await?;
    if viewer.manages {
        Ok(viewer.user)
    } else {
        Err(Failure::admins_only())
    }
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
        let claude = Login::Claude {
            authorize_url: "https://claude.com/cai/oauth/authorize?code=true".to_string(),
            expires_at: at(1_790_137_500),
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
}
