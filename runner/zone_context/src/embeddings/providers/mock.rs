//! Mock embedding service for testing

use crate::embeddings::EmbeddingService;
use crate::error::Result;
use async_trait::async_trait;

/// Mock embedding service that returns random vectors
pub struct MockEmbeddingService {
    dimension: usize,
    model: String,
}

impl MockEmbeddingService {
    /// Create a new mock embedding service with specified dimension
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            model: "mock-embedder".to_string(),
        }
    }

    /// Create a deterministic embedding based on text hash
    fn generate_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();

        // Generate deterministic pseudo-random values based on hash
        let mut result = Vec::with_capacity(self.dimension);
        let mut seed = hash;
        for _ in 0..self.dimension {
            // Simple LCG for pseudo-random generation
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let value = (seed as f32 / u64::MAX as f32) * 2.0 - 1.0; // Range: [-1, 1]
            result.push(value);
        }

        // Normalize to unit vector
        let magnitude: f32 = result.iter().map(|&x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for val in &mut result {
                *val /= magnitude;
            }
        }

        result
    }
}

#[async_trait]
impl EmbeddingService for MockEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.generate_embedding(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| self.generate_embedding(text))
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn max_tokens(&self) -> usize {
        8192
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_embedder_dimension() {
        let embedder = MockEmbeddingService::new(384);
        assert_eq!(embedder.dimension(), 384);
    }

    #[tokio::test]
    async fn test_mock_embedder_embed() {
        let embedder = MockEmbeddingService::new(128);
        let embedding = embedder.embed("test text").await.unwrap();
        assert_eq!(embedding.len(), 128);
    }

    #[tokio::test]
    async fn test_mock_embedder_deterministic() {
        let embedder = MockEmbeddingService::new(64);
        let embedding1 = embedder.embed("same text").await.unwrap();
        let embedding2 = embedder.embed("same text").await.unwrap();
        assert_eq!(embedding1, embedding2);
    }

    #[tokio::test]
    async fn test_mock_embedder_different_texts() {
        let embedder = MockEmbeddingService::new(64);
        let embedding1 = embedder.embed("text one").await.unwrap();
        let embedding2 = embedder.embed("text two").await.unwrap();
        assert_ne!(embedding1, embedding2);
    }

    #[tokio::test]
    async fn test_mock_embedder_batch() {
        let embedder = MockEmbeddingService::new(128);
        let texts = vec!["first", "second", "third"];
        let embeddings = embedder.embed_batch(&texts).await.unwrap();
        assert_eq!(embeddings.len(), 3);
        assert_eq!(embeddings[0].len(), 128);
        assert_eq!(embeddings[1].len(), 128);
        assert_eq!(embeddings[2].len(), 128);
    }

    #[tokio::test]
    async fn test_mock_embedder_normalized() {
        let embedder = MockEmbeddingService::new(128);
        let embedding = embedder.embed("test").await.unwrap();

        // Check that the vector is normalized (magnitude = 1.0)
        let magnitude: f32 = embedding.iter().map(|&x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.001);
    }
}
