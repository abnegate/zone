//! Process-wide query-embedding cache.
//!
//! Search embeds the same questions over and over (eval, agents, retries).
//! Hitting Ollama for each one is why live p50 sat at 10s under modest
//! concurrency.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::EmbeddingService;
use super::query::embed_query_text;
use crate::error::Result;

const CAPACITY: usize = 512;

struct Entry {
    tick: u64,
    vector: Vec<f32>,
}

struct Cache {
    tick: u64,
    entries: HashMap<String, Entry>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            tick: 0,
            entries: HashMap::new(),
        })
    })
}

pub fn cache_get(key: &str) -> Option<Vec<f32>> {
    let mut cache = cache().lock().ok()?;
    if !cache.entries.contains_key(key) {
        return None;
    }
    cache.tick += 1;
    let tick = cache.tick;
    let entry = cache.entries.get_mut(key)?;
    entry.tick = tick;
    Some(entry.vector.clone())
}

pub fn cache_put(key: String, vector: Vec<f32>) {
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    cache.tick += 1;
    if cache.entries.len() >= CAPACITY {
        if let Some(oldest) = cache
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.tick)
            .map(|(key, _)| key.clone())
        {
            cache.entries.remove(&oldest);
        }
    }
    let tick = cache.tick;
    cache.entries.insert(key, Entry { tick, vector });
}

pub async fn embed_cached(service: &dyn EmbeddingService, query: &str) -> Result<Vec<f32>> {
    let text = embed_query_text(service.model(), query);
    if let Some(hit) = cache_get(&text) {
        return Ok(hit);
    }
    let vector = service.embed(&text).await?;
    cache_put(text, vector.clone());
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_a_vector() {
        cache_put("probe-key".into(), vec![0.25, 0.5]);
        assert_eq!(cache_get("probe-key"), Some(vec![0.25, 0.5]));
    }
}
