use sqlx::PgExecutor;
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::DbResult;
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::mode::Mode;
use crate::db::fleet::runners::undecodable;
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;

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
    fn only_unattended_work_needs_labels() {
        assert!(UNATTENDED.contains("ol.labels") && UNATTENDED.contains("wl.labels"));
        assert!(!INTERACTIVE.contains("labels"));
    }
}
