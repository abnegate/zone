//! In-process embedding provider
//!
//! Runs the embedding model inside the server process via ONNX Runtime
//! (`fastembed`). No network hop, real batching, and no contention with the
//! chat model for Ollama's loaded-model slots. Intended for the small
//! embedding models (100M–350M params) that run comfortably on CPU.
//!
//! Model weights are downloaded from Hugging Face on first use and cached on
//! disk (see [`LocalEmbeddingProvider::default_cache_dir`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fastembed::{EmbeddingModel, QuantizationMode, TextEmbedding, TextInitOptions};

use crate::embeddings::EmbeddingService;
use crate::embeddings::providers::AiSettings;
use crate::error::{ContextError, Result};

/// Default number of texts per ONNX forward pass.
pub const DEFAULT_BATCH_SIZE: usize = 32;

/// Environment variable overriding the on-disk model cache directory.
pub const CACHE_DIR_ENV: &str = "ZONE_EMBED_CACHE_DIR";

/// In-process embedding provider backed by `fastembed`.
pub struct LocalEmbeddingProvider {
    // `TextEmbedding::embed` takes `&mut self`, so calls are serialised. ORT
    // already saturates the CPU with intra-op threads for a single batch, so
    // this costs little; concurrent callers queue rather than oversubscribe.
    model: Arc<Mutex<TextEmbedding>>,
    model_name: String,
    dimension: usize,
    max_tokens: usize,
    batch_size: usize,
}

impl std::fmt::Debug for LocalEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEmbeddingProvider")
            .field("model_name", &self.model_name)
            .field("dimension", &self.dimension)
            .field("max_tokens", &self.max_tokens)
            .field("batch_size", &self.batch_size)
            .finish_non_exhaustive()
    }
}

/// Map a model name (Ollama tag or Hugging Face id) to a fastembed model.
///
/// Ollama names are accepted so an existing `OLLAMA_MODEL_EMBED` value keeps
/// working when the engine is switched to `local`. The `:latest` suffix and
/// any other tag are ignored.
pub fn resolve_model(name: &str) -> Result<EmbeddingModel> {
    let base = name.split_once(':').map(|(b, _)| b).unwrap_or(name);
    let model = match base {
        "nomic-embed-text" | "nomic-embed-text-v1.5" => EmbeddingModel::NomicEmbedTextV15,
        "nomic-embed-text-v1" => EmbeddingModel::NomicEmbedTextV1,
        "mxbai-embed-large" => EmbeddingModel::MxbaiEmbedLargeV1,
        "snowflake-arctic-embed" | "snowflake-arctic-embed-l" => {
            EmbeddingModel::SnowflakeArcticEmbedL
        }
        "snowflake-arctic-embed-m" => EmbeddingModel::SnowflakeArcticEmbedM,
        "snowflake-arctic-embed-s" => EmbeddingModel::SnowflakeArcticEmbedS,
        "bge-small-en" | "bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
        "bge-base-en" | "bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
        "bge-large-en" | "bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
        "bge-m3" => EmbeddingModel::BGEM3,
        "all-minilm" | "all-minilm-l6-v2" => EmbeddingModel::AllMiniLML6V2,
        "all-minilm-l12-v2" => EmbeddingModel::AllMiniLML12V2,
        other => {
            // Fall back to fastembed's own identifiers: the Hugging Face repo
            // id (`nomic-ai/nomic-embed-text-v1.5`) or the enum variant name.
            // Quantised variants share the repo id; prefer the full-precision one.
            let candidates: Vec<EmbeddingModel> = TextEmbedding::list_supported_models()
                .into_iter()
                .filter(|info| info.model_code == other)
                .map(|info| info.model)
                .collect();
            candidates
                .iter()
                .find(|m| TextEmbedding::get_quantization_mode(m) == QuantizationMode::None)
                .or_else(|| candidates.first())
                .cloned()
                .or_else(|| other.parse::<EmbeddingModel>().ok())
                .ok_or_else(|| {
                    ContextError::InvalidSourceConfig(format!(
                        "Unknown embedding model '{}' for local engine",
                        name
                    ))
                })?
        }
    };
    Ok(model)
}

