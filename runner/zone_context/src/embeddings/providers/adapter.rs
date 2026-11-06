use std::sync::Arc;

use abnegate_llm::EmbeddingProvider;
use async_trait::async_trait;
use futures::stream::{self, StreamExt, TryStreamExt};

use super::batching::Batching;
use super::failure::context_error;
use crate::content::{embed_char_budget, truncate_chars};
use crate::embeddings::{EmbeddingService, VECTOR_DIMENSION};
use crate::error::{ContextError, Result};

const LONG_TEXT_CHARACTERS: usize = 2000;
const SHORT_TEXT_ATTEMPTS: usize = 2;
const LONG_TEXT_ATTEMPTS: usize = 3;
const HALVING_DIVISOR: usize = 2;
const UNKNOWN_DIMENSIONS: usize = 0;
const INPUTS_PER_REQUEST: usize = 64;

/// Zone's [`EmbeddingService`] over any `abnegate-llm` embedding provider.
///
/// The provider does the transport; the adapter keeps what Zone's index
/// depends on: the character budget and the halving retry on a
/// context-length refusal, each vector's width against the provider's and
/// against [`VECTOR_DIMENSION`], and failures worded so
/// [`ContextError::is_context_length`] and
/// [`ContextError::is_fatal_embedding`] classify them.
pub struct EmbeddingAdapter {
    provider: Arc<dyn EmbeddingProvider>,
    batching: Batching,
}

impl EmbeddingAdapter {
    pub fn new(provider: Arc<dyn EmbeddingProvider>, batching: Batching) -> Self {
        Self { provider, batching }
    }

    fn character_budget(&self) -> usize {
        embed_char_budget(self.max_tokens())
    }

    fn checked(&self, vector: Vec<f32>) -> Result<Vec<f32>> {
        let expected = self.dimension();
        if expected != UNKNOWN_DIMENSIONS && vector.len() != expected {
            return Err(ContextError::EmbeddingDimensionMismatch {
                expected,
                actual: vector.len(),
            });
        }
        if vector.len() > VECTOR_DIMENSION {
            return Err(ContextError::EmbeddingDimensionMismatch {
                expected: VECTOR_DIMENSION,
                actual: vector.len(),
            });
        }
        Ok(vector)
    }

