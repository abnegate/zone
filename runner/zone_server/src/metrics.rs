//! Prometheus metrics for the HTTP API and Zone product jobs.
//!
//! HTTP RED (Manager dashboard / Performance alerts):
//! - `http_requests_total` with `method`, `path`, `status`
//! - `http_request_duration_seconds` histogram
//! - `http_requests_in_flight` gauge
//!
//! Domain series (chat-quality board): `zone_context_search_*`,
//! `zone_embedding_*`, `zone_gathering_*`, `zone_ws_chat_*`, `zone_task_run_*`,
//! `zone_searxng_*`, `zone_comfyui_*`, `zone_auth_*`, `zone_source_resync_*`.
//!
//! The recorder is process-wide and installed once so `create_router` can be
//! called from tests without conflicting global recorders.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
const HTTP_DURATION: &str = "http_request_duration_seconds";
const HTTP_IN_FLIGHT: &str = "http_requests_in_flight";

const SEARCH_REQUESTS: &str = "zone_context_search_requests_total";
const SEARCH_DURATION: &str = "zone_context_search_duration_seconds";
const SEARCH_RESULTS: &str = "zone_context_search_results";

const EMBED_REQUESTS: &str = "zone_embedding_requests_total";
const EMBED_DURATION: &str = "zone_embedding_duration_seconds";
const EMBED_BATCH: &str = "zone_embedding_batch_size";

const GATHER_RUNS: &str = "zone_gathering_runs_total";
const GATHER_DURATION: &str = "zone_gathering_duration_seconds";
const GATHER_EMBEDDINGS: &str = "zone_gathering_embeddings";
const GATHER_UNCHANGED: &str = "zone_gathering_items_unchanged";

const WS_CHAT_EVENTS: &str = "zone_ws_chat_connections_total";
const WS_CHAT_ACTIVE: &str = "zone_ws_chat_connections_active";

const TASK_RUNS: &str = "zone_task_run_total";
const TASK_DURATION: &str = "zone_task_run_duration_seconds";

const SEARXNG_REQUESTS: &str = "zone_searxng_requests_total";
const SEARXNG_DURATION: &str = "zone_searxng_duration_seconds";
const SEARXNG_RESULTS: &str = "zone_searxng_results";

const COMFY_REQUESTS: &str = "zone_comfyui_generations_total";
const COMFY_DURATION: &str = "zone_comfyui_generation_duration_seconds";

const AUTH_FAILURES: &str = "zone_auth_failures_total";
const AUTH_LOGIN: &str = "zone_auth_login_total";

const RESYNC_TRIGGERED: &str = "zone_source_resync_triggered_total";
const PROCESS_RSS: &str = "zone_process_resident_memory_bytes";

