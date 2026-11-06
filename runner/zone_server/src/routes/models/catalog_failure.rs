use abnegate_llm::catalog::CatalogError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::catalog_source::CatalogSource;
use super::types::ErrorResponse;

const UNUSABLE_URL: &str = "Model catalog URL is not a usable HTTP endpoint";

/// Why a catalogue browse failed, in the words and status Zone has always
/// answered `/api/models` and the agent's browse tool with.
#[derive(Debug)]
pub(crate) enum CatalogFailure {
    Catalog {
        source: CatalogSource,
        error: CatalogError,
    },
    UnknownSource(String),
}

impl CatalogFailure {
    pub(crate) fn catalog(source: CatalogSource, error: CatalogError) -> Self {
        Self::Catalog { source, error }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::UnknownSource(_) => StatusCode::BAD_REQUEST,
            Self::Catalog {
                error:
                    CatalogError::Http(_)
                    | CatalogError::Unavailable(_)
                    | CatalogError::Status(_)
                    | CatalogError::InvalidUrl(_),
                ..
            } => StatusCode::BAD_GATEWAY,
            Self::Catalog { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::UnknownSource(source) => format!("Unknown source: {source}"),
            Self::Catalog { source, error } => match error {
                CatalogError::Http(error) => format!("Failed to connect: {error}"),
                CatalogError::Parse(error) => format!("Failed to parse response: {error}"),
                CatalogError::Unavailable(reason) => format!("Provider unavailable: {reason}"),
                CatalogError::Status(code) => format!(
                    "Provider unavailable: {} returned status: {}",
                    source.upstream(),
                    status_text(*code)
                ),
                CatalogError::InvalidUrl(url) => {
                    format!("Provider unavailable: {UNUSABLE_URL}: {url}")
                }
                error => error.to_string(),
            },
        }
    }
}

fn status_text(code: u16) -> String {
    StatusCode::from_u16(code).map_or_else(|_| code.to_string(), |status| status.to_string())
}

impl IntoResponse for CatalogFailure {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = self.message();
        if let Self::Catalog { .. } = self {
            tracing::error!(%status, "{message}");
        }
        (status, Json(ErrorResponse::new(message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_llm::catalog::{BrowseQuery, HuggingFaceProvider, ModelProvider};

    fn rendered(source: CatalogSource, error: CatalogError) -> (StatusCode, String) {
        let failure = CatalogFailure::catalog(source, error);
        (failure.status(), failure.message())
    }

    #[test]
    fn a_huggingface_status_reads_as_zone_always_wrote_it() {
        assert_eq!(
            rendered(CatalogSource::HuggingFace, CatalogError::Status(503)),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: HuggingFace API returned status: 503 Service Unavailable"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_status_from_another_catalogue_names_that_catalogue() {
        assert_eq!(
            rendered(CatalogSource::OpenRouter, CatalogError::Status(429)),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: OpenRouter API returned status: 429 Too Many Requests"
                    .to_string()
            )
        );
        assert_eq!(
            rendered(CatalogSource::Ollama, CatalogError::Status(500)).1,
            "Provider unavailable: Ollama library returned status: 500 Internal Server Error"
        );
    }

    #[test]
    fn a_status_outside_http_keeps_its_number() {
        assert_eq!(
            rendered(CatalogSource::HuggingFace, CatalogError::Status(1_000)).1,
            "Provider unavailable: HuggingFace API returned status: 1000"
        );
    }

    #[test]
    fn an_unavailable_catalogue_is_a_bad_gateway() {
        assert_eq!(
            rendered(
                CatalogSource::Ollama,
                CatalogError::Unavailable(
                    "Ollama library returned status: 500 Internal Server Error".to_string()
                )
            ),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: Ollama library returned status: 500 Internal Server Error"
                    .to_string()
            )
        );
    }

    #[test]
    fn an_unreadable_answer_is_a_server_error() {
        assert_eq!(
            rendered(
                CatalogSource::Ollama,
                CatalogError::Parse("Invalid cursor offset format".to_string())
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to parse response: Invalid cursor offset format".to_string()
            )
        );
    }

    #[tokio::test]
    async fn an_unusable_catalogue_url_is_a_bad_gateway_naming_the_crates_reason() {
        let error = HuggingFaceProvider::new("ftp://mirror")
            .expect("a provider")
            .search(BrowseQuery::default())
            .await
            .expect_err("ftp is not a catalogue scheme");
        assert!(matches!(error, CatalogError::InvalidUrl(_)), "{error:?}");
        assert_eq!(
            rendered(CatalogSource::HuggingFace, error),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: Model catalog URL is not a usable HTTP endpoint: \
                 ftp://mirror: the ftp scheme is not http or https"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_local_file_failure_is_a_server_error() {
        let (status, message) = rendered(
            CatalogSource::HuggingFace,
            CatalogError::Io(std::io::Error::other("disk gone")),
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, "Filesystem error: disk gone");
    }

    #[test]
    fn an_unknown_source_is_a_bad_request() {
        let failure = CatalogFailure::UnknownSource("modelscope".to_string());
        assert_eq!(failure.status(), StatusCode::BAD_REQUEST);
        assert_eq!(failure.message(), "Unknown source: modelscope");
    }

    #[tokio::test]
    async fn a_connection_failure_is_a_bad_gateway() {
        let error = reqwest::Client::new()
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .expect_err("nothing listens on port 1");
        let (status, message) = rendered(CatalogSource::Ollama, CatalogError::from(error));
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(message.starts_with("Failed to connect: "), "{message}");
    }
}
