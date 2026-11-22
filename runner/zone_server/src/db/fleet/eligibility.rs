use chrono::TimeDelta;
use sqlx::{AssertSqlSafe, PgExecutor};
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::DbResult;
use crate::db::fleet::candidate::Candidate;
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::mode::Mode;
use crate::db::fleet::runners::{CONNECTED, undecodable};
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;
use crate::db::tasks::UNCLAIMED;

/// Whether `runner` meets the minimum version of `policy` (`fleet_policy`).
pub const CURRENT: &str =
    "(policy.minimum_version_key IS NULL OR runner.version_key >= policy.minimum_version_key)";

/// Whether `runner` (`fleet_runners`) may claim unattended work under `policy` (`fleet_policy`):
/// it is active, current, and carries every label `ol` (`organization_runner_labels`) and `wl`
/// (`workspace_runner_labels`) require.
///
/// `ol` and `wl` must be joined from the work's own workspace and that workspace's
/// organization, never from a parameter, or one organization's labels could place another's
/// work. Either may be absent, which requires nothing.
pub const UNATTENDED: &str = "runner.state = 'active' \
     AND (policy.minimum_version_key IS NULL OR runner.version_key >= policy.minimum_version_key) \
     AND (COALESCE(ol.labels, '{}') || COALESCE(wl.labels, '{}')) <@ runner.labels";

/// Whether `runner` (`fleet_runners`) may claim interactive work under `policy`
/// (`fleet_policy`): it isn't revoked and is current. Labels don't apply, so no `ol` or `wl`
/// join is needed; were one added, it must come from the work's own workspace as for
/// [`UNATTENDED`].
pub const INTERACTIVE: &str = "runner.state <> 'revoked' \
     AND (policy.minimum_version_key IS NULL OR runner.version_key >= policy.minimum_version_key)";

/// The task `t` with the labels its own workspace `w` and that workspace's organization require,
/// as `ol` and `wl`, beside `policy` (`fleet_policy`): what [`UNATTENDED`] needs for a task's
/// runs. Constrain `t.id` to the run's own `task_id`, never to a parameter.
pub const PLACEMENT: &str = "tasks t \
     JOIN workspaces w ON w.id = t.workspace_id \
     LEFT JOIN organization_runner_labels ol ON ol.organization_id = w.organization_id \
     LEFT JOIN workspace_runner_labels wl ON wl.workspace_id = w.id \
     CROSS JOIN fleet_policy policy";

/// How long an admitted run stays unclaimed before another runner may pick it up, so the runner
/// that admitted it claims it first.
pub const PICKUP_AFTER: TimeDelta = TimeDelta::seconds(2);

#[derive(sqlx::FromRow)]
struct Judged {
    state: String,
    version: String,
    labels: Vec<String>,
    minimum_version: Option<String>,
    required: Vec<String>,
}

/// Why `runner` may not claim `mode` work in `workspace`, in [`Shortfall`]'s precedence, or
/// `None` when it may. A missing runner, whether never registered or since removed, is
/// unregistered.
pub async fn shortfall(
    executor: impl PgExecutor<'_>,
    runner: Option<Uuid>,
    workspace: Option<Uuid>,
    mode: Mode,
) -> DbResult<Option<Shortfall>> {
    let Some(runner) = runner else {
        return Ok(Some(Shortfall::Unregistered));
    };
    let judged: Option<Judged> = sqlx::query_as(
        "SELECT runner.state, runner.version, runner.labels, policy.minimum_version, \
             COALESCE(ol.labels, '{}') || COALESCE(wl.labels, '{}') AS required \
         FROM fleet_runners runner \
         CROSS JOIN fleet_policy policy \
         LEFT JOIN workspaces w ON w.id = $2 \
         LEFT JOIN organization_runner_labels ol ON ol.organization_id = w.organization_id \
         LEFT JOIN workspace_runner_labels wl ON wl.workspace_id = w.id \
         WHERE runner.id = $1",
    )
    .bind(runner)
    .bind(workspace)
    .fetch_optional(executor)
    .await?;
    let Some(judged) = judged else {
        return Ok(Some(Shortfall::Unregistered));
    };
    let state = State::parse(&judged.state).ok_or_else(|| {
        undecodable(
            "state",
            format!("\"{}\" is not a runner state", judged.state),
        )
    })?;
    let version = Version::parse(&judged.version).map_err(|error| undecodable("version", error))?;
    let minimum = judged
        .minimum_version
        .map(|minimum| Minimum::parse(&minimum))
        .transpose()
        .map_err(|error| undecodable("minimum_version", error))?;
    let carried =
        Labels::from_strings(&judged.labels).map_err(|error| undecodable("labels", error))?;
    let required =
        Labels::from_strings(&judged.required).map_err(|error| undecodable("required", error))?;
    Ok(Shortfall::of(
        mode,
        state,
        &version,
        minimum.as_ref(),
        required.missing_from(&carried),
    ))
}

/// Up to `limit` task runs `runner` may pick up, oldest first: admitted at least
/// [`PICKUP_AFTER`] ago, still unclaimed and fresh, and placed where [`UNATTENDED`] lets
/// `runner` take them.
pub async fn candidates(
    executor: impl PgExecutor<'_>,
    runner: Uuid,
    limit: i64,
) -> DbResult<Vec<Candidate>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT r.id AS run, r.task_id AS task \
         FROM {PLACEMENT} \
         JOIN task_runs r ON r.task_id = t.id \
         JOIN fleet_runners runner ON runner.id = $1 \
         WHERE {UNCLAIMED} AND r.heartbeat_at <= now() - $3 AND {UNATTENDED} \
         ORDER BY r.heartbeat_at, r.id \
         LIMIT $2"
    )))
    .bind(runner)
    .bind(limit)
    .bind(PICKUP_AFTER)
    .fetch_all(executor)
    .await
}

/// Whether a connected runner other than `except` may take task run `run`.
pub async fn exists(
    executor: impl PgExecutor<'_>,
    run: Uuid,
    except: Option<Uuid>,
) -> DbResult<bool> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT EXISTS ( \
             SELECT 1 FROM {PLACEMENT} \
             JOIN task_runs r ON r.task_id = t.id \
             JOIN fleet_runners runner \
               ON runner.id IS DISTINCT FROM $2 AND runner.last_seen_at > now() - $3 \
             WHERE r.id = $1 AND {UNATTENDED} \
         )"
    )))
    .bind(run)
    .bind(except)
    .bind(CONNECTED)
    .fetch_one(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_predicates_hold_the_minimum_clause() {
        assert!(UNATTENDED.contains(CURRENT));
        assert!(INTERACTIVE.contains(CURRENT));
    }

    #[test]
    fn the_predicates_name_the_stored_states() {
        assert!(UNATTENDED.contains(&format!("runner.state = '{}'", State::Active.as_str())));
        assert!(INTERACTIVE.contains(&format!("runner.state <> '{}'", State::Revoked.as_str())));
    }

    #[test]
    fn placement_takes_the_labels_from_the_tasks_own_workspace_and_organization() {
        assert!(PLACEMENT.contains("w.id = t.workspace_id"));
        assert!(PLACEMENT.contains("ol.organization_id = w.organization_id"));
        assert!(PLACEMENT.contains("wl.workspace_id = w.id"));
        assert!(!PLACEMENT.contains('$'));
    }

    #[test]
    fn only_unattended_work_needs_labels() {
        assert!(UNATTENDED.contains("ol.labels") && UNATTENDED.contains("wl.labels"));
        assert!(!INTERACTIVE.contains("labels"));
    }
}