const HTTP_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const SEARCH_DURATION_BUCKETS: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];
const EMBED_DURATION_BUCKETS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 15.0, 30.0];
const JOB_DURATION_BUCKETS: &[f64] = &[1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
const SEARXNG_DURATION_BUCKETS: &[f64] = &[0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];
const COUNT_BUCKETS: &[f64] = &[0.0, 1.0, 5.0, 10.0, 20.0, 50.0, 100.0];
const BATCH_BUCKETS: &[f64] = &[1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

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

            describe_counter!(
                SEARCH_REQUESTS,
                "Context search requests by mode and status"
            );
            describe_histogram!(SEARCH_DURATION, "Context search duration in seconds");
            describe_histogram!(SEARCH_RESULTS, "Context search result count");
            describe_counter!(
                EMBED_REQUESTS,
                "Embedding calls by engine, model, and status"
            );
            describe_histogram!(EMBED_DURATION, "Embedding call duration in seconds");
            describe_histogram!(EMBED_BATCH, "Texts per embedding batch");
            describe_counter!(GATHER_RUNS, "Source gather/index runs");
            describe_histogram!(GATHER_DURATION, "Gather/index duration in seconds");
            describe_histogram!(GATHER_EMBEDDINGS, "Embeddings created per gather");
            describe_histogram!(GATHER_UNCHANGED, "Unchanged items skipped per gather");
            describe_counter!(WS_CHAT_EVENTS, "Chat WebSocket connect outcomes");
            describe_gauge!(WS_CHAT_ACTIVE, "Open chat WebSocket connections");
            describe_counter!(TASK_RUNS, "Background task run outcomes");
            describe_histogram!(TASK_DURATION, "Background task run duration in seconds");
            describe_counter!(SEARXNG_REQUESTS, "Outbound SearXNG searches");
            describe_histogram!(SEARXNG_DURATION, "SearXNG search duration in seconds");
            describe_histogram!(SEARXNG_RESULTS, "SearXNG hits returned");
            describe_counter!(COMFY_REQUESTS, "ComfyUI generation attempts");
            describe_histogram!(COMFY_DURATION, "ComfyUI generation duration in seconds");
            describe_counter!(AUTH_FAILURES, "HTTP authentication failures");
            describe_counter!(AUTH_LOGIN, "Password login attempts");
            describe_counter!(RESYNC_TRIGGERED, "Source resync jobs queued");
            describe_gauge!(
                PROCESS_RSS,
                "Manager process resident memory in bytes (from /proc/self/statm)"
            );

            PrometheusBuilder::new()
                .set_buckets_for_metric(Matcher::Full(HTTP_DURATION.into()), HTTP_DURATION_BUCKETS)
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(SEARCH_DURATION.into()),
                        SEARCH_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(EMBED_DURATION.into()),
                        EMBED_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(GATHER_DURATION.into()),
                        JOB_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(TASK_DURATION.into()),
                        JOB_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(SEARXNG_DURATION.into()),
                        SEARXNG_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(
                        Matcher::Full(COMFY_DURATION.into()),
                        JOB_DURATION_BUCKETS,
                    )
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(Matcher::Full(SEARCH_RESULTS.into()), COUNT_BUCKETS)
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(Matcher::Full(SEARXNG_RESULTS.into()), COUNT_BUCKETS)
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(Matcher::Full(GATHER_EMBEDDINGS.into()), COUNT_BUCKETS)
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(Matcher::Full(GATHER_UNCHANGED.into()), COUNT_BUCKETS)
                })
                .and_then(|b| {
                    b.set_buckets_for_metric(Matcher::Full(EMBED_BATCH.into()), BATCH_BUCKETS)
                })
                .expect("prometheus histogram buckets")
                .install_recorder()
                .expect("prometheus recorder")
        })
        .clone()
}

/// GET /metrics — Prometheus text exposition. Public on the internal network.
pub async fn scrape() -> impl IntoResponse {
    record_process_rss();
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        handle().render(),
    )
}

fn record_process_rss() {
    init();
    if let Some(bytes) = process_rss_bytes() {
        gauge!(PROCESS_RSS).set(bytes as f64);
    }
}

fn process_rss_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages.saturating_mul(4096))
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

/// Observes one `/api/context/search` attempt. Records on drop.
pub struct SearchObs {
    start: Instant,
    mode: String,
    status: &'static str,
    results: usize,
}

impl SearchObs {
    pub fn new() -> Self {
        init();
        Self {
            start: Instant::now(),
            mode: "unknown".to_string(),
            status: "error",
            results: 0,
        }
    }

    pub fn set_mode(&mut self, mode: impl Into<String>) {
        self.mode = mode.into();
    }

    pub fn set_status(&mut self, status: &'static str) {
        self.status = status;
    }

    pub fn succeed(&mut self, results: usize) {
        self.status = "ok";
        self.results = results;
    }
}

impl Drop for SearchObs {
    fn drop(&mut self) {
        let mode = self.mode.clone();
        let status = self.status;
        counter!(SEARCH_REQUESTS, "mode" => mode.clone(), "status" => status).increment(1);
        histogram!(SEARCH_DURATION, "mode" => mode.clone(), "status" => status)
            .record(self.start.elapsed().as_secs_f64());
        if self.status == "ok" {
            histogram!(SEARCH_RESULTS, "mode" => mode).record(self.results as f64);
        }
    }
}

pub fn record_embedding(
    engine: &str,
    model: &str,
    op: &'static str,
    status: &'static str,
    duration: Duration,
    batch_size: usize,
) {
    init();
    let engine = engine.to_string();
    let model = model.to_string();
    counter!(
        EMBED_REQUESTS,
        "engine" => engine.clone(),
        "model" => model.clone(),
        "op" => op,
        "status" => status
    )
    .increment(1);
    histogram!(
        EMBED_DURATION,
        "engine" => engine,
        "model" => model,
        "op" => op,
        "status" => status
    )
    .record(duration.as_secs_f64());
    histogram!(EMBED_BATCH, "op" => op).record(batch_size as f64);
}

