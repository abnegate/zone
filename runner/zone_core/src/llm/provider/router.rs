//! One handle in front of many providers.

use std::sync::Arc;

use async_trait::async_trait;

use super::completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
use super::error::ProviderError;
use super::selection::{SelectionStrategy, Weighted};

/// Routes a completion to one of several providers.
///
/// A [`Router`] is itself a [`CompletionProvider`], so a consumer holds one
/// handle and never learns whether it is talking to a single model, an A/B
/// split, or a chain three deep. Routers compose for the same reason: a
/// weighted split between two chains is a router of routers.
#[derive(Debug, Clone)]
pub struct Router {
    providers: Vec<Weighted>,
    strategy: SelectionStrategy,
    name: String,
}

impl Router {
    pub fn new(providers: Vec<Weighted>, strategy: SelectionStrategy) -> Self {
        Self {
            providers,
            strategy,
            name: "router".to_string(),
        }
    }

    /// A single provider, used for every request.
    pub fn primary(provider: Arc<dyn CompletionProvider>) -> Self {
        Self::new(vec![Weighted::spare(provider)], SelectionStrategy::Primary)
    }

    /// Providers tried in order until one answers.
    pub fn fallback(providers: Vec<Arc<dyn CompletionProvider>>) -> Self {
        Self::new(
            providers.into_iter().map(Weighted::spare).collect(),
            SelectionStrategy::Fallback,
        )
    }

    /// An A/B split, with the arms not drawn standing by as backups.
    pub fn weighted(providers: Vec<Weighted>) -> Self {
        Self::new(providers, SelectionStrategy::WeightedFallback)
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn strategy(&self) -> SelectionStrategy {
        self.strategy
    }

    pub fn providers(&self) -> &[Weighted] {
        &self.providers
    }

    /// Run a request against a caller-supplied split sample.
    ///
    /// [`CompletionProvider::complete`] draws the sample from entropy. Passing
    /// it in keeps the routing decision reproducible for a test, and lets a
    /// caller pin an experiment bucket to something stable such as a task id.
    pub async fn complete_with_sample(
        &self,
        request: CompletionRequest<'_>,
        sample: f64,
    ) -> Result<Completion, ProviderError> {
        if self.providers.is_empty() {
            return Err(ProviderError::Unconfigured);
        }

        let start = self
            .strategy
            .start(&self.providers, sample)
            .min(self.providers.len() - 1);

        let mut attempted = 0;
        let mut last: Option<ProviderError> = None;

        for index in self.order(start) {
            let provider = &self.providers[index].provider;
            attempted += 1;

            match provider.complete(request).await {
                Ok(completion) => return Ok(completion),
                Err(error) => {
                    // A request the provider refused on its merits will be
                    // refused identically by the next one, and retrying it
                    // only spends another provider's budget.
                    if !error.recoverable() {
                        return Err(error);
                    }
                    tracing::warn!(
                        router = %self.name,
                        provider = provider.name(),
                        error = %error,
                        "provider failed, trying the next"
                    );
                    last = Some(error);
                }
            }
        }

        Err(match last {
            // Reporting the last provider's own words is the whole point: the
            // task worker decides whether to retry by reading them.
            Some(last) => ProviderError::Exhausted {
                attempted,
                last: Box::new(last),
            },
            None => ProviderError::Unconfigured,
        })
    }

    /// The provider indices to try, starting where the strategy pointed.
    fn order(&self, start: usize) -> Vec<usize> {
        if !self.strategy.chains() {
            return vec![start];
        }

        let mut order = Vec::with_capacity(self.providers.len());
        order.push(start);
        order.extend((0..self.providers.len()).filter(|index| *index != start));
        order
    }
}

#[async_trait]
impl CompletionProvider for Router {
    fn name(&self) -> &str {
        &self.name
    }

    /// A router reports the kind of the provider it would start with, so a
    /// caller asking "is this a CLI agent?" gets the answer for the arm most
    /// likely to serve it rather than for the router itself.
    fn kind(&self) -> ProviderKind {
        self.providers
            .first()
            .map_or(ProviderKind::Http, |weighted| weighted.provider.kind())
    }

    async fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        self.complete_with_sample(request, rand::random_range(0.0..1.0))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::testing::StubProvider;
    use crate::llm::{Message, RequestOptions};

    fn request(messages: &[Message]) -> CompletionRequest<'_> {
        CompletionRequest {
            model: "test-model",
            messages,
            tools: None,
            options: RequestOptions { reserved: 512 },
        }
    }

    async fn answer(router: &Router, sample: f64) -> Result<Completion, ProviderError> {
        let messages = [Message::user("hello")];
        router
            .complete_with_sample(request(&messages), sample)
            .await
    }

    #[tokio::test]
    async fn a_primary_router_uses_its_only_provider() {
        let router = Router::primary(StubProvider::answering("local", "from local").shared());

        let completion = answer(&router, 0.9).await.expect("an answer");

        assert_eq!(completion.provider, "local");
        assert_eq!(completion.message.content.as_deref(), Some("from local"));
    }

