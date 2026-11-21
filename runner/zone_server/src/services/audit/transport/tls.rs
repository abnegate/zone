use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use rustls::ClientConfig;
use uuid::Uuid;

use super::Failure;

pub const CAPACITY: usize = 256;

/// TLS client configurations built from each destination's secrets, so certificates are parsed
/// and the platform's roots loaded once per version of a destination rather than per delivery.
/// A destination's entry is replaced when it changes; past `CAPACITY` destinations, the least
/// recently used is dropped.
#[derive(Default)]
pub struct Configurations {
    entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
    cached: HashMap<Uuid, Cached>,
    clock: u64,
}

struct Cached {
    updated_at: DateTime<Utc>,
    config: Arc<ClientConfig>,
    used: u64,
}

impl Configurations {
    pub fn get(
        &self,
        id: Uuid,
        updated_at: DateTime<Utc>,
        build: impl FnOnce() -> Result<Arc<ClientConfig>, Failure>,
    ) -> Result<Arc<ClientConfig>, Failure> {
        if let Some(config) = self.lock().hit(id, updated_at) {
            return Ok(config);
        }
        let config = build()?;
        self.lock().insert(id, updated_at, Arc::clone(&config));
        Ok(config)
    }

    pub fn len(&self) -> usize {
        self.lock().cached.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Entries {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn hit(&mut self, id: Uuid, updated_at: DateTime<Utc>) -> Option<Arc<ClientConfig>> {
        let now = self.tick();
        let cached = self
            .cached
            .get_mut(&id)
            .filter(|cached| cached.updated_at == updated_at)?;
        cached.used = now;
        Some(Arc::clone(&cached.config))
    }

    fn insert(&mut self, id: Uuid, updated_at: DateTime<Utc>, config: Arc<ClientConfig>) {
        if self
            .cached
            .get(&id)
            .is_some_and(|cached| cached.updated_at > updated_at)
        {
            return;
        }
        if !self.cached.contains_key(&id) && self.cached.len() >= CAPACITY {
            let least = self
                .cached
                .iter()
                .min_by_key(|(_, cached)| cached.used)
                .map(|(id, _)| *id);
            if let Some(least) = least {
                self.cached.remove(&least);
            }
        }
        let used = self.tick();
        self.cached.insert(
            id,
            Cached {
                updated_at,
                config,
                used,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use chrono::TimeDelta;
    use rustls::RootCertStore;
    use rustls::crypto::aws_lc_rs;

    use super::*;

    fn built(count: &Cell<usize>) -> Result<Arc<ClientConfig>, Failure> {
        count.set(count.get() + 1);
        Ok(Arc::new(
            ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(RootCertStore::empty())
                .with_no_client_auth(),
        ))
    }

    #[test]
    fn a_destination_is_built_once_until_it_changes() {
        let configurations = Configurations::default();
        let builds = Cell::new(0);
        let id = Uuid::new_v4();
        let saved = Utc::now();

        let first = configurations.get(id, saved, || built(&builds)).unwrap();
        let again = configurations.get(id, saved, || built(&builds)).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(builds.get(), 1);

        let changed = saved + TimeDelta::seconds(1);
        let rebuilt = configurations.get(id, changed, || built(&builds)).unwrap();
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        assert_eq!(builds.get(), 2);
        assert_eq!(configurations.len(), 1, "the old version is evicted");

        let stale = configurations.get(id, saved, || built(&builds)).unwrap();
        assert!(!Arc::ptr_eq(&stale, &rebuilt));
        let current = configurations.get(id, changed, || built(&builds)).unwrap();
        assert!(
            Arc::ptr_eq(&current, &rebuilt),
            "a stale read never replaces a newer version"
        );
    }

    #[test]
    fn a_failed_build_is_not_cached() {
        let configurations = Configurations::default();
        let id = Uuid::new_v4();
        let failure = Failure::of(zone_enterprise::audit::FailureCode::Configuration);
        assert_eq!(
            configurations
                .get(id, Utc::now(), || Err(failure.clone()))
                .unwrap_err(),
            failure
        );
        assert!(configurations.is_empty());
    }

    #[test]
    fn the_least_recently_used_destination_goes_first_past_capacity() {
        let configurations = Configurations::default();
        let builds = Cell::new(0);
        let saved = Utc::now();
        let ids: Vec<Uuid> = (0..CAPACITY).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            configurations.get(*id, saved, || built(&builds)).unwrap();
        }
        configurations
            .get(ids[0], saved, || built(&builds))
            .unwrap();
        configurations
            .get(Uuid::new_v4(), saved, || built(&builds))
            .unwrap();
        assert_eq!(configurations.len(), CAPACITY);

        let before = builds.get();
        configurations
            .get(ids[0], saved, || built(&builds))
            .unwrap();
        assert_eq!(builds.get(), before, "the recently used entry stayed");
        configurations
            .get(ids[1], saved, || built(&builds))
            .unwrap();
        assert_eq!(
            builds.get(),
            before + 1,
            "the least recently used was dropped"
        );
    }
}