/// Sequence length the tokenizer truncates to for a given model.
///
/// Kept in line with what Ollama runs these models at so switching engines
/// does not silently change how long documents are chunked.
fn model_max_tokens(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::NomicEmbedTextV1
        | EmbeddingModel::NomicEmbedTextV15
        | EmbeddingModel::NomicEmbedTextV15Q => 2048,
        EmbeddingModel::AllMiniLML6V2
        | EmbeddingModel::AllMiniLML6V2Q
        | EmbeddingModel::AllMiniLML12V2
        | EmbeddingModel::AllMiniLML12V2Q => 256,
        EmbeddingModel::BGEM3 => 8192,
        _ => 512,
    }
}

impl LocalEmbeddingProvider {
    /// Default location for downloaded model weights.
    ///
    /// `$ZONE_EMBED_CACHE_DIR` if set, else `<os cache dir>/zone/fastembed`,
    /// else `.fastembed_cache` in the working directory.
    pub fn default_cache_dir() -> PathBuf {
        if let Ok(dir) = std::env::var(CACHE_DIR_ENV)
            && !dir.trim().is_empty()
        {
            return PathBuf::from(dir);
        }
        dirs::cache_dir()
            .map(|d| d.join("zone").join("fastembed"))
            .unwrap_or_else(|| PathBuf::from(".fastembed_cache"))
    }

    /// Load (downloading on first use) the given model.
    ///
    /// This is blocking and can take seconds on a cold cache; call it from a
    /// blocking context or via [`Self::load`].
    pub fn new(model_name: &str, cache_dir: &Path, batch_size: usize) -> Result<Self> {
        let model = resolve_model(model_name)?;
        let info = TextEmbedding::get_model_info(&model)
            .map_err(|e| ContextError::Config(format!("No model info for {:?}: {}", model, e)))?;
        let dimension = info.dim;
        let max_tokens = model_max_tokens(&model);

        let options = TextInitOptions::new(model.clone())
            .with_cache_dir(cache_dir.to_path_buf())
            .with_max_length(max_tokens)
            .with_show_download_progress(false);

        let embedder = TextEmbedding::try_new(options).map_err(|e| {
            ContextError::Config(format!(
                "Failed to load local embedding model {:?}: {}",
                model, e
            ))
        })?;

        tracing::info!(
            model = %model,
            dimension,
            max_tokens,
            cache_dir = %cache_dir.display(),
            "Loaded in-process embedding model"
        );

        Ok(Self {
            model: Arc::new(Mutex::new(embedder)),
            model_name: model_name.to_string(),
            dimension,
            max_tokens,
            batch_size: batch_size.max(1),
        })
    }

    /// Async wrapper around [`Self::new`] that runs the load off the runtime.
    pub async fn load(model_name: &str, cache_dir: PathBuf, batch_size: usize) -> Result<Self> {
        let name = model_name.to_string();
        tokio::task::spawn_blocking(move || Self::new(&name, &cache_dir, batch_size))
            .await
            .map_err(|e| ContextError::Config(format!("Embedding model load task failed: {}", e)))?
    }

    /// Create from AI settings using the default cache dir and batch size.
    pub fn from_settings(settings: &AiSettings) -> Result<Self> {
        let model = settings
            .model_embedding
            .as_deref()
            .unwrap_or("nomic-embed-text");
        Self::new(model, &Self::default_cache_dir(), DEFAULT_BATCH_SIZE)
    }
}

