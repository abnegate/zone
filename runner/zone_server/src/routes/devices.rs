//! Organization admin device list, allow/block, and instance lock.

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::OrgAdmin;
use crate::db::audit::{actions, resources};
use crate::db::{devices, sessions};
use crate::error::ServerError;
use crate::state::AppState;

use super::common::{AuditEvent, audit};

#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub name: Option<String>,
    pub platform: String,
    pub user_agent: Option<String>,
    pub last_ip: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub status: String,
    pub connected: bool,
    pub session_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListDevicesResponse {
    pub devices: Vec<DeviceResponse>,
}

#[derive(Debug, Serialize)]
pub struct DevicePolicyResponse {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeviceRequest {
    pub status: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDevicePolicyRequest {
    pub mode: String,
}

/// GET /api/organizations/:org_id/devices
pub async fn list(
    State(state): State<AppState>,
    admin: OrgAdmin,
) -> Result<impl IntoResponse, ServerError> {
    let listed = devices::list_for_organization(state.db(), admin.org_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to list devices");
            ServerError::Internal("Failed to list devices".to_string())
        })?;

    let devices = listed
        .into_iter()
        .map(|row| {
            let live = state.device_is_connected(row.device.id);
            DeviceResponse {
                id: row.device.id,
                user_id: row.device.user_id,
                email: row.email,
                display_name: row.display_name,
                name: row.device.name,
                platform: row.device.platform.as_str().to_string(),
                user_agent: row.device.user_agent,
                last_ip: row.device.last_ip,
                last_seen_at: row.device.last_seen_at,
                status: row.device.status.as_str().to_string(),
                connected: devices::is_connected(row.device.last_seen_at, live),
                session_count: row.session_count,
                created_at: row.device.created_at,
            }
        })
        .collect();

    Ok(Json(ListDevicesResponse { devices }))
}

/// PATCH /api/organizations/:org_id/devices/:device_id
pub async fn update(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Path((_, device_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateDeviceRequest>,
) -> Result<impl IntoResponse, ServerError> {
    if !devices::belongs_to_organization(state.db(), admin.org_id, device_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to verify device membership");
            ServerError::Internal("Failed to update device".to_string())
        })?
    {
        return Err(ServerError::NotFound("Device not found".to_string()));
    }

    let current = devices::get(state.db(), device_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to load device");
            ServerError::Internal("Failed to update device".to_string())
        })?
        .ok_or_else(|| ServerError::NotFound("Device not found".to_string()))?;

    if let Some(status) = request.status.as_deref() {
        let status = match status {
            "allowed" => devices::Status::Allowed,
            "blocked" => devices::Status::Blocked,
            _ => {
                return Err(ServerError::BadRequest(
                    "status must be allowed or blocked".to_string(),
                ));
            }
        };
        if status == devices::Status::Blocked {
            refuse_last_admin_block(&state, &current).await?;
        }
        devices::set_status(state.db(), device_id, status)
            .await
            .map_err(|error| {
                tracing::error!(%error, "Failed to update device status");
                ServerError::Internal("Failed to update device".to_string())
            })?
            .ok_or_else(|| ServerError::NotFound("Device not found".to_string()))?;
        if status == devices::Status::Blocked {
            sessions::revoke_device_sessions(state.db(), device_id)
                .await
                .map_err(|error| {
                    tracing::error!(%error, "Failed to revoke device sessions");
                    ServerError::Internal("Failed to update device".to_string())
                })?;
        }
        audit(
            state.db(),
            AuditEvent {
                organization_id: Some(admin.org_id),
                workspace_id: None,
                actor_id: admin.user_id,
                actor_email: &admin.email,
                action: match status {
                    devices::Status::Allowed => actions::DEVICE_ALLOWED,
                    devices::Status::Blocked => actions::DEVICE_BLOCKED,
                    devices::Status::Pending => actions::DEVICE_ALLOWED,
                },
                resource_type: resources::DEVICE,
                resource_id: Some(device_id),
                old_values: Some(serde_json::json!({ "status": current.status.as_str() })),
                new_values: Some(serde_json::json!({ "status": status.as_str() })),
            },
        )
        .await;
    }

    if let Some(name) = request.name.as_deref() {
        let name = name.trim();
        let stored = if name.is_empty() { None } else { Some(name) };
        devices::rename(state.db(), device_id, stored)
            .await
            .map_err(|error| {
                tracing::error!(%error, "Failed to rename device");
                ServerError::Internal("Failed to update device".to_string())
            })?;
        audit(
            state.db(),
            AuditEvent {
                organization_id: Some(admin.org_id),
                workspace_id: None,
                actor_id: admin.user_id,
                actor_email: &admin.email,
                action: actions::DEVICE_RENAMED,
                resource_type: resources::DEVICE,
                resource_id: Some(device_id),
                old_values: Some(serde_json::json!({ "name": current.name })),
                new_values: Some(serde_json::json!({ "name": stored })),
            },
        )
        .await;
    }

    let listed = devices::list_for_organization(state.db(), admin.org_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to load device");
            ServerError::Internal("Failed to update device".to_string())
        })?;
    let row = listed
        .into_iter()
        .find(|row| row.device.id == device_id)
        .ok_or_else(|| ServerError::NotFound("Device not found".to_string()))?;
    let live = state.device_is_connected(row.device.id);
    Ok(Json(DeviceResponse {
        id: row.device.id,
        user_id: row.device.user_id,
        email: row.email,
        display_name: row.display_name,
        name: row.device.name,
        platform: row.device.platform.as_str().to_string(),
        user_agent: row.device.user_agent,
        last_ip: row.device.last_ip,
        last_seen_at: row.device.last_seen_at,
        status: row.device.status.as_str().to_string(),
        connected: devices::is_connected(row.device.last_seen_at, live),
        session_count: row.session_count,
        created_at: row.device.created_at,
    }))
}

