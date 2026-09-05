//! Cross-encoder rerank: joint (query, document) scoring.
//!
//! A lexical cross-encoder always runs in the learned ranker. Neural scores
//! are applied only after a probe proves the scorer can order a known pair.
//! Ollama `/api/rerank` does not exist, and generate-yes/no on a Qwen3
//! GGUF is not a logit reranker — those paths are not attached by default.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{ContextError, Result};

/// Blend learned-ranker logits with a 0-1 cross-encoder score.
pub fn blend_rank(ltr: f32, cross_encoder: f32) -> f32 {
    0.62 * ltr + 0.38 * cross_encoder
}

/// Joint query-document score used when no neural reranker is configured.
pub fn lexical_cross_score(query: &str, uri: &str, title: &str, text: &str) -> f32 {
    let rewritten = crate::embeddings::rewrite_query(query);
    let query_l = query.to_ascii_lowercase();
    let uri_l = uri.to_ascii_lowercase();
    let title_l = title.to_ascii_lowercase();
    let text_l = text.to_ascii_lowercase();

    let role = crate::embeddings::ranker::identifier_role(text, uri, &rewritten.identifiers);
    let mut ident: f32 = match role {
        crate::embeddings::ranker::IdentifierRole::Exported => 0.7,
        crate::embeddings::ranker::IdentifierRole::Defined => 0.55,
        crate::embeddings::ranker::IdentifierRole::Mention => 0.16,
        crate::embeddings::ranker::IdentifierRole::None => 0.0,
    };
    for id in &rewritten.identifiers {
        let id_l = id.to_ascii_lowercase();
        if uri_l.contains(&id_l) {
            ident += 0.12;
        } else if title_l.contains(&id_l) {
            ident += 0.08;
        }
    }
    ident = ident.min(0.75);

    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|t| t.len() >= 2)
        .collect();
    let coverage = if tokens.is_empty() {
        0.0
    } else {
        tokens
            .iter()
            .filter(|token| text_l.contains(&token.to_ascii_lowercase()))
            .count() as f32
            / tokens.len() as f32
    };

    let phrase = if query_l.len() >= 8 && text_l.contains(&query_l) {
        0.15
    } else {
        0.0
    };
    let title_hit = if tokens
        .iter()
        .any(|token| title_l.contains(&token.to_ascii_lowercase()))
    {
        0.08
    } else {
        0.0
    };
    let path_hit = if tokens
        .iter()
        .any(|token| uri_l.contains(&token.to_ascii_lowercase()))
    {
        0.07
    } else {
        0.0
    };

    let mut bridge = 0.0f32;
    for term in crate::embeddings::nl_bridge_terms(query) {
        if term.contains('_') && text_l.contains(&term) {
            bridge += 0.12;
        }
    }
    bridge = bridge.min(0.24);

    (ident + 0.22 * coverage + phrase + title_hit + path_hit + bridge).clamp(0.0, 1.0)
}

#[async_trait]
pub trait CrossEncoder: Send + Sync {
    async fn score_pair(&self, query: &str, document: &str) -> Result<f32>;

    async fn score_documents(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        let mut scores = Vec::with_capacity(documents.len());
        for document in documents {
            scores.push(self.score_pair(query, document).await.unwrap_or(0.4));
        }
        Ok(scores)
    }
}

/// Min-max normalize a candidate batch so neural logits and 0-1 scores blend.
pub fn min_max_norm(scores: &[f32]) -> Vec<f32> {
    let Some(min) = scores
        .iter()
        .copied()
        .filter(|s| s.is_finite())
        .reduce(f32::min)
    else {
        return vec![0.5; scores.len()];
    };
    let max = scores
        .iter()
        .copied()
        .filter(|s| s.is_finite())
        .reduce(f32::max)
        .unwrap_or(min);
    if (max - min).abs() < 1e-5 {
        return vec![0.5; scores.len()];
    }
    scores
        .iter()
        .map(|score| ((score - min) / (max - min)).clamp(0.0, 1.0))
        .collect()
}