    fn all_checked(&self, vectors: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>> {
        vectors
            .into_iter()
            .map(|vector| self.checked(vector))
            .collect()
    }

    async fn embed_prompt(&self, prompt: &str) -> Result<Vec<f32>> {
        let vector = self
            .provider
            .embed_single(prompt)
            .await
            .map_err(context_error)?;
        self.checked(vector)
    }

    async fn embed_within_context(&self, text: &str) -> Result<Vec<f32>> {
        let mut prompt = truncate_chars(text, self.character_budget());
        let attempts = if text.chars().count() < LONG_TEXT_CHARACTERS {
            SHORT_TEXT_ATTEMPTS
        } else {
            LONG_TEXT_ATTEMPTS
        };
        let mut refusal = None;
        for attempt in 0..attempts {
            match self.embed_prompt(&prompt).await {
                Ok(vector) => return Ok(vector),
                Err(error) if error.is_context_length() => {
                    refusal = Some(error);
                    prompt = truncate_chars(&prompt, prompt.chars().count() / HALVING_DIVISOR);
                    if prompt.is_empty() {
                        break;
                    }
                    tracing::warn!(
                        model = %self.model(),
                        attempt,
                        chars = prompt.chars().count(),
                        "retrying embedding after context-length error"
                    );
                }
                Err(error) => return Err(error),
            }
        }
        Err(refusal.unwrap_or_else(|| {
            ContextError::Embedding("input exceeds embedding context length".to_string())
        }))
    }

    async fn embed_indexed(&self, index: usize, text: &str) -> Result<(usize, Vec<f32>)> {
        Ok((index, self.embed_within_context(text).await?))
    }

    async fn embed_each(&self, texts: &[&str], concurrency: usize) -> Result<Vec<Vec<f32>>> {
        let mut vectors = vec![Vec::new(); texts.len()];
        let mut answers = stream::iter(0..texts.len())
            .map(|index| self.embed_indexed(index, texts[index]))
            .buffer_unordered(concurrency.max(1));
        while let Some((index, vector)) = answers.try_next().await? {
            vectors[index] = vector;
        }
        Ok(vectors)
    }

    async fn embed_together(&self, texts: &[&str], concurrency: usize) -> Result<Vec<Vec<f32>>> {
        let requests: Vec<Vec<Vec<f32>>> =
            stream::iter((0..texts.len()).step_by(INPUTS_PER_REQUEST))
                .map(|start| {
                    let end = texts.len().min(start + INPUTS_PER_REQUEST);
                    self.embed_request(&texts[start..end], concurrency)
                })
                .buffered(concurrency.max(1))
                .try_collect()
                .await?;
        Ok(requests.into_iter().flatten().collect())
    }

    async fn embed_request(&self, texts: &[&str], concurrency: usize) -> Result<Vec<Vec<f32>>> {
        let budget = self.character_budget();
        let prompts: Vec<String> = texts
            .iter()
            .map(|text| truncate_chars(text, budget))
            .collect();
        match self.provider.embed(&prompts).await.map_err(context_error) {
            Ok(vectors) => self.all_checked(vectors),
            Err(error) if error.is_context_length() => {
                tracing::warn!(
                    model = %self.model(),
                    texts = texts.len(),
                    "embedding the request one text at a time after context-length error"
                );
                self.embed_each(texts, concurrency).await
            }
            Err(error) => Err(error),
        }
    }

    async fn embed_locally(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let owned: Vec<String> = texts.iter().map(|text| text.to_string()).collect();
        let vectors = self.provider.embed(&owned).await.map_err(context_error)?;
        self.all_checked(vectors)
    }
}

#[async_trait]
impl EmbeddingService for EmbeddingAdapter {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match self.batching {
            Batching::Local => self.embed_prompt(text).await,
            Batching::PerText { .. } | Batching::Batched { .. } => {
                self.embed_within_context(text).await
            }
        }
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        match self.batching {
            Batching::PerText { concurrency } => self.embed_each(texts, concurrency).await,
            Batching::Batched { concurrency } => self.embed_together(texts, concurrency).await,
            Batching::Local => self.embed_locally(texts).await,
        }
    }

    fn dimension(&self) -> usize {
        usize::try_from(self.provider.dimensions()).unwrap_or(usize::MAX)
    }

    fn model(&self) -> &str {
        self.provider.model()
    }

