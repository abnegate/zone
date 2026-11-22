use serde_json::{Map, Value, json};
use uuid::Uuid;
use zone_enterprise::scim;

use super::client::ScimClient;
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::db::invitations::Revoked;
use crate::db::scim::{NewError, errors};
use crate::db::suspensions::Successor;
use crate::state::AppState;

pub const TOKEN_ID: &str = "scim_token_id";
pub const TOKEN_NAME: &str = "scim_token_name";

pub async fn audit(
    state: &AppState,
    client: &ScimClient,
    action: &str,
    resource_type: &str,
    resource_id: Uuid,
    attributes: Map<String, Value>,
) {
    log(
        state,
        client,
        None,
        action,
        resource_type,
        resource_id,
        attributes,
    )
    .await;
}

pub async fn successors(state: &AppState, client: &ScimClient, successors: &[Successor]) {
    for successor in successors {
        let mut attributes = Map::new();
        attributes.insert("user_id".to_owned(), json!(successor.user_id));
        log(
            state,
            client,
            Some(successor.workspace_id),
            actions::WORKSPACE_OWNER_SEATED,
            resources::WORKSPACE,
            successor.workspace_id,
            attributes,
        )
        .await;
    }
}

pub async fn invitations(state: &AppState, client: &ScimClient, revoked: &[Revoked]) {
    for invitation in revoked {
        let mut attributes = Map::new();
        attributes.insert("email".to_owned(), json!(invitation.email));
        log(
            state,
            client,
            None,
            actions::INVITATION_REVOKED,
            resources::INVITATION,
            invitation.id,
            attributes,
        )
        .await;
    }
}

pub async fn failure(
    state: &AppState,
    client: &ScimClient,
    method: &str,
    path: &str,
    failure: &scim::Failure,
) {
    let scim_type = failure.scim_type().map(scim::ScimType::as_str);
    let recorded = errors::record(
        state.db(),
        &NewError {
            organization_id: client.organization_id,
            token_id: Some(client.token_id),
            method,
            path,
            status: failure.status(),
            scim_type,
            detail: failure.detail(),
        },
    )
    .await;
    if let Err(error) = recorded {
        tracing::error!(
            %error,
            organization_id = %client.organization_id,
            "Failed to record a refused SCIM request"
        );
    }
}

async fn log(
    state: &AppState,
    client: &ScimClient,
    workspace_id: Option<Uuid>,
    action: &str,
    resource_type: &str,
    resource_id: Uuid,
    mut attributes: Map<String, Value>,
) {
    attributes.insert(TOKEN_ID.to_owned(), json!(client.token_id));
    attributes.insert(TOKEN_NAME.to_owned(), json!(client.token_name));
    let context = AuditContext {
        org_id: Some(client.organization_id),
        workspace_id,
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        state.db(),
        &context,
        action,
        resource_type,
        Some(resource_id),
        None,
        Some(Value::Object(attributes)),
    )
    .await
    {
        tracing::error!(%error, action, "Failed to write a SCIM audit log");
    }
}
