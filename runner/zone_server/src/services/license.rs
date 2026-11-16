//! The server's license, verified offline against the trusted keyring

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;
use zone_enterprise::{Keyring, License, Seats, Status, period_start};

use crate::db::{DbResult, licenses, seats};

pub const LAPSED: &str = "license_lapsed";

const RELOAD_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum SetError {
    #[error(transparent)]
    Refused(#[from] zone_enterprise::Error),
    #[error("The license key's grace period has ended. Paste a renewed key.")]
    Lapsed,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl SetError {
    /// The refusal code a client is told; a database failure is not a refusal.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Refused(error) => Some(error.code()),
            Self::Lapsed => Some(LAPSED),
            Self::Database(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub previous: Option<License>,
    pub current: License,
}

struct Loaded {
    license: Option<License>,
    at: Instant,
}

impl Loaded {
    fn new(license: Option<License>) -> Self {
        Self {
            license,
            at: Instant::now(),
        }
    }

    fn is_fresh(&self, within: Duration) -> bool {
        self.at.elapsed() < within
    }
}

/// Caches the last license that verified, never a status: a status depends on
/// the moment it is read.
pub struct Licenses {
    keyring: OnceLock<Keyring>,
    cache: RwLock<Option<Loaded>>,
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
            cache: RwLock::new(None),
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

    /// Reads and verifies the stored key, always from the database.
    pub async fn load(&self, pool: &PgPool) -> DbResult<Option<License>> {
        let mut cache = self.cache.write().await;
        let license = self.fetch(pool).await?;
        *cache = Some(Loaded::new(license.clone()));
        Ok(license)
    }

    /// The cached license, reloaded once it is older than the reload interval.
    ///
    /// A failed reload keeps the last license that loaded and waits a full
    /// interval before trying again, so an unreachable database is not asked
    /// on every read.
    pub async fn current(&self, pool: &PgPool) -> Option<License> {
        if let Some(loaded) = self.cache.read().await.as_ref()
            && loaded.is_fresh(self.reload_after)
        {
            return loaded.license.clone();
        }
        let mut cache = self.cache.write().await;
        if let Some(loaded) = cache.as_ref()
            && loaded.is_fresh(self.reload_after)
        {
            return loaded.license.clone();
        }
        let license = match self.fetch(pool).await {
            Ok(license) => license,
            Err(error) => {
                tracing::warn!(%error, "Could not reload the license; the last one that loaded stays in force");
                cache.as_ref().and_then(|loaded| loaded.license.clone())
            }
        };
        *cache = Some(Loaded::new(license.clone()));
        license
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
        let mut cache = self.cache.write().await;
        let previous = self.fetch(pool).await?;
        *cache = Some(Loaded::new(previous.clone()));
        licenses::set(pool, current.text()).await?;
        *cache = Some(Loaded::new(Some(current.clone())));
        Ok(Change { previous, current })
    }

    /// Removes the stored key, returning its license if it still verified.
    pub async fn remove(&self, pool: &PgPool) -> DbResult<Option<License>> {
        let mut cache = self.cache.write().await;
        let removed = licenses::remove(pool).await?;
        *cache = Some(Loaded::new(None));
        Ok(removed.and_then(|text| self.keyring().verify(&text).ok()))
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

    async fn fetch(&self, pool: &PgPool) -> DbResult<Option<License>> {
        let stored = licenses::get(pool).await?;
        Ok(stored.and_then(|text| self.verify_stored(&text)))
    }

    fn verify_stored(&self, text: &str) -> Option<License> {
        self.keyring()
            .verify(text)
            .inspect_err(|error| {
                tracing::warn!(
                    code = error.code(),
                    "The stored license key does not verify, so this server runs as Community until an admin replaces or removes it"
                );
            })
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Uuid;
    use zone_enterprise::{
        MALFORMED, PAYLOAD_INVALID, Payload, SIGNATURE_INVALID, SigningKey, Tier, TrustedKey,
        UNKNOWN_KEY, sign,
    };

    use super::*;
    use crate::state::unreachable_pool;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [0x11; 32];

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&SEED)
    }

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, signing().verifying_key().to_bytes()).unwrap()])
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

    #[test]
    fn lapsed_code_is_the_wire_contract() {
        assert_eq!(LAPSED, "license_lapsed");
    }

    #[test]
    fn each_refusal_returns_its_code() {
        let cases = [
            (
                SetError::Refused(zone_enterprise::Error::Malformed),
                MALFORMED,
            ),
            (
                SetError::Refused(zone_enterprise::Error::UnknownKey { id: "x".into() }),
                UNKNOWN_KEY,
            ),
            (
                SetError::Refused(zone_enterprise::Error::Signature),
                SIGNATURE_INVALID,
            ),
            (
                SetError::Refused(zone_enterprise::Error::Payload("seats".into())),
                PAYLOAD_INVALID,
            ),
            (SetError::Lapsed, LAPSED),
        ];
        for (error, code) in cases {
            assert_eq!(error.code(), Some(code), "{error:?}");
        }
    }

    #[test]
    fn a_database_failure_has_no_refusal_code() {
        assert_eq!(SetError::Database(sqlx::Error::PoolClosed).code(), None);
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
}