/// Run the model under its lock and validate output shape.
///
/// Each ONNX batch is padded to its longest sequence, so a batch mixing a
/// 15-token chat message with a 250-token document chunk pays for 32×250
/// tokens of attention. Sorting by length first makes batches homogeneous
/// (measured 3× on a mixed corpus) and the original order is restored after.
fn embed_locked(
    model: &Mutex<TextEmbedding>,
    texts: &[String],
    batch_size: usize,
    dimension: usize,
) -> Result<Vec<Vec<f32>>> {
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len());
    let sorted: Vec<&str> = order.iter().map(|&i| texts[i].as_str()).collect();

    let out = {
        let mut guard = model
            .lock()
            .map_err(|_| ContextError::Embedding("Embedding model mutex poisoned".into()))?;
        guard
            .embed(&sorted, Some(batch_size))
            .map_err(|e| ContextError::Embedding(format!("Local inference failed: {}", e)))?
    };

    if out.len() != texts.len() {
        return Err(ContextError::Embedding(format!(
            "Model returned {} embeddings for {} texts",
            out.len(),
            texts.len()
        )));
    }

    let mut restored: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
    for (sorted_pos, embedding) in out.into_iter().enumerate() {
        if embedding.len() != dimension {
            return Err(ContextError::EmbeddingDimensionMismatch {
                expected: dimension,
                actual: embedding.len(),
            });
        }
        restored[order[sorted_pos]] = embedding;
    }
    Ok(restored)
}

#[async_trait]
impl EmbeddingService for LocalEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut out = self.embed_batch(&[text]).await?;
        out.pop()
            .ok_or_else(|| ContextError::Embedding("Model returned no embedding".into()))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();

        // The trait hands us `&self`, not `Arc<Self>`; clone the inner handle
        // so the blocking task owns everything it needs.
        let model = Arc::clone(&self.model);
        let dimension = self.dimension;
        let batch_size = self.batch_size;

        tokio::task::spawn_blocking(move || embed_locked(&model, &owned, batch_size, dimension))
            .await
            .map_err(|e| ContextError::Embedding(format!("Embedding task failed: {}", e)))?
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model(&self) -> &str {
        &self.model_name
    }

    fn max_tokens(&self) -> usize {
        self.max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_ollama_names() {
        assert_eq!(
            resolve_model("nomic-embed-text").unwrap(),
            EmbeddingModel::NomicEmbedTextV15
        );
        assert_eq!(
            resolve_model("nomic-embed-text:latest").unwrap(),
            EmbeddingModel::NomicEmbedTextV15
        );
        assert_eq!(
            resolve_model("mxbai-embed-large").unwrap(),
            EmbeddingModel::MxbaiEmbedLargeV1
        );
        assert_eq!(
            resolve_model("bge-small-en-v1.5").unwrap(),
            EmbeddingModel::BGESmallENV15
        );
        assert_eq!(
            resolve_model("all-minilm").unwrap(),
            EmbeddingModel::AllMiniLML6V2
        );
    }

    #[test]
    fn resolves_huggingface_ids() {
        assert_eq!(
            resolve_model("nomic-ai/nomic-embed-text-v1.5").unwrap(),
            EmbeddingModel::NomicEmbedTextV15
        );
    }

    #[test]
    fn rejects_unknown_model() {
        let err = resolve_model("definitely-not-a-model").unwrap_err();
        assert!(matches!(err, ContextError::InvalidSourceConfig(_)));
    }

    #[test]
    fn dimensions_match_ollama_table() {
        // Must agree with `ollama::get_model_dimension` so pgvector columns
        // created under one engine keep working under the other.
        let cases = [
            ("nomic-embed-text", 768),
            ("mxbai-embed-large", 1024),
            ("snowflake-arctic-embed", 1024),
            ("bge-small-en-v1.5", 384),
            ("all-minilm", 384),
        ];
        for (name, dim) in cases {
            let model = resolve_model(name).unwrap();
            let info = TextEmbedding::get_model_info(&model).unwrap();
            assert_eq!(info.dim, dim, "{name}");
        }
    }

    #[test]
    fn cache_dir_env_override() {
        // Serialise env mutation with other tests in this module.
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: test-only, guarded by ENV_LOCK.
        unsafe { std::env::set_var(CACHE_DIR_ENV, "/tmp/zone-embed-test") };
        assert_eq!(
            LocalEmbeddingProvider::default_cache_dir(),
            PathBuf::from("/tmp/zone-embed-test")
        );
        unsafe { std::env::remove_var(CACHE_DIR_ENV) };
        assert_ne!(
            LocalEmbeddingProvider::default_cache_dir(),
            PathBuf::from("/tmp/zone-embed-test")
        );
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());
}