/// Attach a neural cross-encoder only when a scorer actually ranks.
pub async fn probe_cross_encoder(ollama_host: &str) -> Option<Arc<dyn CrossEncoder>> {
    if let Ok(url) = std::env::var("RERANK_URL") {
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

pub struct HttpCrossEncoder {
    client: reqwest::Client,
    rerank_url: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct ApiRerankResponse {
    results: Option<Vec<ApiRerankHit>>,
}

#[derive(Debug, Deserialize)]
struct ApiRerankHit {
    index: Option<usize>,
    relevance_score: Option<f32>,
    #[serde(alias = "score")]
    score: Option<f32>,
}

impl HttpCrossEncoder {
    pub fn new(rerank_url: &str, model: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(rerank_timeout_ms()))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ContextError::Config(format!("rerank client: {e}")))?;
        Ok(Self {
            client,
            rerank_url: rerank_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Probe a host for `/api/rerank` or `/v1/rerank`. Model-name matching is
    /// not enough — the endpoint must return a discriminative pair score.
    pub async fn probe(base_url: &str) -> Option<Arc<dyn CrossEncoder>> {
        let model = std::env::var("RERANK_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "rerank".into());
        let base = base_url.trim_end_matches('/');
        for path in ["/v1/rerank", "/api/rerank", "/rerank"] {
            if let Some(encoder) = Self::probe_url(&format!("{base}{path}"), Some(&model)).await {
                return Some(encoder);
            }
        }
        None
    }

    pub async fn probe_url(url: &str, model: Option<&str>) -> Option<Arc<dyn CrossEncoder>> {
        let model = model
            .map(str::to_string)
            .or_else(|| std::env::var("RERANK_MODEL").ok())
            .unwrap_or_else(|| "rerank".into());
        let encoder = Self::new(url, &model).ok()?;
        let scores = encoder
            .score_documents(
                "What does should_skip_blob do?",
                &[
                    "fn should_skip_blob(uri: &str, sha: &str) -> bool",
                    "generic authentication helper with a login form",
                ],
            )
            .await
            .ok()?;
        if scores.len() == 2 && scores[0] > scores[1] + 0.02 {
            tracing::info!(url, model = %encoder.model, "using HTTP cross-encoder reranker");
            Some(Arc::new(encoder))
        } else {
            None
        }
    }

    async fn score_via_rerank_api(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let body = serde_json::json!({
            "model": self.model,
            "query": query,
            "documents": documents,
        });
        let response = self
            .client
            .post(&self.rerank_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ContextError::Embedding(format!("rerank request: {e}")))?;
        if !response.status().is_success() {
            return Err(ContextError::Embedding(format!(
                "rerank API {}",
                response.status()
            )));
        }
        let parsed = response
            .json::<ApiRerankResponse>()
            .await
            .map_err(|e| ContextError::Embedding(format!("rerank parse: {e}")))?;
        let hits = parsed.results.unwrap_or_default();
        if hits.is_empty() {
            return Err(ContextError::Embedding("empty rerank response".into()));
        }
        let mut scores = vec![0.0; documents.len()];
        for hit in hits {
            if let Some(index) = hit.index
                && let Some(slot) = scores.get_mut(index)
            {
                *slot = hit.relevance_score.or(hit.score).unwrap_or(0.0);
            }
        }
        Ok(scores)
    }
}

#[async_trait]
impl CrossEncoder for HttpCrossEncoder {
    async fn score_pair(&self, query: &str, document: &str) -> Result<f32> {
        let clipped: String = document.chars().take(1200).collect();
        let scores = self.score_via_rerank_api(query, &[&clipped]).await?;
        scores
            .into_iter()
            .next()
            .ok_or_else(|| ContextError::Embedding("empty rerank pair".into()))
    }

    async fn score_documents(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        self.score_via_rerank_api(query, documents).await
    }
}

/// Back-compat name used by older call sites.
pub type OllamaCrossEncoder = HttpCrossEncoder;

fn rerank_timeout_ms() -> u64 {
    std::env::var("RERANK_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2500)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_cross_encoder_is_joint_over_query_and_doc() {
        let query = "What does should_skip_blob do?";
        let exact = lexical_cross_score(
            query,
            "github://zone/content/mod.rs",
            "mod.rs",
            "fn should_skip_blob(uri: &str) -> bool",
        );
        let other = lexical_cross_score(query, "github://zone/auth.ts", "auth.ts", "login form");
        assert!(exact > other);
        assert!(exact > 0.4);
    }

    #[test]
    fn min_max_norm_spans_unit_interval() {
        let norm = min_max_norm(&[1.0, 3.0, 5.0]);
        assert!((norm[0] - 0.0).abs() < 1e-5);
        assert!((norm[2] - 1.0).abs() < 1e-5);
    }
}
