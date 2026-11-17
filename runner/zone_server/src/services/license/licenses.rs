use std::sync::{OnceLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::Mutex;
use zone_enterprise::{Keyring, License, Seats, Status, period_start};

use super::cache::Cache;
use super::change::Change;
use super::error::SetError;
use super::stored::Stored;
use crate::db::{DbResult, licenses, seats};

const RELOAD_AFTER: Duration = Duration::from_secs(60);

/// Caches the stored key as it last verified, never a status: a status depends
/// on the moment it is read.
///
/// The cache lock is never held across a database call, so a slow or
/// unreachable database cannot stall a reader. `refresh` lets one caller
/// reload at a time and keeps set and remove from interleaving with it.
pub struct Licenses {
    keyring: OnceLock<Keyring>,
    cache: RwLock<Cache>,
    refresh: Mutex<()>,
    reload_after: Duration,
}

impl Default for Licenses {
    fn default() -> Self {
        Self::new()
    }
}

impl Licenses {
    pub fn new() -> Self {
        Self {
            keyring: OnceLock::new(),
            cache: RwLock::new(Cache::default()),
            refresh: Mutex::new(()),
            reload_after: RELOAD_AFTER,
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn with_reload_after(reload_after: Duration) -> Self {
        Self {
            reload_after,
            ..Self::new()
        }
    }

    pub fn keyring(&self) -> &Keyring {
        self.keyring.get_or_init(Keyring::production)
    }

    /// Panics if a keyring is already in place.
    #[cfg(any(test, feature = "testing"))]
    pub fn install_keyring(&self, keyring: Keyring) {
        assert!(
            self.keyring.set(keyring).is_ok(),
            "install_keyring must run before the first keyring() call"
        );
    }

    pub async fn load(&self, pool: &PgPool) -> DbResult<Stored> {
        let generation = self.read().generation();
        let stored = self.fetch(pool).await?;
        self.write().swap_since(generation, stored.clone());
        Ok(stored)
    }

    /// The cached license, reloaded once it is older than the reload interval.
    ///
    /// While one caller reloads, the others are answered from the cache, or as
    /// Community if nothing has loaded yet. A failed reload keeps the last key
    /// that loaded and waits a full interval before trying again, so an
    /// unreachable database is not asked on every read.
    pub async fn current(&self, pool: &PgPool) -> Option<License> {
        if let Some(stored) = self.fresh() {
            return stored.into_license();
        }
        let Ok(_refresh) = self.refresh.try_lock() else {
            return self.cached().and_then(Stored::into_license);
        };
        if let Some(stored) = self.fresh() {
            return stored.into_license();
        }
        let generation = self.read().generation();
        let stored = match self.fetch(pool).await {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(%error, "Could not reload the license; the last one that loaded stays in force");
                self.cached().unwrap_or(Stored::Empty)
            }
        };
        self.write().swap_since(generation, stored);
        self.cached().and_then(Stored::into_license)
    }

    pub async fn set(
        &self,
        pool: &PgPool,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Change, SetError> {
        let current = self.keyring().verify(text)?;
        if current.status(now) == Status::Lapsed {
            return Err(SetError::Lapsed);
        }
        let _refresh = self.refresh.lock().await;
        let previous = self.verify(licenses::replace(pool, current.text()).await?);
        self.write().swap(Stored::Verified(current.clone()));
        Ok(Change { previous, current })
    }

    pub async fn remove(&self, pool: &PgPool) -> DbResult<Stored> {
        let _refresh = self.refresh.lock().await;
        let removed = self.verify(licenses::remove(pool).await?);
        self.write().swap(Stored::Empty);
        Ok(removed)
    }

    pub async fn seats(
        &self,
        pool: &PgPool,
        license: Option<&License>,
        now: DateTime<Utc>,
    ) -> DbResult<Seats> {
        Ok(Seats {
            used: seats::used(pool, period_start(now)).await?,
            licensed: license.map(License::seats),
        })
    }

    async fn fetch(&self, pool: &PgPool) -> DbResult<Stored> {
        let stored = self.verify(licenses::get(pool).await?);
        if let Some(error) = stored.refusal() {
            tracing::warn!(
                code = error.code(),
                "The stored license key does not verify, so this server runs as Community until an admin replaces or removes it"
            );
        }
        Ok(stored)
    }

    fn verify(&self, text: Option<String>) -> Stored {
        match text.map(|text| self.keyring().verify(&text)) {
            None => Stored::Empty,
            Some(Ok(license)) => Stored::Verified(license),
            Some(Err(error)) => Stored::Unverified(error),
        }
    }

    fn fresh(&self) -> Option<Stored> {
        self.read().fresh(self.reload_after).cloned()
    }

    fn cached(&self) -> Option<Stored> {
        self.read().stored().cloned()
    }

    fn read(&self) -> RwLockReadGuard<'_, Cache> {
        self.cache.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Cache> {
        self.cache.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeDelta;
    use sqlx::postgres::PgPoolOptions;
    use tokio::time::timeout;
    use uuid::Uuid;
    use zone_enterprise::{Payload, SigningKey, Tier, TrustedKey, sign};

    use super::*;
    use crate::state::unreachable_pool;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [0x11; 32];
    const UNREACHABLE: &str = "postgres://127.0.0.1:1/unreachable";
    const STALLED: Duration = Duration::from_secs(30);
    const PROMPTLY: Duration = Duration::from_secs(1);
    const BRIEFLY: Duration = Duration::from_millis(100);

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&SEED)
    }

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, signing().verifying_key().to_bytes()).unwrap()])
            .unwrap()
    }

    fn licenses() -> Licenses {
        let licenses = Licenses::new();
        licenses.install_keyring(keyring());
        licenses
    }

    fn key(expires_in: TimeDelta) -> String {
        let now = Utc::now();
        sign(
            &Payload {
                id: Uuid::new_v4(),
                customer: "Acme Ltd".into(),
                tier: Tier::Team,
                seats: 5,
                issued_at: now - TimeDelta::days(400),
                expires_at: now + expires_in,
                key_id: KEY_ID.into(),
            },
            &signing(),
        )
    }

    fn stale(license: &License) -> Licenses {
        let licenses = Licenses::with_reload_after(Duration::ZERO);
        licenses.install_keyring(keyring());
        licenses.write().swap(Stored::Verified(license.clone()));
        licenses
    }

    fn stalled_pool() -> PgPool {
        PgPoolOptions::new()
            .acquire_timeout(STALLED)
            .connect_lazy(UNREACHABLE)
            .expect("a lazy pool needs no server")
    }

    #[test]
    fn the_keyring_defaults_to_production() {
        assert_eq!(Licenses::new().keyring(), &Keyring::production());
    }

    #[test]
    fn an_installed_keyring_replaces_production() {
        assert_eq!(licenses().keyring(), &keyring());
    }

    #[test]
    #[should_panic(expected = "install_keyring must run before the first keyring() call")]
    fn installing_a_keyring_after_one_is_in_use_panics() {
        let licenses = Licenses::new();
        let _ = licenses.keyring();
        licenses.install_keyring(keyring());
    }

    #[test]
    #[should_panic(expected = "install_keyring must run before the first keyring() call")]
    fn installing_a_second_keyring_panics() {
        let licenses = licenses();
        licenses.install_keyring(keyring());
    }

    #[test]
    fn a_stored_key_is_verified_against_the_keyring() {
        let licenses = licenses();
        let text = key(TimeDelta::days(30));

        assert_eq!(licenses.verify(None), Stored::Empty);
        assert_eq!(
            licenses.verify(Some(text.clone())),
            Stored::Verified(keyring().verify(&text).unwrap())
        );
        assert_eq!(
            licenses.verify(Some("not a key".into())),
            Stored::Unverified(zone_enterprise::Error::Malformed)
        );
    }

    #[tokio::test]
    async fn a_malformed_key_is_refused_before_the_database() {
        let error = licenses()
            .set(&unreachable_pool(), "not a key", Utc::now())
            .await
            .unwrap_err();
        assert!(
            matches!(error, SetError::Refused(zone_enterprise::Error::Malformed)),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_key_past_grace_is_refused_before_the_database() {
        let error = licenses()
            .set(&unreachable_pool(), &key(-TimeDelta::days(31)), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(error, SetError::Lapsed), "{error:?}");
    }

    #[tokio::test]
    async fn a_key_in_grace_reaches_the_database() {
        let error = licenses()
            .set(&unreachable_pool(), &key(-TimeDelta::days(1)), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(error, SetError::Database(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_failed_set_leaves_nothing_cached() {
        let licenses = licenses();
        let pool = unreachable_pool();
        assert!(
            licenses
                .set(&pool, &key(TimeDelta::days(30)), Utc::now())
                .await
                .is_err()
        );
        assert_eq!(licenses.cached(), None);
        assert_eq!(licenses.current(&pool).await, None);
    }

    #[tokio::test]
    async fn nothing_loaded_and_an_unreachable_database_is_community() {
        assert_eq!(licenses().current(&unreachable_pool()).await, None);
    }

    #[tokio::test]
    async fn load_returns_the_database_error() {
        assert!(licenses().load(&unreachable_pool()).await.is_err());
    }

    #[tokio::test]
    async fn remove_returns_the_database_error() {
        assert!(licenses().remove(&unreachable_pool()).await.is_err());
    }

    #[tokio::test]
    async fn seats_return_the_database_error() {
        assert!(
            licenses()
                .seats(&unreachable_pool(), None, Utc::now())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_reader_is_answered_from_the_cache_while_another_caller_reloads() {
        let license = keyring().verify(&key(TimeDelta::days(30))).unwrap();
        let licenses = stale(&license);
        let _reloading = licenses.refresh.lock().await;

        let current = timeout(PROMPTLY, licenses.current(&stalled_pool()))
            .await
            .expect("the reader waited for the reload");

        assert_eq!(current, Some(license));
    }

    #[tokio::test]
    async fn a_reader_is_not_held_up_by_a_reload_stalled_on_the_database() {
        let license = keyring().verify(&key(TimeDelta::days(30))).unwrap();
        let licenses = Arc::new(stale(&license));
        let pool = stalled_pool();
        let reload = tokio::spawn({
            let licenses = Arc::clone(&licenses);
            let pool = pool.clone();
            async move { licenses.current(&pool).await }
        });
        timeout(PROMPTLY, async {
            while licenses.refresh.try_lock().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the reload never started");

        let current = timeout(PROMPTLY, licenses.current(&pool))
            .await
            .expect("the reader waited for the stalled reload");

        assert_eq!(current, Some(license));
        assert!(!reload.is_finished(), "the reload was not stalled");
        reload.abort();
    }

    #[tokio::test]
    async fn nothing_loaded_while_another_caller_reloads_is_community() {
        let licenses = licenses();
        let _reloading = licenses.refresh.lock().await;

        let current = timeout(PROMPTLY, licenses.current(&stalled_pool()))
            .await
            .expect("the reader waited for the reload");

        assert_eq!(current, None);
    }

    #[tokio::test]
    async fn a_failed_reload_keeps_the_last_license_that_loaded() {
        let license = keyring().verify(&key(TimeDelta::days(30))).unwrap();
        let licenses = stale(&license);

        assert_eq!(
            licenses.current(&unreachable_pool()).await,
            Some(license.clone())
        );
        assert_eq!(licenses.cached(), Some(Stored::Verified(license)));
    }

    #[tokio::test]
    async fn set_and_remove_wait_for_a_reload_in_flight() {
        let licenses = licenses();
        let reloading = licenses.refresh.lock().await;
        let pool = unreachable_pool();
        let text = key(TimeDelta::days(30));

        assert!(
            timeout(BRIEFLY, licenses.set(&pool, &text, Utc::now()))
                .await
                .is_err()
        );
        assert!(timeout(BRIEFLY, licenses.remove(&pool)).await.is_err());
        drop(reloading);
    }
}
