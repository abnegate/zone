//! Installation tokens held in memory for as long as they are safe to use.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};

use super::identifier::InstallationId;
use super::token::InstallationToken;

/// A token per installation, dropped once its safety margin is reached.
///
/// Every accessor returns owned data so no guard is ever held across an await.
#[derive(Debug, Default)]
pub struct TokenCache {
    entries: RwLock<HashMap<InstallationId, InstallationToken>>,
}

impl TokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The token for `installation` if one is held and still usable at `now`.
    pub fn get(
        &self,
        installation: InstallationId,
        now: DateTime<Utc>,
    ) -> Option<InstallationToken> {
        let entries = self.entries.read().ok()?;
        entries
            .get(&installation)
            .filter(|token| token.is_usable_at(now))
            .cloned()
    }

    pub fn insert(&self, installation: InstallationId, token: InstallationToken) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(installation, token);
        }
    }

    /// Forget the token for `installation`, so the next request mints a new one.
    pub fn forget(&self, installation: InstallationId) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(&installation);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use zone_core::secret::SecretValue;

    use crate::services::github_app::testing::at;

    const NOW: i64 = 1_700_000_000;

    fn token(value: &str, minutes: i64) -> InstallationToken {
        InstallationToken::new(
            SecretValue::new(value),
            at(NOW) + TimeDelta::minutes(minutes),
        )
    }

    #[test]
    fn an_empty_cache_holds_nothing() {
        let cache = TokenCache::new();
        assert!(cache.get(InstallationId::new(1), at(NOW)).is_none());
    }

    #[test]
    fn a_cached_token_is_returned_again() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_first", 60));

        let first = cache.get(InstallationId::new(1), at(NOW)).expect("cached");
        let second = cache.get(InstallationId::new(1), at(NOW)).expect("cached");

        assert_eq!(first.secret().expose(), "ghs_first");
        assert_eq!(second.secret().expose(), "ghs_first");
    }

    #[test]
    fn a_token_near_expiry_is_withheld_rather_than_served() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_nearly_spent", 60));

        assert!(
            cache.get(InstallationId::new(1), at(NOW)).is_some(),
            "a fresh token is served"
        );
        assert!(
            cache
                .get(InstallationId::new(1), at(NOW) + TimeDelta::minutes(56))
                .is_none(),
            "four minutes of life is inside the safety margin"
        );
    }

    #[test]
    fn installations_do_not_share_a_token() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_one", 60));
        cache.insert(InstallationId::new(2), token("ghs_two", 60));

        assert_eq!(
            cache
                .get(InstallationId::new(1), at(NOW))
                .expect("cached")
                .secret()
                .expose(),
            "ghs_one"
        );
        assert_eq!(
            cache
                .get(InstallationId::new(2), at(NOW))
                .expect("cached")
                .secret()
                .expose(),
            "ghs_two"
        );
    }

    #[test]
    fn a_replacement_supersedes_the_token_it_replaces() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_old", 60));
        cache.insert(InstallationId::new(1), token("ghs_new", 60));

        assert_eq!(
            cache
                .get(InstallationId::new(1), at(NOW))
                .expect("cached")
                .secret()
                .expose(),
            "ghs_new"
        );
    }

    #[test]
    fn forgetting_and_clearing_empty_the_cache() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_one", 60));
        cache.insert(InstallationId::new(2), token("ghs_two", 60));

        cache.forget(InstallationId::new(1));
        assert!(cache.get(InstallationId::new(1), at(NOW)).is_none());
        assert!(cache.get(InstallationId::new(2), at(NOW)).is_some());

        cache.clear();
        assert!(cache.get(InstallationId::new(2), at(NOW)).is_none());
    }

    #[test]
    fn the_cache_never_renders_the_tokens_it_holds() {
        let cache = TokenCache::new();
        cache.insert(InstallationId::new(1), token("ghs_secret_value", 60));

        let rendered = format!("{cache:?}");
        assert!(!rendered.contains("ghs_secret_value"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
