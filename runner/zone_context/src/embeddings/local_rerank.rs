//! In-process ONNX cross-encoder (fastembed `TextRerank`).
//!
//! Ollama has no `/api/rerank`, and generate-yes/no on a Qwen3-Reranker GGUF
//! does not produce usable yes/no logprobs. This runs a real pair scorer on
//! the first-stage candidates.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

use super::providers::LocalEmbeddingProvider;
use super::rerank::CrossEncoder;
use crate::error::{ContextError, Result};

const DEFAULT_MAX_DOC_CHARS: usize = 1500;

pub struct LocalCrossEncoder {
    model: Arc<Mutex<TextRerank>>,
    model_name: String,
}

impl LocalCrossEncoder {
    pub fn model(&self) -> &str {
        &self.model_name
    }

    pub fn resolve_model(name: &str) -> RerankerModel {
        let base = name.split_once(':').map(|(b, _)| b).unwrap_or(name);
        match base {
            "jina-turbo" | "jinaai/jina-reranker-v1-turbo-en" => {
                RerankerModel::JINARerankerV1TurboEn
            }
            "bge-m3" | "rozgo/bge-reranker-v2-m3" | "BAAI/bge-reranker-v2-m3" => {
                RerankerModel::BGERerankerV2M3
            }
            "bge-base" | "BAAI/bge-reranker-base" => RerankerModel::BGERerankerBase,
            "jina-v2" | "jinaai/jina-reranker-v2-base-multilingual" => {
                RerankerModel::JINARerankerV2BaseMultiligual
            }
            other => other
                .parse::<RerankerModel>()
                .unwrap_or(RerankerModel::JINARerankerV1TurboEn),
        }
    }

    pub fn new(model_name: &str, cache_dir: PathBuf) -> Result<Self> {
        let model = Self::resolve_model(model_name);
        let options = RerankInitOptions::new(model.clone())
            .with_cache_dir(cache_dir)
            .with_max_length(512)
            .with_intra_threads(2)
            .with_show_download_progress(false);
        let reranker = TextRerank::try_new(options).map_err(|e| {
            ContextError::Config(format!("Failed to load local reranker {model:?}: {e}"))
        })?;
        Ok(Self {
            model: Arc::new(Mutex::new(reranker)),
            model_name: model.to_string(),
        })
    }

    /// Load off the runtime, then refuse the model if it cannot order a known pair.
    pub async fn probe() -> Option<Self> {
        let name = std::env::var("RERANK_MODEL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "jinaai/jina-reranker-v1-turbo-en".into());
        let cache = LocalEmbeddingProvider::default_cache_dir();
        let loaded = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            tokio::task::spawn_blocking({
                let name = name.clone();
                move || Self::new(&name, cache)
            }),
        )
        .await
        .ok()?
        .ok()?
        .ok()?;
        if !loaded.discriminates_known_pair().await {
            tracing::warn!(
                model = %loaded.model_name,
                "local cross-encoder failed the known-pair probe; leaving it disabled"
            );
            return None;
        }
        tracing::info!(model = %loaded.model_name, "using in-process ONNX cross-encoder");
        Some(loaded)
    }

    async fn discriminates_known_pair(&self) -> bool {
        let query = "What does should_skip_blob do when a GitHub file SHA is unchanged?";
        let docs = [
            "pub fn should_skip_blob(&self, uri: &str, blob_sha: &str) -> bool { self.known_blobs.get(uri) }",
            "generic authentication helper with a login form and password reset",
        ];
        match self.score_documents(query, &docs).await {
            Ok(scores) if scores.len() == 2 => scores[0] > scores[1] + 0.02,
            _ => false,
        }
    }
}

#[async_trait]
impl CrossEncoder for LocalCrossEncoder {
    async fn score_pair(&self, query: &str, document: &str) -> Result<f32> {
        let scores = self.score_documents(query, &[document]).await?;
        scores
            .into_iter()
            .next()
            .ok_or_else(|| ContextError::Embedding("empty local rerank".into()))
    }

    async fn score_documents(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let query = query.to_string();
        let docs: Vec<String> = documents
            .iter()
            .map(|doc| clip_doc(doc, DEFAULT_MAX_DOC_CHARS))
            .collect();
        if self.model.try_lock().is_err() {
            return Err(ContextError::Embedding("reranker busy".into()));
        }
        let handle = Arc::clone(&self.model);
        tokio::task::spawn_blocking(move || {
            let mut model = handle
                .try_lock()
                .map_err(|_| ContextError::Embedding("reranker busy".into()))?;
            let docs_ref: Vec<&str> = docs.iter().map(String::as_str).collect();
            let ranked = model
                .rerank(query.as_str(), docs_ref.as_slice(), false, Some(8))
                .map_err(|e| ContextError::Embedding(format!("local rerank: {e}")))?;
            let mut scores = vec![0.0f32; docs.len()];
            for hit in ranked {
                if let Some(slot) = scores.get_mut(hit.index) {
                    *slot = hit.score;
                }
            }
            Ok(scores)
        })
        .await
        .map_err(|e| ContextError::Embedding(format!("rerank task: {e}")))?
    }
}

fn clip_doc(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_reranker_aliases() {
        assert_eq!(
            LocalCrossEncoder::resolve_model("jina-turbo"),
            RerankerModel::JINARerankerV1TurboEn
        );
        assert_eq!(
            LocalCrossEncoder::resolve_model("BAAI/bge-reranker-base"),
            RerankerModel::BGERerankerBase
        );
    }
}
