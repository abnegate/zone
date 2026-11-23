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
/// runner on `state`. A process whose name a newer live process holds leaves the row to it, and
/// under fleet policy claims no work until it holds the row again. A name registering for the
/// first time is audited at server scope.
pub async fn register(state: &AppState) -> DbResult<Uuid> {
    let identity = state.identity();
    let first = state.runner().is_none();
    let registered = runners::upsert(
        state.db(),
        &identity.name,
        state.instance(),
        state.started(),
        &identity.version,
        &identity.labels,
    )
    .await?;
    let runner = registered.runner;
    state.set_runner(runner.id);
    if registered.held {
        state.set_started(runner.started_at);
    }
    if registered.created {
        audit(state.db(), &runner).await;
    }
    if !registered.held {
        tracing::warn!(
            runner = %runner.name,
            "A newer Zone server process holds this runner's name, so this process takes no new work while fleet policy is enforced, and finishes only the work it holds. A process draining after a restart or rollout logs this until it exits. If it lasts, two live servers share the name: give each its own {NAME}"
        );
    } else if registered.contested {
        tracing::warn!(
            runner = %runner.name,
            "This process took its runner's name over from another Zone server process seen within the last minute, as a restart or rollout does. If that process is still running, it takes no new work while fleet policy is enforced: give each server its own {NAME}"
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
