use std::sync::Arc;
use std::time::Duration;

use abnegate_llm::ProviderError;
use abnegate_llm::RerankProvider;
use abnegate_llm::modality::vendor::HttpRerankConfig;
use abnegate_llm::modality::vendor::HttpRerankProvider;
use async_trait::async_trait;
use reqwest::StatusCode;

use crate::embeddings::rerank::CrossEncoder;
use crate::error::ContextError;
use crate::error::Result;

const MODEL_VARIABLE: &str = "RERANK_MODEL";
const TIMEOUT_VARIABLE: &str = "RERANK_TIMEOUT_MS";
const DEFAULT_MODEL: &str = "rerank";
const DEFAULT_TIMEOUT_MILLISECONDS: u64 = 2500;
const PROBE_PATHS: [&str; 3] = ["/v1/rerank", "/api/rerank", "/rerank"];
const PROBE_QUERY: &str = "What does should_skip_blob do?";
const PROBE_DOCUMENTS: [&str; 2] = [
    "fn should_skip_blob(uri: &str, sha: &str) -> bool",
    "generic authentication helper with a login form",
];
const PROBE_MARGIN: f32 = 0.02;
const UNCLASSIFIED_FAILURE: &str = "rerank failed";
const VERBATIM_PARSE_DETAILS: [&str; 3] = [
    "empty rerank response",
    "rerank response scored no document",
    "empty rerank pair",
];

pub struct HttpCrossEncoder {
    provider: HttpRerankProvider,
}

impl HttpCrossEncoder {
    pub fn new(url: &str, model: &str) -> Result<Self> {
        let config = HttpRerankConfig::new(url, model).with_timeout(timeout());
        let provider = HttpRerankProvider::new(config).map_err(context_error)?;
        Ok(Self { provider })
    }

    pub fn model(&self) -> &str {
        self.provider.model()
    }

    /// Probe a host's `/v1/rerank`, `/api/rerank` and `/rerank` in turn. Model-name
    /// matching is not enough — the endpoint must return a discriminative pair score.
    pub async fn probe(base_url: &str) -> Option<Arc<dyn CrossEncoder>> {
        let model = model(None);
        let base = base_url.trim_end_matches('/');
        for path in PROBE_PATHS {
            if let Some(encoder) = Self::probe_url(&format!("{base}{path}"), Some(&model)).await {
                return Some(encoder);
            }
        }
        None
    }

    pub async fn probe_url(url: &str, model: Option<&str>) -> Option<Arc<dyn CrossEncoder>> {
        let model = self::model(model);
        let encoder = Self::new(url, &model).ok()?;
        let scores = encoder
            .score_documents(PROBE_QUERY, &PROBE_DOCUMENTS)
            .await
            .ok()?;
        if scores.len() == PROBE_DOCUMENTS.len() && scores[0] > scores[1] + PROBE_MARGIN {
            tracing::info!(url, model = %encoder.model(), "using HTTP cross-encoder reranker");
            Some(Arc::new(encoder))
        } else {
            None
        }
    }
}

#[async_trait]
impl CrossEncoder for HttpCrossEncoder {
    async fn score_pair(&self, query: &str, document: &str) -> Result<f32> {
        self.provider
            .score(query, document)
            .await
            .map_err(context_error)
    }

    async fn score_documents(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
        let documents: Vec<String> = documents
            .iter()
            .map(|document| (*document).to_string())
            .collect();
        self.provider
            .rerank(query, &documents)
            .await
            .map_err(context_error)
    }
}

fn model(explicit: Option<&str>) -> String {
    resolve_model(explicit, std::env::var(MODEL_VARIABLE).ok())
}

fn resolve_model(explicit: Option<&str>, environment: Option<String>) -> String {
    explicit
        .map(str::to_string)
        .or_else(|| environment.filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| DEFAULT_MODEL.into())
}

fn timeout() -> Duration {
    let milliseconds = std::env::var(TIMEOUT_VARIABLE)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MILLISECONDS);
    Duration::from_millis(milliseconds)
}

fn context_error(error: ProviderError) -> ContextError {
    match error {
        ProviderError::Network { detail, .. } => {
            ContextError::Embedding(format!("rerank request: {detail}"))
        }
        ProviderError::Api { status, .. } => {
            ContextError::Embedding(format!("rerank API {}", status_line(status)))
        }
        ProviderError::Parse { detail, .. }
            if VERBATIM_PARSE_DETAILS.contains(&detail.as_str()) =>
        {
            ContextError::Embedding(detail)
        }
        ProviderError::Parse { detail, .. } => {
            ContextError::Embedding(format!("rerank parse: {detail}"))
        }
        ProviderError::Config { detail, .. } => {
            ContextError::Config(format!("rerank client: {detail}"))
        }
        _ => ContextError::Embedding(UNCLASSIFIED_FAILURE.to_string()),
    }
}

fn status_line(status: u16) -> String {
    StatusCode::from_u16(status).map_or_else(|_| status.to_string(), |code| code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_OUTSIDE_HTTP: u16 = 1000;

    fn embedding_message(error: ProviderError) -> String {
        match context_error(error) {
            ContextError::Embedding(message) => message,
            other => panic!("expected an embedding failure, got {other:?}"),
        }
    }

    #[test]
    fn a_refusal_drops_its_message() {
        assert_eq!(
            embedding_message(ProviderError::api(429, "secret document")),
            "rerank API 429 Too Many Requests"
        );
        assert_eq!(
            embedding_message(ProviderError::api(STATUS_OUTSIDE_HTTP, "secret document")),
            format!("rerank API {STATUS_OUTSIDE_HTTP}")
        );
    }

    #[test]
    fn a_network_failure_is_a_request_failure() {
        assert_eq!(
            embedding_message(ProviderError::network("connection refused")),
            "rerank request: connection refused"
        );
    }

    #[test]
    fn a_configuration_failure_is_a_client_failure() {
        let error = context_error(ProviderError::config("invalid base URL: relative URL"));
        assert!(
            matches!(&error, ContextError::Config(message)
                if message == "rerank client: invalid base URL: relative URL"),
            "{error:?}"
        );
    }

    #[test]
    fn an_explicit_model_wins() {
        assert_eq!(
            resolve_model(Some("explicit"), Some("environment".into())),
            "explicit"
        );
    }

    #[test]
    fn the_environment_model_fills_in_for_a_missing_argument() {
        assert_eq!(
            resolve_model(None, Some("environment".into())),
            "environment"
        );
        assert_eq!(resolve_model(None, None), DEFAULT_MODEL);
    }

    #[test]
    fn a_blank_environment_model_is_absent() {
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(
                resolve_model(None, Some(blank.into())),
                DEFAULT_MODEL,
                "{blank:?}"
            );
        }
    }

    #[test]
    fn any_other_failure_drops_its_wording() {
        assert_eq!(
            embedding_message(ProviderError::unsupported("secret document")),
            UNCLASSIFIED_FAILURE
        );
    }
}
