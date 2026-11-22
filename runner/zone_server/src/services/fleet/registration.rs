use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::fleet::Labels;

use crate::db::DbResult;
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::db::fleet::mode::Mode;
use crate::db::fleet::policy;
use crate::db::fleet::runners::{self, Runner};
use crate::db::fleet::shortfall::Shortfall;
use crate::services::fleet::identity::NAME;
use crate::services::fleet::refusal;
use crate::state::AppState;

/// Registers this process as its runner, or refreshes the registration, and remembers the
/// runner on `state`. A name registering for the first time is audited at server scope.
pub async fn register(state: &AppState) -> DbResult<Uuid> {
    let identity = state.identity();
    let first = state.runner().is_none();
    let registered = runners::upsert(
        state.db(),
        &identity.name,
        state.instance(),
        &identity.version,
        &identity.labels,
    )
    .await?;
    let runner = registered.runner;
    state.set_runner(runner.id);
    if registered.created {
        audit(state.db(), &runner).await;
    }
    if registered.contested {
        tracing::warn!(
            runner = %runner.name,
            "Another Zone server process held this runner's name within the last minute. That's expected right after a restart; otherwise give each server its own {NAME}, since servers sharing a name share its state"
        );
    }
    if first {
        announce(state.db(), &runner).await;
    }
    Ok(runner.id)
}

async fn announce(pool: &PgPool, runner: &Runner) {
    let minimum = match policy::get(pool).await {
        Ok(policy) => policy.minimum,
        Err(error) => {
            tracing::warn!(%error, "Could not read the fleet policy this runner registered under");
            None
        }
    };
    let shortfall = Shortfall::of(
        Mode::Unattended,
        runner.state,
        &runner.version,
        minimum.as_ref(),
        Labels::default(),
    );
    match shortfall {
        Some(shortfall) => tracing::warn!(
            runner = %runner.name,
            state = runner.state.as_str(),
            code = refusal::of(shortfall).code(),
            "Registered as a runner that takes no unattended work while fleet policy is enforced"
        ),
        None => tracing::info!(
            runner = %runner.name,
            version = %runner.version,
            labels = %runner.labels,
            "Registered as a runner"
        ),
    }
}

async fn audit(pool: &PgPool, runner: &Runner) {
    let context = AuditContext {
        org_id: None,
        workspace_id: None,
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        pool,
        &context,
        actions::RUNNER_REGISTERED,
        resources::RUNNER,
        Some(runner.id),
        None,
        Some(json!({
            "name": runner.name,
            "version": runner.version.to_string(),
            "labels": runner.labels.to_vec(),
        })),
    )
    .await
    {
        tracing::error!(%error, action = actions::RUNNER_REGISTERED, "Failed to record audit log");
    }
}
