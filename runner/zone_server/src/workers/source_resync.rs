//! Periodic and change-driven source reindexing
//!
//! Polls active sources on a schedule. Incremental adapters (GitHub, GitLab,
//! filesystem) are reindexed when their remote version changes or when files
//! never got embeddings. Other sources are refreshed when they age past
//! `SOURCE_RESYNC_INTERVAL_SECS`.

use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::db::sources::{self, ActiveIndexSource, IndexStatus};
use crate::state::AppState;
use crate::workers::gathering::core_source_from_row;
use crate::workers::indexing;

/// Why a source should be reindexed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncReason {
    NeverSynced,
    RemoteChanged,
    MissingEmbeddings,
    ScheduleDue,
}

/// Decide whether a source needs a resync pass
pub fn resync_decision(
    supports_incremental: bool,
    stored_version: Option<&str>,
    remote_version: Option<&str>,
    missing_embeddings: i64,
    last_sync: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    interval: Duration,
) -> Option<ResyncReason> {
    if missing_embeddings > 0 {
        return Some(ResyncReason::MissingEmbeddings);
    }
    if stored_version.is_none() && last_sync.is_none() {
        return Some(ResyncReason::NeverSynced);
    }
    if supports_incremental {
        if remote_version.is_some() && remote_version != stored_version {
            return Some(ResyncReason::RemoteChanged);
        }
        if remote_version.is_none() {
            return match last_sync {
                None => Some(ResyncReason::NeverSynced),
                Some(synced) if now.signed_duration_since(synced).to_std().ok()? >= interval => {
                    Some(ResyncReason::ScheduleDue)
                }
                _ => None,
            };
        }
        return None;
    }
    match last_sync {
        None => Some(ResyncReason::NeverSynced),
        Some(synced) if now.signed_duration_since(synced).to_std().ok()? >= interval => {
            Some(ResyncReason::ScheduleDue)
        }
        _ => None,
    }
}

/// Start the source resync worker
pub fn start_resync_worker(state: AppState) {
    let config = state.config().source_index.clone();
    if !config.enabled {
        tracing::info!("Source resync worker disabled");
        return;
    }

    tokio::spawn(async move {
        // Short first delay so a just-started server can pick up failed files
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            if let Err(e) = poll_sources(&state).await {
                tracing::error!("Source resync poll failed: {}", e);
            }
            tokio::time::sleep(Duration::from_secs(config.poll_interval_secs)).await;
        }
    });
}

async fn poll_sources(state: &AppState) -> Result<(), sqlx::Error> {
    let sources = sources::list_active_index_sources(state.db()).await?;
    if sources.is_empty() {
        return Ok(());
    }

    let interval = Duration::from_secs(state.config().source_index.interval_secs);
    let now = Utc::now();
    let mut triggered = 0usize;

    for source in sources {
        match consider_source(state, &source, now, interval).await {
            Ok(true) => triggered += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!(
                source_id = %source.id,
                error = %e,
                "source resync check failed"
            ),
        }
    }

    if triggered > 0 {
        tracing::info!("Source resync worker queued {triggered} incremental indexes");
    }
    Ok(())
}

async fn consider_source(
    state: &AppState,
    source: &ActiveIndexSource,
    now: DateTime<Utc>,
    interval: Duration,
) -> Result<bool, String> {
    let Some(user_id) = source.user_id else {
        tracing::debug!(source_id = %source.id, "skipping source with no workspace member");
        return Ok(false);
    };

    match sources::get_source_index_status(state.db(), source.id).await {
        Ok(status) if matches!(status.status, IndexStatus::Indexing) => return Ok(false),
        Ok(_) => {}
        Err(e) => return Err(format!("index status: {e}")),
    }

    let missing = sources::count_items_missing_embeddings(state.db(), source.id)
        .await
        .map_err(|e| format!("missing embeddings: {e}"))?;

    let Some(registry) = state.adapter_registry() else {
        return Err("adapter registry unavailable".to_string());
    };
    let Some(core) = core_source_from_row(state, source.as_source_row()) else {
        return Ok(false);
    };
    let adapter = registry
        .get_for_source(&core)
        .map_err(|e| format!("adapter: {e}"))?;

    let supports_incremental = adapter.supports_incremental();
    let remote_version = if supports_incremental {
        match adapter.get_sync_state(&core).await {
            Ok(sync) => sync.version,
            Err(e) => {
                tracing::debug!(
                    source_id = %source.id,
                    error = %e,
                    "could not read remote sync state; falling back to schedule"
                );
                None
            }
        }
    } else {
        None
    };

    let last_sync = source
        .last_sync_at
        .or(source.last_verified_at)
        .map(|dt| dt.and_utc());

    let reason = resync_decision(
        supports_incremental,
        source.sync_version.as_deref(),
        remote_version.as_deref(),
        missing,
        last_sync,
        now,
        interval,
    );

    let Some(reason) = reason else {
        return Ok(false);
    };

    tracing::info!(
        source_id = %source.id,
        source_name = %source.name,
        ?reason,
        "queueing incremental source index"
    );
    indexing::spawn_index_source(state.clone(), source.id, source.workspace_id, user_id, true);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_synced_sources_are_queued() {
        let now = Utc::now();
        assert_eq!(
            resync_decision(true, None, None, 0, None, now, Duration::from_secs(3600)),
            Some(ResyncReason::NeverSynced)
        );
    }

    #[test]
    fn missing_embeddings_win() {
        let now = Utc::now();
        assert_eq!(
            resync_decision(
                true,
                Some("abc"),
                Some("abc"),
                12,
                Some(now),
                now,
                Duration::from_secs(3600)
            ),
            Some(ResyncReason::MissingEmbeddings)
        );
    }

    #[test]
    fn incremental_change_is_detected() {
        let now = Utc::now();
        assert_eq!(
            resync_decision(
                true,
                Some("old"),
                Some("new"),
                0,
                Some(now),
                now,
                Duration::from_secs(3600)
            ),
            Some(ResyncReason::RemoteChanged)
        );
        assert_eq!(
            resync_decision(
                true,
                Some("same"),
                Some("same"),
                0,
                Some(now),
                now,
                Duration::from_secs(3600)
            ),
            None
        );
    }

    #[test]
    fn non_incremental_uses_schedule() {
        let now = Utc::now();
        let stale = now - chrono::Duration::hours(2);
        assert_eq!(
            resync_decision(
                false,
                None,
                None,
                0,
                Some(stale),
                now,
                Duration::from_secs(3600)
            ),
            Some(ResyncReason::ScheduleDue)
        );
        assert_eq!(
            resync_decision(
                false,
                None,
                None,
                0,
                Some(now),
                now,
                Duration::from_secs(3600)
            ),
            None
        );
    }
}