/// Observes one gather/index worker run.
pub struct GatheringObs {
    start: Instant,
    kind: &'static str,
    status: &'static str,
    embeddings: u64,
    unchanged: u64,
}

impl GatheringObs {
    pub fn new(index_mode: bool) -> Self {
        init();
        Self {
            start: Instant::now(),
            kind: if index_mode { "index" } else { "gather" },
            status: "error",
            embeddings: 0,
            unchanged: 0,
        }
    }

    pub fn set_status(&mut self, status: &'static str) {
        self.status = status;
    }

    pub fn set_stats(&mut self, embeddings: u64, unchanged: u64) {
        self.embeddings = embeddings;
        self.unchanged = unchanged;
    }
}

impl Drop for GatheringObs {
    fn drop(&mut self) {
        let kind = self.kind;
        let status = self.status;
        counter!(GATHER_RUNS, "kind" => kind, "status" => status).increment(1);
        histogram!(GATHER_DURATION, "kind" => kind, "status" => status)
            .record(self.start.elapsed().as_secs_f64());
        if matches!(self.status, "completed" | "failed") {
            histogram!(GATHER_EMBEDDINGS, "kind" => kind).record(self.embeddings as f64);
            histogram!(GATHER_UNCHANGED, "kind" => kind).record(self.unchanged as f64);
        }
    }
}

/// Observes one background task run.
pub struct TaskObs {
    start: Instant,
    status: &'static str,
}

impl TaskObs {
    pub fn new() -> Self {
        init();
        Self {
            start: Instant::now(),
            status: "error",
        }
    }

    pub fn set_status(&mut self, status: &'static str) {
        self.status = status;
    }
}

impl Drop for TaskObs {
    fn drop(&mut self) {
        let status = self.status;
        counter!(TASK_RUNS, "status" => status).increment(1);
        histogram!(TASK_DURATION, "status" => status).record(self.start.elapsed().as_secs_f64());
    }
}

pub fn record_ws_chat(event: &'static str, reason: &'static str) {
    init();
    counter!(WS_CHAT_EVENTS, "event" => event, "reason" => reason).increment(1);
}

/// Increments the chat WS gauge and decrements it on drop.
pub struct WsActiveGuard;

impl WsActiveGuard {
    pub fn acquire() -> Self {
        init();
        record_ws_chat("opened", "ok");
        gauge!(WS_CHAT_ACTIVE).increment(1.0);
        Self
    }
}

impl Drop for WsActiveGuard {
    fn drop(&mut self) {
        gauge!(WS_CHAT_ACTIVE).decrement(1.0);
    }
}

pub fn record_searxng(status: &'static str, duration: Duration, results: usize) {
    init();
    counter!(SEARXNG_REQUESTS, "status" => status).increment(1);
    histogram!(SEARXNG_DURATION, "status" => status).record(duration.as_secs_f64());
    if status == "ok" {
        histogram!(SEARXNG_RESULTS).record(results as f64);
    }
}

pub fn record_comfyui(kind: &'static str, status: &'static str, duration: Duration) {
    init();
    counter!(COMFY_REQUESTS, "kind" => kind, "status" => status).increment(1);
    histogram!(COMFY_DURATION, "kind" => kind, "status" => status).record(duration.as_secs_f64());
}

pub fn record_auth_failure(reason: &'static str) {
    init();
    counter!(AUTH_FAILURES, "reason" => reason).increment(1);
}

pub fn record_login(status: &'static str) {
    init();
    counter!(AUTH_LOGIN, "status" => status).increment(1);
}

pub fn record_resync(reason: &'static str) {
    init();
    counter!(RESYNC_TRIGGERED, "reason" => reason).increment(1);
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
    use super::{SearchObs, normalize_path, record_embedding, record_searxng};
    use std::time::Duration;

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

    #[test]
    fn domain_helpers_render_in_scrape() {
        {
            let mut obs = SearchObs::new();
            obs.set_mode("hybrid");
            obs.succeed(3);
        }
        record_embedding(
            "ollama",
            "nomic-embed-text",
            "single",
            "ok",
            Duration::from_millis(12),
            1,
        );
        record_searxng("ok", Duration::from_millis(40), 4);
        let body = super::handle().render();
        assert!(
            body.contains("zone_context_search_requests_total"),
            "{body}"
        );
        assert!(body.contains("zone_embedding_requests_total"), "{body}");
        assert!(body.contains("zone_searxng_requests_total"), "{body}");
    }
}