    #[tokio::test]
    async fn a_primary_router_never_reaches_past_its_primary() {
        let backup = Arc::new(StubProvider::answering("backup", "from backup"));
        let router = Router::new(
            vec![
                Weighted::spare(StubProvider::failing("primary", "upstream reset").shared()),
                Weighted::spare(backup.clone()),
            ],
            SelectionStrategy::Primary,
        );

        let error = answer(&router, 0.5).await.expect_err("a failure");

        assert_eq!(backup.calls(), 0, "the backup was reached");
        assert!(matches!(
            error,
            ProviderError::Exhausted { attempted: 1, .. }
        ));
    }

    #[tokio::test]
    async fn a_weighted_split_honours_its_weights_across_a_seeded_sweep() {
        let router = Router::new(
            vec![
                Weighted::new(StubProvider::answering("control", "c").shared(), 80.0),
                Weighted::new(StubProvider::answering("variant", "v").shared(), 20.0),
            ],
            SelectionStrategy::Weighted,
        );

        let mut control = 0;
        let mut variant = 0;
        for step in 0..100 {
            let completion = answer(&router, f64::from(step) / 100.0)
                .await
                .expect("an answer");
            match completion.provider.as_str() {
                "control" => control += 1,
                "variant" => variant += 1,
                other => panic!("unexpected arm {other}"),
            }
        }

        assert_eq!((control, variant), (80, 20));
    }

    #[tokio::test]
    async fn a_chain_advances_past_a_failing_provider_and_stops_at_the_first_success() {
        let third = Arc::new(StubProvider::answering("third", "never reached"));
        let router = Router::fallback(vec![
            StubProvider::failing("first", "connection reset").shared(),
            StubProvider::answering("second", "from second").shared(),
            third.clone(),
        ]);

        let completion = answer(&router, 0.0).await.expect("an answer");

        assert_eq!(completion.provider, "second");
        assert_eq!(completion.message.content.as_deref(), Some("from second"));
        assert_eq!(third.calls(), 0, "the chain kept going after a success");
    }

    #[tokio::test]
    async fn an_exhausted_chain_reports_the_last_failure_not_a_generic_one() {
        let router = Router::fallback(vec![
            StubProvider::failing("first", "connection reset").shared(),
            StubProvider::failing("second", "socket hang up").shared(),
            StubProvider::failing("third", "429 rate limit reached").shared(),
        ]);

        let error = answer(&router, 0.0).await.expect_err("a failure");

        let ProviderError::Exhausted { attempted, last } = &error else {
            panic!("expected an exhausted chain, got {error:?}");
        };
        assert_eq!(*attempted, 3);
        assert_eq!(last.provider(), Some("third"));

        let rendered = error.to_string();
        assert!(
            rendered.contains("rate limit"),
            "lost the cause: {rendered}"
        );
        assert!(
            !rendered.contains("connection reset"),
            "reported an earlier failure: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_weighted_chain_falls_back_from_the_arm_it_drew() {
        let router = Router::weighted(vec![
            Weighted::new(
                StubProvider::answering("control", "from control").shared(),
                50.0,
            ),
            Weighted::new(
                StubProvider::failing("variant", "upstream reset").shared(),
                50.0,
            ),
        ]);

        let drawn = answer(&router, 0.75).await.expect("an answer");

        assert_eq!(
            drawn.provider, "control",
            "the variant should have fallen back"
        );
    }

    #[tokio::test]
    async fn a_rejected_request_stops_the_chain_immediately() {
        let backup = Arc::new(StubProvider::answering("backup", "from backup"));
        let router = Router::fallback(vec![
            StubProvider::rejecting("first", "messages is malformed").shared(),
            backup.clone(),
        ]);

        let error = answer(&router, 0.0).await.expect_err("a failure");

        assert_eq!(backup.calls(), 0, "a bad request was retried elsewhere");
        assert!(matches!(error, ProviderError::Http { .. }));
    }

    #[tokio::test]
    async fn a_router_with_no_providers_says_so() {
        let router = Router::new(Vec::new(), SelectionStrategy::Fallback);

        let error = answer(&router, 0.5).await.expect_err("a failure");

        assert!(matches!(error, ProviderError::Unconfigured));
    }

    #[tokio::test]
    async fn routers_compose_because_a_router_is_a_provider() {
        let inner = Router::fallback(vec![
            StubProvider::failing("inner-first", "connection reset").shared(),
            StubProvider::answering("inner-second", "from inner").shared(),
        ])
        .with_name("inner");

        let outer = Router::fallback(vec![
            StubProvider::failing("outer-first", "connection reset").shared(),
            Arc::new(inner),
        ]);

        let completion = answer(&outer, 0.0).await.expect("an answer");

        assert_eq!(completion.provider, "inner-second");
    }

    #[tokio::test]
    async fn the_drawn_arm_is_tried_before_the_rest_of_the_chain() {
        let control = Arc::new(StubProvider::answering("control", "c"));
        let router = Router::new(
            vec![
                Weighted::new(control.clone(), 50.0),
                Weighted::new(StubProvider::answering("variant", "v").shared(), 50.0),
            ],
            SelectionStrategy::WeightedFallback,
        );

        let completion = answer(&router, 0.9).await.expect("an answer");

        assert_eq!(completion.provider, "variant");
        assert_eq!(control.calls(), 0, "the unchosen arm was tried first");
    }
}
