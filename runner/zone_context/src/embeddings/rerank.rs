//! Cross-encoder rerank: joint (query, document) scoring.
//!
//! Neural scores are applied only after a probe proves the scorer can order a
//! known pair. Ollama `/api/rerank` does not exist, and generate-yes/no on a
//! Qwen3 GGUF is not a logit reranker — those paths are not attached by default.

mod http_cross_encoder;

use std::sync::Arc;

use async_trait::async_trait;

pub use http_cross_encoder::HttpCrossEncoder;

use crate::error::Result;

const FALLBACK_SCORE: f32 = 0.4;
const URL_VARIABLE: &str = "RERANK_URL";

#[async_trait]
pub trait CrossEncoder: Send + Sync {
    async fn score_pair(&self, query: &str, document: &str) -> Result<f32>;

    async fn score_documents(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        let mut scores = Vec::with_capacity(documents.len());
        for document in documents {
            scores.push(
                self.score_pair(query, document)
                    .await
                    .unwrap_or(FALLBACK_SCORE),
            );
        }
        Ok(scores)
    }
}

/// Attach a neural cross-encoder only when a scorer actually ranks.
pub async fn probe_cross_encoder(ollama_host: &str) -> Option<Arc<dyn CrossEncoder>> {
    if let Ok(url) = std::env::var(URL_VARIABLE) {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty()
            && let Some(encoder) = HttpCrossEncoder::probe_url(&trimmed, None).await
        {
            return Some(encoder);
        }
    }
    if let Some(encoder) = HttpCrossEncoder::probe(ollama_host).await {
        return Some(encoder);
    }
    #[cfg(feature = "local-embeddings")]
    {
        if let Some(encoder) = super::local_rerank::LocalCrossEncoder::probe().await {
            return Some(Arc::new(encoder));
        }
    }
    None
}
