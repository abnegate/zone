use std::time::Duration;

use super::loaded::Loaded;
use super::stored::Stored;

/// Every swap advances the generation, so a load that began before a newer
/// swap can tell its result is stale and leave the newer one in place.
#[derive(Debug, Default)]
pub(super) struct Cache {
    loaded: Option<Loaded>,
    generation: u64,
}

impl Cache {
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn stored(&self) -> Option<&Stored> {
        self.loaded.as_ref().map(|loaded| &loaded.stored)
    }

    pub(super) fn fresh(&self, within: Duration) -> Option<&Stored> {
        self.loaded
            .as_ref()
            .filter(|loaded| loaded.is_fresh(within))
            .map(|loaded| &loaded.stored)
    }

    pub(super) fn settled(&self) -> Option<&Stored> {
        self.loaded
            .as_ref()
            .filter(|loaded| loaded.is_settled())
            .map(|loaded| &loaded.stored)
    }

    pub(super) fn fresh_settled(&self, within: Duration) -> Option<&Stored> {
        self.loaded
            .as_ref()
            .filter(|loaded| loaded.is_settled() && loaded.is_fresh(within))
            .map(|loaded| &loaded.stored)
    }

    pub(super) fn swap(&mut self, stored: Stored) {
        self.replace(Loaded::new(stored));
    }

    pub(super) fn swap_since(&mut self, generation: u64, stored: Stored) -> bool {
        self.replace_since(generation, Loaded::new(stored))
    }

    pub(super) fn unsettle_since(&mut self, generation: u64) -> bool {
        self.replace_since(generation, Loaded::unsettled())
    }

    fn replace(&mut self, loaded: Loaded) {
        self.loaded = Some(loaded);
        self.generation += 1;
    }

    fn replace_since(&mut self, generation: u64, loaded: Loaded) -> bool {
        if self.generation != generation {
            return false;
        }
        self.replace(loaded);
        true
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::Error;

    use super::*;

    const INTERVAL: Duration = Duration::from_secs(60);

    #[test]
    fn an_empty_cache_holds_nothing() {
        let cache = Cache::default();
        assert_eq!(cache.stored(), None);
        assert_eq!(cache.fresh(INTERVAL), None);
    }

    #[test]
    fn a_swap_is_held_and_fresh_within_the_interval() {
        let mut cache = Cache::default();
        cache.swap(Stored::Unverified(Error::Signature));
        assert_eq!(cache.stored(), Some(&Stored::Unverified(Error::Signature)));
        assert_eq!(
            cache.fresh(INTERVAL),
            Some(&Stored::Unverified(Error::Signature))
        );
        assert_eq!(cache.fresh(Duration::ZERO), None);
    }

    #[test]
    fn every_swap_advances_the_generation() {
        let mut cache = Cache::default();
        let start = cache.generation();
        cache.swap(Stored::Empty);
        assert_eq!(cache.generation(), start + 1);
        assert!(cache.swap_since(start + 1, Stored::Empty));
        assert_eq!(cache.generation(), start + 2);
    }

    #[test]
    fn an_unsettled_load_answers_as_community_but_is_never_settled() {
        let mut cache = Cache::default();
        assert!(cache.unsettle_since(cache.generation()));

        assert_eq!(cache.stored(), Some(&Stored::Empty));
        assert_eq!(cache.fresh(INTERVAL), Some(&Stored::Empty));
        assert_eq!(cache.settled(), None);
        assert_eq!(cache.fresh_settled(INTERVAL), None);
    }

    #[test]
    fn a_swap_is_settled_while_fresh() {
        let mut cache = Cache::default();
        cache.swap(Stored::Unverified(Error::Signature));
        assert_eq!(
            cache.fresh_settled(INTERVAL),
            Some(&Stored::Unverified(Error::Signature))
        );
        assert_eq!(cache.fresh_settled(Duration::ZERO), None);
        assert_eq!(cache.settled(), Some(&Stored::Unverified(Error::Signature)));
    }

    #[test]
    fn a_load_older_than_the_last_swap_leaves_it_in_place() {
        let mut cache = Cache::default();
        let loading = cache.generation();
        cache.swap(Stored::Unverified(Error::Malformed));

        assert!(!cache.swap_since(loading, Stored::Empty));
        assert_eq!(cache.stored(), Some(&Stored::Unverified(Error::Malformed)));
    }
}