/// GET /api/organizations/:org_id/device-policy
pub async fn get_policy(
    State(state): State<AppState>,
    _admin: OrgAdmin,
) -> Result<impl IntoResponse, ServerError> {
    let mode = devices::policy(state.db()).await.map_err(|error| {
        tracing::error!(%error, "Failed to load device policy");
        ServerError::Internal("Failed to load device policy".to_string())
    })?;
    Ok(Json(DevicePolicyResponse {
        mode: mode.as_str().to_string(),
    }))
}

/// PUT /api/organizations/:org_id/device-policy
pub async fn set_policy(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Json(request): Json<UpdateDevicePolicyRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let mode = match request.mode.as_str() {
        "open" => devices::Mode::Open,
        "allowed" => devices::Mode::Allowed,
        _ => {
            return Err(ServerError::BadRequest(
                "mode must be open or allowed".to_string(),
            ));
        }
    };
    let previous = devices::policy(state.db()).await.map_err(|error| {
        tracing::error!(%error, "Failed to load device policy");
        ServerError::Internal("Failed to update device policy".to_string())
    })?;
    let mode = devices::set_policy(state.db(), mode)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to update device policy");
            ServerError::Internal("Failed to update device policy".to_string())
        })?;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: None,
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action: actions::DEVICE_POLICY_UPDATED,
            resource_type: resources::DEVICE_POLICY,
            resource_id: None,
            old_values: Some(serde_json::json!({ "mode": previous.as_str() })),
            new_values: Some(serde_json::json!({ "mode": mode.as_str() })),
        },
    )
    .await;
    Ok(Json(DevicePolicyResponse {
        mode: mode.as_str().to_string(),
    }))
}

async fn refuse_last_admin_block(
    state: &AppState,
    device: &devices::Device,
) -> Result<(), ServerError> {
    if device.status != devices::Status::Allowed {
        return Ok(());
    }
    let mode = devices::policy(state.db()).await.map_err(|error| {
        tracing::error!(%error, "Failed to load device policy");
        ServerError::Internal("Failed to update device".to_string())
    })?;
    if mode != devices::Mode::Allowed {
        return Ok(());
    }
    let is_admin = devices::is_admin_device(state.db(), device.id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to check admin device");
            ServerError::Internal("Failed to update device".to_string())
        })?;
    if !is_admin {
        return Ok(());
    }
    let count = devices::allowed_admin_count(state.db())
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to count admin devices");
            ServerError::Internal("Failed to update device".to_string())
        })?;
    if devices::is_last_allowed_admin(mode, device.status, is_admin, count) {
        return Err(ServerError::Conflict(
            "Cannot block the last allowed admin device while only allowed devices may connect"
                .to_string(),
        ));
    }
    Ok(())
}
