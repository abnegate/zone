/// How an [`EmbeddingAdapter`](super::EmbeddingAdapter) hands texts to its
/// provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Batching {
    /// One request per text, up to `concurrency` in flight. Each text is cut
    /// to the character budget first and halved again whenever the model
    /// refuses it for its context length.
    PerText {
        /// Texts in flight at once.
        concurrency: usize,
    },
    /// The whole batch in one request, each text cut to the character budget.
    /// A context-length refusal does not say which text caused it, so the
    /// batch is then embedded as [`Batching::PerText`] at `concurrency`.
    Batched {
        /// Texts in flight at once after a context-length refusal.
        concurrency: usize,
    },
    /// The whole batch in one call to an in-process model, whose tokenizer
    /// truncates each text to the model's sequence length.
    Local,
}
