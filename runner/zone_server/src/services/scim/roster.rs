use std::collections::HashSet;
use std::fmt::Display;
use std::time::{Duration, Instant};

use dashmap::DashSet;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Roster {
    hashes: DashSet<String>,
    loaded: Mutex<Option<Instant>>,
    interval: Duration,
}

impl Roster {
    pub fn new(interval: Duration) -> Self {
        Self {
            hashes: DashSet::new(),
            loaded: Mutex::new(None),
            interval,
        }
    }

    pub fn admit(&self, hash: &str) {
        if !self.hashes.contains(hash) {
            self.hashes.insert(hash.to_owned());
        }
    }

    pub fn forget(&self, hash: &str) {
        self.hashes.remove(hash);
    }

    pub async fn recognizes<Load, Loading, Error>(&self, hash: &str, load: Load) -> bool
    where
        Load: FnOnce() -> Loading,
        Loading: Future<Output = Result<Vec<String>, Error>>,
        Error: Display,
    {
        if self.hashes.contains(hash) {
            return true;
        }
        let Ok(mut loaded) = self.loaded.try_lock() else {
            return false;
        };
        if loaded.is_some_and(|at| at.elapsed() < self.interval) {
            return false;
        }
        *loaded = Some(Instant::now());
        match load().await {
            Ok(hashes) => self.replace(hashes),
            Err(error) => tracing::warn!(%error, "Failed to load the active SCIM token hashes"),
        }
        self.hashes.contains(hash)
    }

    fn replace(&self, hashes: Vec<String>) {
        let active: HashSet<String> = hashes.into_iter().collect();
        self.hashes.retain(|hash| active.contains(hash));
        for hash in active {
            self.hashes.insert(hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const INTERVAL: Duration = Duration::from_secs(60);

    struct Store {
        hashes: Vec<String>,
        loads: AtomicUsize,
    }

    impl Store {
        fn of(hashes: &[&str]) -> Self {
            Self {
                hashes: hashes.iter().map(|hash| (*hash).to_owned()).collect(),
                loads: AtomicUsize::new(0),
            }
        }

        async fn load(&self) -> Result<Vec<String>, &'static str> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(self.hashes.clone())
        }

        fn loads(&self) -> usize {
            self.loads.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn an_active_token_never_seen_before_is_recognized_after_one_load() {
        let roster = Roster::new(INTERVAL);
        let store = Store::of(&["active"]);

        assert!(roster.recognizes("active", || store.load()).await);
        assert!(roster.recognizes("active", || store.load()).await);
        assert_eq!(store.loads(), 1);
    }

    #[tokio::test]
    async fn a_flood_of_unknown_hashes_loads_at_most_once_an_interval() {
        let roster = Roster::new(INTERVAL);
        let store = Store::of(&["active"]);

        for guess in 0..100 {
            assert!(!roster.recognizes(&guess.to_string(), || store.load()).await);
        }
        assert_eq!(store.loads(), 1);
    }

    #[tokio::test]
    async fn a_stale_roster_reloads_and_drops_revoked_tokens() {
        let roster = Roster::new(Duration::ZERO);
        roster.admit("revoked");
        let store = Store::of(&["fresh"]);

        assert!(roster.recognizes("fresh", || store.load()).await);
        assert!(!roster.recognizes("unknown", || store.load()).await);
        assert!(!roster.recognizes("revoked", || store.load()).await);
        assert_eq!(store.loads(), 3);
    }

    #[tokio::test]
    async fn an_admitted_token_is_recognized_until_forgotten() {
        let roster = Roster::new(INTERVAL);
        let store = Store::of(&[]);
        roster.admit("accepted");

        assert!(roster.recognizes("accepted", || store.load()).await);
        roster.forget("accepted");
        assert!(!roster.recognizes("accepted", || store.load()).await);
        assert!(!roster.recognizes("accepted", || store.load()).await);
        assert_eq!(store.loads(), 1);
    }

    #[tokio::test]
    async fn a_failed_load_recognizes_nothing_new_and_waits_an_interval() {
        let roster = Roster::new(INTERVAL);
        let mut loads = 0;

        for _ in 0..3 {
            let recognized = roster
                .recognizes("active", || {
                    loads += 1;
                    async { Err::<Vec<String>, _>("database unavailable") }
                })
                .await;
            assert!(!recognized);
        }
        assert_eq!(loads, 1);
    }
}
