use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};

pub(crate) const LIFETIME: TimeDelta = TimeDelta::hours(1);

pub(crate) struct Cached<T> {
    pub(crate) value: Arc<T>,
    pub(crate) fetched_at: DateTime<Utc>,
}

impl<T> Cached<T> {
    pub(crate) fn fresh(&self, now: DateTime<Utc>) -> Option<Arc<T>> {
        (now - self.fetched_at < LIFETIME).then(|| Arc::clone(&self.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_for_an_hour() {
        let fetched_at = Utc::now();
        let cached = Cached {
            value: Arc::new(1),
            fetched_at,
        };
        assert_eq!(cached.fresh(fetched_at).as_deref(), Some(&1));
        assert!(
            cached
                .fresh(fetched_at + LIFETIME - TimeDelta::seconds(1))
                .is_some()
        );
        assert!(cached.fresh(fetched_at + LIFETIME).is_none());
    }
}