    fn max_tokens(&self) -> usize {
        usize::try_from(self.provider.maximum_input_tokens()).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use abnegate_llm::ProviderError;

    use super::*;

    const MODEL: &str = "stub-embed";
    const DIMENSIONS: u32 = 4;
    const MAXIMUM_INPUT_TOKENS: u32 = 1024;
    const CONTEXT_LENGTH_MESSAGE: &str = "the input length exceeds the context length";
    const CONTEXT_LIMIT_CHARACTERS: usize = 1000;
    const LONG_TEXT: usize = 5000;
    const CONCURRENCY: usize = 3;
    const INTERNAL_SERVER_ERROR: u16 = 500;
    const UNAUTHORIZED: u16 = 401;

    #[derive(Default)]
    struct Calls {
        batches: Vec<Vec<String>>,
        singles: Vec<String>,
    }

    struct Stub {
        dimensions: AtomicU32,
        learns_dimensions: bool,
        width: usize,
        maximum_input_tokens: u32,
        context_limit: Option<usize>,
        refusal: Option<u16>,
        calls: Mutex<Calls>,
    }

    impl Stub {
        fn new() -> Self {
            Self {
                dimensions: AtomicU32::new(DIMENSIONS),
                learns_dimensions: false,
                width: DIMENSIONS as usize,
                maximum_input_tokens: MAXIMUM_INPUT_TOKENS,
                context_limit: None,
                refusal: None,
                calls: Mutex::default(),
            }
        }

        fn vector(&self, text: &str) -> Vec<f32> {
            (0..self.width)
                .map(|index| (index + text.chars().count()) as f32)
                .collect()
        }

        fn answer(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
            if let Some(status) = self.refusal {
                return Err(ProviderError::api(status, "refused"));
            }
            if self
                .context_limit
                .is_some_and(|limit| texts.iter().any(|text| text.chars().count() > limit))
            {
                return Err(ProviderError::api(
                    INTERNAL_SERVER_ERROR,
                    CONTEXT_LENGTH_MESSAGE,
                ));
            }
            if self.learns_dimensions {
                let width = u32::try_from(self.width).unwrap();
                let _ = self.dimensions.compare_exchange(
                    0,
                    width,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
            }
            Ok(texts.iter().map(|text| self.vector(text)).collect())
        }

        fn calls(&self) -> std::sync::MutexGuard<'_, Calls> {
            self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl EmbeddingProvider for Stub {
        fn name(&self) -> &str {
            "stub"
        }

        fn dimensions(&self) -> u32 {
            self.dimensions.load(Ordering::Relaxed)
        }

        fn model(&self) -> &str {
            MODEL
        }

        fn maximum_input_tokens(&self) -> u32 {
            self.maximum_input_tokens
        }

        async fn embed(
            &self,
            texts: &[String],
        ) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
            self.calls().batches.push(texts.to_vec());
            self.answer(texts)
        }

        async fn embed_single(&self, text: &str) -> std::result::Result<Vec<f32>, ProviderError> {
            self.calls().singles.push(text.to_string());
            self.answer(&[text.to_string()])
                .map(|mut vectors| vectors.remove(0))
        }
    }

    fn adapter(stub: Stub, batching: Batching) -> (EmbeddingAdapter, Arc<Stub>) {
        let stub = Arc::new(stub);
        let provider: Arc<dyn EmbeddingProvider> = stub.clone();
        (EmbeddingAdapter::new(provider, batching), stub)
    }

    fn per_text() -> Batching {
        Batching::PerText {
            concurrency: CONCURRENCY,
        }
    }

    fn batched() -> Batching {
        Batching::Batched {
            concurrency: CONCURRENCY,
        }
    }

    fn long_text() -> String {
        "a".repeat(LONG_TEXT)
    }

    #[test]
    fn the_character_budget_is_the_providers_input_budget() {
        for tokens in [1, 256, 512, 1024, 2048, 8192, u32::MAX] {
            let (adapter, stub) = adapter(
                Stub {
                    maximum_input_tokens: tokens,
                    ..Stub::new()
                },
                per_text(),
            );
            assert_eq!(
                adapter.character_budget(),
                stub.maximum_input_characters(),
                "{tokens}"
            );
        }
    }

    #[test]
    fn the_adapter_reports_the_providers_model_width_and_token_limit() {
        let (adapter, _) = adapter(Stub::new(), per_text());
        assert_eq!(adapter.model(), MODEL);
        assert_eq!(adapter.dimension(), DIMENSIONS as usize);
        assert_eq!(adapter.max_tokens(), MAXIMUM_INPUT_TOKENS as usize);
    }

    #[tokio::test]
    async fn per_text_batches_keep_their_order_one_request_each() {
        let (adapter, stub) = adapter(Stub::new(), per_text());
        let texts = ["a", "bbb", "cc", "dddd", "e"];
        let vectors = adapter.embed_batch(&texts).await.unwrap();
        let expected: Vec<Vec<f32>> = texts.iter().map(|text| stub.vector(text)).collect();
        assert_eq!(vectors, expected);
        assert_eq!(stub.calls().singles.len(), texts.len());
        assert!(stub.calls().batches.is_empty());
    }

    #[tokio::test]
    async fn an_empty_batch_sends_nothing() {
        for batching in [per_text(), batched(), Batching::Local] {
            let (adapter, stub) = adapter(Stub::new(), batching);
            assert!(adapter.embed_batch(&[]).await.unwrap().is_empty());
            assert!(stub.calls().batches.is_empty());
            assert!(stub.calls().singles.is_empty());
        }
    }

    #[tokio::test]
    async fn a_long_text_is_cut_to_the_character_budget() {
        let (adapter, stub) = adapter(Stub::new(), per_text());
        adapter.embed(&long_text()).await.unwrap();
        assert_eq!(
            stub.calls().singles[0].chars().count(),
            embed_char_budget(MAXIMUM_INPUT_TOKENS as usize)
        );
    }

    #[tokio::test]
    async fn a_context_length_refusal_halves_the_prompt_until_it_fits() {
        let (adapter, stub) = adapter(
            Stub {
                maximum_input_tokens: 8192,
                context_limit: Some(CONTEXT_LIMIT_CHARACTERS),
                ..Stub::new()
            },
            per_text(),
        );
        adapter.embed(&long_text()).await.unwrap();
        let lengths: Vec<usize> = stub
            .calls()
            .singles
            .iter()
            .map(|prompt| prompt.chars().count())
            .collect();
        assert_eq!(lengths, [3000, 1500, 750]);
    }

    #[tokio::test]
    async fn a_short_text_gets_two_attempts_and_keeps_the_refusal() {
        let (adapter, stub) = adapter(
            Stub {
                context_limit: Some(0),
                ..Stub::new()
            },
            per_text(),
        );
        let error = adapter.embed("hello").await.unwrap_err();
        assert!(error.is_context_length(), "{error:?}");
        let lengths: Vec<usize> = stub
            .calls()
            .singles
            .iter()
            .map(|prompt| prompt.chars().count())
            .collect();
        assert_eq!(lengths, [5, 2]);
    }

    #[tokio::test]
    async fn a_prompt_halved_to_nothing_stops_retrying() {
        let (adapter, stub) = adapter(
            Stub {
                context_limit: Some(0),
                ..Stub::new()
            },
            per_text(),
        );
        let error = adapter.embed("h").await.unwrap_err();
        assert!(error.is_context_length(), "{error:?}");
        assert_eq!(stub.calls().singles.len(), 1);
    }

    #[tokio::test]
    async fn any_other_refusal_is_not_retried() {
        let (adapter, stub) = adapter(
            Stub {
                refusal: Some(UNAUTHORIZED),
                ..Stub::new()
            },
            per_text(),
        );
        let error = adapter.embed("hello").await.unwrap_err();
        assert!(error.is_fatal_embedding(), "{error:?}");
        assert_eq!(stub.calls().singles.len(), 1);
    }

    #[tokio::test]
    async fn the_first_failure_in_a_per_text_batch_fails_the_batch() {
        let (adapter, _) = adapter(
            Stub {
                refusal: Some(UNAUTHORIZED),
                ..Stub::new()
            },
            per_text(),
        );
        let error = adapter.embed_batch(&["a", "b"]).await.unwrap_err();
        assert!(matches!(error, ContextError::Auth(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_vector_of_another_width_is_a_dimension_mismatch() {
        let (adapter, _) = adapter(
            Stub {
                width: 3,
                ..Stub::new()
            },
            per_text(),
        );
        let error = adapter.embed("hello").await.unwrap_err();
        assert!(matches!(
            error,
            ContextError::EmbeddingDimensionMismatch {
                expected: 4,
                actual: 3
            }
        ));
    }

    #[tokio::test]
    async fn a_vector_wider_than_the_store_is_a_dimension_mismatch() {
        let width = VECTOR_DIMENSION + 512;
        let (adapter, _) = adapter(
            Stub {
                dimensions: AtomicU32::new(0),
                learns_dimensions: true,
                width,
                ..Stub::new()
            },
            batched(),
        );
        let error = adapter.embed_batch(&["hello"]).await.unwrap_err();
        assert!(
            matches!(
                error,
                ContextError::EmbeddingDimensionMismatch { expected, actual }
                    if expected == VECTOR_DIMENSION && actual == width
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_learned_width_is_checked_from_the_next_vector_on() {
        let (adapter, stub) = adapter(
            Stub {
                dimensions: AtomicU32::new(0),
                learns_dimensions: true,
                ..Stub::new()
            },
            batched(),
        );
        assert_eq!(adapter.dimension(), 0);
        adapter.embed_batch(&["hello"]).await.unwrap();
        assert_eq!(adapter.dimension(), DIMENSIONS as usize);
        assert_eq!(stub.calls().batches.len(), 1);
    }

    #[tokio::test]
    async fn a_small_batch_is_one_request_cut_to_the_budget() {
        let (adapter, stub) = adapter(Stub::new(), batched());
        let long = long_text();
        let vectors = adapter.embed_batch(&["a", &long]).await.unwrap();
        assert_eq!(vectors.len(), 2);
        let calls = stub.calls();
        assert_eq!(calls.batches.len(), 1);
        assert_eq!(
            calls.batches[0][1].chars().count(),
            embed_char_budget(MAXIMUM_INPUT_TOKENS as usize)
        );
        assert!(calls.singles.is_empty());
    }

    #[tokio::test]
    async fn a_batch_past_the_context_falls_back_to_one_text_at_a_time() {
        let (adapter, stub) = adapter(
            Stub {
                maximum_input_tokens: 8192,
                context_limit: Some(CONTEXT_LIMIT_CHARACTERS),
                ..Stub::new()
            },
            batched(),
        );
        let long = long_text();
        let vectors = adapter.embed_batch(&["short", &long]).await.unwrap();
        assert_eq!(vectors[0], stub.vector("short"));
        assert_eq!(vectors[1], stub.vector(&"a".repeat(750)));
        let calls = stub.calls();
        assert_eq!(calls.batches.len(), 1);
        let mut lengths: Vec<usize> = calls
            .singles
            .iter()
            .map(|prompt| prompt.chars().count())
            .collect();
        lengths.sort_unstable();
        assert_eq!(lengths, [5, 750, 1500, 3000]);
    }

    #[tokio::test]
    async fn a_large_batch_is_split_into_requests_and_keeps_its_order() {
        let (adapter, stub) = adapter(Stub::new(), batched());
        let owned: Vec<String> = (1..=INPUTS_PER_REQUEST * 2 + 5)
            .map(|length| "a".repeat(length))
            .collect();
        let texts: Vec<&str> = owned.iter().map(String::as_str).collect();
        let vectors = adapter.embed_batch(&texts).await.unwrap();
        let expected: Vec<Vec<f32>> = texts.iter().map(|text| stub.vector(text)).collect();
        assert_eq!(vectors, expected);
        let calls = stub.calls();
        let mut sizes: Vec<usize> = calls.batches.iter().map(Vec::len).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [5, INPUTS_PER_REQUEST, INPUTS_PER_REQUEST]);
        assert!(calls.singles.is_empty());
    }

    #[tokio::test]
    async fn a_context_length_refusal_falls_back_only_for_its_request() {
        let (adapter, stub) = adapter(
            Stub {
                maximum_input_tokens: 8192,
                context_limit: Some(CONTEXT_LIMIT_CHARACTERS),
                ..Stub::new()
            },
            batched(),
        );
        let long = long_text();
        let mut texts = vec!["short"; INPUTS_PER_REQUEST + 3];
        texts[INPUTS_PER_REQUEST + 1] = &long;
        let vectors = adapter.embed_batch(&texts).await.unwrap();
        assert_eq!(vectors.len(), texts.len());
        for (index, vector) in vectors.iter().enumerate() {
            let expected = if index == INPUTS_PER_REQUEST + 1 {
                stub.vector(&"a".repeat(750))
            } else {
                stub.vector("short")
            };
            assert_eq!(vector, &expected, "{index}");
        }
        let calls = stub.calls();
        assert_eq!(calls.batches.len(), 2);
        let mut lengths: Vec<usize> = calls
            .singles
            .iter()
            .map(|prompt| prompt.chars().count())
            .collect();
        lengths.sort_unstable();
        assert_eq!(lengths, [5, 5, 750, 1500, 3000]);
    }

    #[tokio::test]
    async fn a_local_model_gets_every_text_whole_in_one_call() {
        let (adapter, stub) = adapter(Stub::new(), Batching::Local);
        let long = long_text();
        adapter.embed_batch(&["a", &long]).await.unwrap();
        adapter.embed(&long).await.unwrap();
        let calls = stub.calls();
        assert_eq!(calls.batches, [vec!["a".to_string(), long.clone()]]);
        assert_eq!(calls.singles, [long]);
    }
}
