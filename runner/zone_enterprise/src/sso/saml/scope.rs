use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

pub(super) struct Scope<K, V> {
    bindings: HashMap<K, Vec<V>>,
    log: Vec<K>,
}

impl<K: Eq + Hash + Clone, V> Scope<K, V> {
    pub(super) fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            log: Vec::new(),
        }
    }

    pub(super) fn bind(&mut self, key: K, value: V) {
        self.bindings.entry(key.clone()).or_default().push(value);
        self.log.push(key);
    }

    pub(super) fn lookup<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.bindings.get(key).and_then(|values| values.last())
    }

    pub(super) fn mark(&self) -> usize {
        self.log.len()
    }

    pub(super) fn restore(&mut self, mark: usize) {
        while self.log.len() > mark {
            if let Some(key) = self.log.pop()
                && let Some(values) = self.bindings.get_mut(&key)
            {
                values.pop();
            }
        }
    }
}
