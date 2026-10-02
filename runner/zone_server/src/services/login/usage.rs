//! How much of each login's subscription is left.

mod availability;
mod snapshot;

pub use availability::Availability;
pub use snapshot::Snapshot;

use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;

use super::router::Chosen;
use crate::db::agent_logins;
use crate::state::AppState;

/// Brings each of `logins`' snapshots up to date when it is older than the configured TTL.
///
/// Stub until usage is read from each agent's service: a stale snapshot is read again from the
/// store, which another server may have written since.
pub async fn refresh(state: &AppState, logins: &mut [Chosen]) {
    let now = Utc::now();
    let ttl = TimeDelta::from_std(state.config().agents.usage_ttl).unwrap_or(TimeDelta::MAX);
    let stale: Vec<&mut Chosen> = logins
        .iter_mut()
        .filter(|chosen| {
            stale(
                chosen.snapshot.as_ref().map(|snapshot| snapshot.fetched_at),
                now,
                ttl,
            )
        })
        .collect();
    join_all(stale.into_iter().map(|chosen| async move {
        match agent_logins::get(state.db(), chosen.login.id).await {
            Ok(Some(stored)) => chosen.snapshot = stored.snapshot(),
            Ok(None) => {}
            Err(error) => tracing::warn!(
                login = %chosen.login.id,
                %error,
                "Could not read the login's usage; routing on the snapshot it had"
            ),
        }
    }))
    .await;
}

fn stale(fetched_at: Option<DateTime<Utc>>, now: DateTime<Utc>, ttl: TimeDelta) -> bool {
    fetched_at.is_none_or(|fetched_at| now.signed_duration_since(fetched_at) >= ttl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_stale_once_it_is_as_old_as_the_ttl_and_a_missing_one_always_is() {
        let now = Utc::now();
        let ttl = TimeDelta::seconds(60);

        assert!(stale(None, now, ttl));
        assert!(!stale(Some(now - TimeDelta::seconds(59)), now, ttl));
        assert!(stale(Some(now - ttl), now, ttl));
    }
}
