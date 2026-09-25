//! Sign-ins and sign-outs, as an organization's audit log records them.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::status::Source;
use crate::db::agent_logins::AgentLoginRow;
use crate::db::audit::{AuditContext, actions, log_action, resources};

pub async fn signed_in(
    pool: &PgPool,
    organization: Uuid,
    actor: Uuid,
    email: &str,
    login: &AgentLoginRow,
) {
    record(
        pool,
        actions::AGENT_SIGNED_IN,
        organization,
        actor,
        email,
        login,
    )
    .await;
}

pub async fn signed_out(
    pool: &PgPool,
    organization: Uuid,
    actor: Uuid,
    email: &str,
    login: &AgentLoginRow,
) {
    record(
        pool,
        actions::AGENT_SIGNED_OUT,
        organization,
        actor,
        email,
        login,
    )
    .await;
}

/// A failure to record one is logged and never undoes the change it describes.
async fn record(
    pool: &PgPool,
    action: &str,
    organization: Uuid,
    actor: Uuid,
    email: &str,
    login: &AgentLoginRow,
) {
    let context = AuditContext {
        org_id: Some(organization),
        workspace_id: None,
        actor_id: Some(actor),
        actor_email: Some(email.to_string()),
        ip_address: None,
        user_agent: None,
    };
    let values = json!({ "agent": login.agent, "source": Source::Zone });
    if let Err(error) = log_action(
        pool,
        &context,
        action,
        resources::AGENT_LOGIN,
        Some(login.id),
        None,
        Some(values),
    )
    .await
    {
        tracing::error!(%error, action, "Failed to record audit log");
    }
}
