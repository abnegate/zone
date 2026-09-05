//! Prometheus RED metrics for the HTTP API.
//!
//! Grafana's Manager dashboard and the Performance alert rules expect:
//! - `http_requests_total` with `method`, `path`, `status`
//! - `http_request_duration_seconds` histogram
//! - `http_requests_in_flight` gauge
//!
//! The recorder is process-wide and installed once so `create_router` can be
//! called from tests without conflicting global recorders.

use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
const HTTP_DURATION: &str = "http_request_duration_seconds";
const HTTP_IN_FLIGHT: &str = "http_requests_in_flight";

/// Default Prometheus HTTP histogram buckets, plus 30s for slow API calls.
const HTTP_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

/// Install the process-wide Prometheus recorder if it is not already installed.
pub fn init() {
    let _ = handle();
}

fn handle() -> PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            describe_counter!(
                HTTP_REQUESTS_TOTAL,
                "Total HTTP requests, labeled by method, path, and status"
            );
            describe_histogram!(
                HTTP_DURATION,
                "HTTP request duration in seconds, labeled by method and path"
            );
            describe_gauge!(HTTP_IN_FLIGHT, "HTTP requests currently in flight");

            PrometheusBuilder::new()
                .set_buckets_for_metric(
                    Matcher::Full(HTTP_DURATION.to_string()),
                    HTTP_DURATION_BUCKETS,
                )
                .expect("HTTP duration histogram buckets")
                .install_recorder()
                .expect("prometheus recorder")
        })
        .clone()
}

/// GET /metrics — Prometheus text exposition. Public on the internal network.
pub async fn scrape() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        handle().render(),
    )
}

/// Record RED metrics for every request except the scrape endpoint itself.
pub async fn track_http(request: Request, next: Next) -> Response {
    init();

    let method = request.method().as_str().to_owned();
    let path = normalize_path(request.uri().path());
    if path == "/metrics" {
        return next.run(request).await;
    }

    gauge!(HTTP_IN_FLIGHT).increment(1.0);
    let _in_flight = InFlightGuard;
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();

    counter!(
        HTTP_REQUESTS_TOTAL,
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status
    )
    .increment(1);
    histogram!(HTTP_DURATION, "method" => method, "path" => path)
        .record(started.elapsed().as_secs_f64());

    response
}

struct InFlightGuard;

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        gauge!(HTTP_IN_FLIGHT).decrement(1.0);
    }
}

/// Collapse UUIDs, numeric IDs, and long hex tokens so path labels stay bounded.
pub(crate) fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }

    let mut out = String::with_capacity(path.len());
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        out.push('/');
        if is_dynamic_segment(segment) {
            out.push_str("{id}");
        } else {
            out.push_str(segment);
        }
    }

    if out.is_empty() { "/".to_string() } else { out }
}

fn is_dynamic_segment(segment: &str) -> bool {
    if uuid::Uuid::parse_str(segment).is_ok() {
        return true;
    }
    if !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // Invitation tokens are 64-char hex; also fold other long hex IDs.
    segment.len() >= 16
        && segment.len().is_multiple_of(2)
        && segment.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::normalize_path;

    #[test]
    fn root_and_empty_stay_slash() {
        assert_eq!(normalize_path(""), "/");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn static_api_paths_are_unchanged() {
        assert_eq!(normalize_path("/health"), "/health");
        assert_eq!(normalize_path("/api/auth/login"), "/api/auth/login");
        assert_eq!(
            normalize_path("/api/models/llama3.2:latest"),
            "/api/models/llama3.2:latest"
        );
    }

    #[test]
    fn uuids_and_numeric_ids_collapse() {
        assert_eq!(
            normalize_path("/api/organizations/550e8400-e29b-41d4-a716-446655440000/usage"),
            "/api/organizations/{id}/usage"
        );
        assert_eq!(normalize_path("/api/plans/42"), "/api/plans/{id}");
    }

    #[test]
    fn hex_invitation_tokens_collapse() {
        let token = "a".repeat(64);
        assert_eq!(
            normalize_path(&format!("/api/invitations/{token}")),
            "/api/invitations/{id}"
        );
    }
}
