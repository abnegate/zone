use abnegate_llm::catalog::CatalogError;
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;

use super::types::ErrorResponse;

const HUGGINGFACE_STATUS: &str = "HuggingFace API returned status";
const UNUSABLE_URL: &str = "Model catalog URL is not a usable HTTP endpoint";

/// Why a catalogue browse failed, in the words and status Zone has always
/// answered `/api/models` and the agent's browse tool with.
#[derive(Debug)]
pub(crate) enum CatalogFailure {
    Catalog(CatalogError),
    UnknownSource(String),
}

impl CatalogFailure {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::UnknownSource(_) => StatusCode::BAD_REQUEST,
            Self::Catalog(
                CatalogError::Http(_)
                | CatalogError::Unavailable(_)
                | CatalogError::Status(_)
                | CatalogError::InvalidUrl(_),
            ) => StatusCode::BAD_GATEWAY,
            Self::Catalog(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::UnknownSource(source) => format!("Unknown source: {source}"),
            Self::Catalog(CatalogError::Http(error)) => format!("Failed to connect: {error}"),
            Self::Catalog(CatalogError::Parse(error)) => {
                format!("Failed to parse response: {error}")
            }
            Self::Catalog(CatalogError::Unavailable(reason)) => {
                format!("Provider unavailable: {reason}")
            }
            Self::Catalog(CatalogError::Status(code)) => format!(
                "Provider unavailable: {HUGGINGFACE_STATUS}: {}",
                status_text(*code)
            ),
            Self::Catalog(CatalogError::InvalidUrl(url)) => {
                format!("Provider unavailable: {UNUSABLE_URL}: {url}")
            }
            Self::Catalog(error) => error.to_string(),
        }
    }
}

fn status_text(code: u16) -> String {
    StatusCode::from_u16(code).map_or_else(|_| code.to_string(), |status| status.to_string())
}

impl From<CatalogError> for CatalogFailure {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl IntoResponse for CatalogFailure {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = self.message();
        if let Self::Catalog(_) = self {
            tracing::error!(%status, "{message}");
        }
        (status, Json(ErrorResponse::new(message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(error: CatalogError) -> (StatusCode, String) {
        let failure = CatalogFailure::from(error);
        (failure.status(), failure.message())
    }

    #[test]
    fn a_huggingface_status_reads_as_zone_always_wrote_it() {
        assert_eq!(
            rendered(CatalogError::Status(503)),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: HuggingFace API returned status: 503 Service Unavailable"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_status_outside_http_keeps_its_number() {
        assert_eq!(
            rendered(CatalogError::Status(1_000)).1,
            "Provider unavailable: HuggingFace API returned status: 1000"
        );
    }

    #[test]
    fn an_unavailable_catalogue_is_a_bad_gateway() {
        assert_eq!(
            rendered(CatalogError::Unavailable(
                "Ollama library returned status: 500 Internal Server Error".to_string()
            )),
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
            rendered(CatalogError::Parse(
                "Invalid cursor offset format".to_string()
            )),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to parse response: Invalid cursor offset format".to_string()
            )
        );
    }

    #[test]
    fn an_unusable_catalogue_url_is_a_bad_gateway() {
        assert_eq!(
            rendered(CatalogError::InvalidUrl("ftp://mirror".to_string())),
            (
                StatusCode::BAD_GATEWAY,
                "Provider unavailable: Model catalog URL is not a usable HTTP endpoint: ftp://mirror"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_local_file_failure_is_a_server_error() {
        let (status, message) = rendered(CatalogError::Io(std::io::Error::other("disk gone")));
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
        let (status, message) = rendered(CatalogError::from(error));
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(message.starts_with("Failed to connect: "), "{message}");
    }
}
