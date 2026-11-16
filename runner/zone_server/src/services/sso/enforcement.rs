use serde_json::json;
use sqlx::PgPool;
use zone_enterprise::sso::{Error, PasswordAccess, password_access};

use super::failure::Failure;
use crate::db::DbResult;
use crate::db::audit::{actions, resources};
use crate::db::sso::{providers, settings};
use crate::db::users::UserRow;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

const REQUIRED: &str = "required";

/// Whether a password may be used, given who is using it and the server's enforcement setting.
pub async fn access(pool: &PgPool, admin: bool) -> DbResult<PasswordAccess> {
    Ok(password_access(settings::required(pool).await?, admin))
}

pub async fn set(state: &AppState, actor: &UserRow, required: bool) -> Result<bool, Failure> {
    let mut transaction = state.db().begin().await?;
    settings::lock(&mut transaction).await?;
    if required && providers::count_enabled(&mut *transaction).await? == 0 {
        return Err(Error::ProviderRequired.into());
    }
    let previous = settings::set_required(&mut transaction, required).await?;
    transaction.commit().await?;
    if previous != required {
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: None,
                actor_id: actor.id,
                actor_email: &actor.email,
                action: actions::SSO_ENFORCEMENT_UPDATED,
                resource_type: resources::SSO_SETTINGS,
                resource_id: None,
                old_values: Some(json!({ REQUIRED: previous })),
                new_values: Some(json!({ REQUIRED: required })),
            },
        )
        .await;
    }
    Ok(required)
}
