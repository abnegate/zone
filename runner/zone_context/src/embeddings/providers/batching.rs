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
    /// The batch in requests of up to 64 texts, `concurrency` requests in
    /// flight, each text cut to the character budget. A context-length
    /// refusal does not say which text caused it, so that request's texts are
    /// then embedded as [`Batching::PerText`] at `concurrency`.
    Batched {
        /// Requests in flight at once, and texts in flight at once within a
        /// request refused for its context length.
        concurrency: usize,
    },
    /// The whole batch in one call to an in-process model, whose tokenizer
    /// truncates each text to the model's sequence length.
    Local,
}
