//! The upstream a LiteLLM endpoint sends a model to, read from the deployment
//! routes it publishes at `/v2/model/info`.

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::services::endpoint::Endpoint;

const INFORMATION_PATH: &str = "/v2/model/info";
const TIMEOUT: Duration = Duration::from_secs(2);
const TTL: Duration = Duration::from_secs(60);
const UNREAD_TTL: Duration = Duration::from_secs(5);
const VERSION_PATH: &str = "/v1";
const WILDCARD: char = '*';

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(TIMEOUT)
        .timeout(TIMEOUT)
        .build()
        .expect("Failed to build upstream route client")
});

static TABLES: LazyLock<Tables> = LazyLock::new(Tables::default);

#[derive(Default)]
struct Tables {
    read: DashMap<String, Table>,
    reading: DashMap<String, Arc<Mutex<()>>>,
}

struct Table {
    routes: Option<Arc<[Route]>>,
    fetched: Instant,
}

impl Table {
    fn fresh(&self, now: Instant) -> bool {
        let ttl = match self.routes {
            Some(_) => TTL,
            None => UNREAD_TTL,
        };
        now.saturating_duration_since(self.fetched) < ttl
    }
}

#[derive(Deserialize)]
struct Page {
    data: Vec<Deployment>,
    #[serde(default)]
    total_pages: Option<u64>,
}

#[derive(Deserialize)]
struct Deployment {
    model_name: String,
    litellm_params: Parameters,
}

#[derive(Deserialize)]
struct Parameters {
    model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    name: String,
    model: String,
}

impl From<Deployment> for Route {
    fn from(deployment: Deployment) -> Self {
        Self {
            name: deployment.model_name,
            model: deployment.litellm_params.model,
        }
    }
}

impl Route {
    fn capture<'a>(&self, model: &'a str) -> Option<&'a str> {
        let (prefix, suffix) = self.name.split_once(WILDCARD)?;
        model.strip_prefix(prefix)?.strip_suffix(suffix)
    }
}

/// The LiteLLM model `endpoint` sends `model` to, such as
/// `bedrock/anthropic.claude-sonnet-4` or `ollama_chat/llama3:8b`. `None`
/// when the endpoint publishes no deployment metadata or no route for it.
pub async fn route(endpoint: &Endpoint, model: &str) -> Option<String> {
    lookup(&TABLES, endpoint, model).await
}

async fn lookup(tables: &Tables, endpoint: &Endpoint, model: &str) -> Option<String> {
    let url = endpoint.url();
    if let Some(routes) = cached(&tables.read, url, Instant::now()) {
        return resolve(routes.as_deref()?, model);
    }
    let reading = Arc::clone(tables.reading.entry(url.to_string()).or_default().value());
    let _reading = reading.lock().await;
    if let Some(routes) = cached(&tables.read, url, Instant::now()) {
        return resolve(routes.as_deref()?, model);
    }
    let routes: Option<Arc<[Route]>> = fetch(endpoint).await.map(Into::into);
    store(&tables.read, url, routes.clone(), Instant::now());
    tables.reading.remove(url);
    resolve(routes.as_deref()?, model)
}

/// The routes read from `url` within their TTL: none read when the last read failed.
fn cached(
    tables: &DashMap<String, Table>,
    url: &str,
    now: Instant,
) -> Option<Option<Arc<[Route]>>> {
    tables
        .get(url)
        .filter(|table| table.fresh(now))
        .map(|table| table.routes.clone())
}

fn store(tables: &DashMap<String, Table>, url: &str, routes: Option<Arc<[Route]>>, now: Instant) {
    tables.retain(|_, table| table.fresh(now));
    tables.insert(
        url.to_string(),
        Table {
            routes,
            fetched: now,
        },
    );
}

/// An exact name first, then the longest wildcard name that matches, with
/// its `*` given what the wildcard matched.
fn resolve(routes: &[Route], model: &str) -> Option<String> {
    if let Some(route) = routes.iter().find(|route| route.name == model) {
        return Some(route.model.clone());
    }
    routes
        .iter()
        .filter_map(|route| route.capture(model).map(|captured| (route, captured)))
        .max_by_key(|(route, _)| route.name.len())
        .map(|(route, captured)| route.model.replace(WILDCARD, captured))
}

async fn fetch(endpoint: &Endpoint) -> Option<Vec<Route>> {
    let url = information_url(endpoint.url());
    match tokio::time::timeout(TIMEOUT, pages(&url, endpoint)).await {
        Ok(Ok(routes)) => Some(routes),
        Ok(Err(error)) => {
            tracing::warn!(%error, url, "Could not read the endpoint's deployment routes");
            None
        }
        Err(_) => {
            tracing::warn!(url, "Timed out reading the endpoint's deployment routes");
            None
        }
    }
}

async fn pages(url: &str, endpoint: &Endpoint) -> Result<Vec<Route>, reqwest::Error> {
    let mut routes = Vec::new();
    let mut page = 1_u64;
    loop {
        let body = CLIENT
            .get(format!("{url}?page={page}"))
            .bearer_auth(endpoint.key().expose())
            .send()
            .await?
            .error_for_status()?
            .json::<Page>()
            .await?;
        routes.extend(body.data.into_iter().map(Route::from));
        if page >= body.total_pages.unwrap_or(1) {
            return Ok(routes);
        }
        page += 1;
    }
}

/// LiteLLM serves its metadata at the root, beside the `/v1` a saved
/// endpoint is versioned with.
fn information_url(endpoint: &str) -> String {
    let root = endpoint.trim_end_matches('/');
    let root = root.strip_suffix(VERSION_PATH).unwrap_or(root);
    format!("{root}{INFORMATION_PATH}")
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::services::endpoint::testing::endpoint;

    const KEY: &str = "sk-upstream-key-0123456789";

    fn deployments(routes: &[(&str, &str)]) -> Value {
        let data: Vec<Value> = routes
            .iter()
            .map(|(name, model)| {
                json!({
                    "model_name": name,
                    "litellm_params": {"model": model},
                    "model_info": {"max_input_tokens": 128_000}
                })
            })
            .collect();
        json!({"data": data, "total_pages": 1})
    }

    async fn serving(body: Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(INFORMATION_PATH))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    async fn routed(server: &MockServer, model: &str) -> Option<String> {
        lookup(&Tables::default(), &endpoint(&server.uri(), KEY), model).await
    }

    #[tokio::test]
    async fn an_exact_name_routes_before_any_wildcard() {
        let server = serving(deployments(&[
            ("*", "ollama_chat/*"),
            ("alias", "openai/remote"),
        ]))
        .await;

        assert_eq!(
            routed(&server, "alias").await.as_deref(),
            Some("openai/remote")
        );
    }

    #[tokio::test]
    async fn an_unnamed_model_falls_back_to_the_catch_all() {
        let server = serving(deployments(&[
            ("*", "ollama_chat/*"),
            ("alias", "openai/remote"),
        ]))
        .await;

        assert_eq!(
            routed(&server, "llama3:8b").await.as_deref(),
            Some("ollama_chat/llama3:8b")
        );
    }

    #[tokio::test]
    async fn the_longest_wildcard_wins() {
        let server = serving(deployments(&[
            ("*", "ollama_chat/*"),
            ("anthropic/*", "anthropic/*"),
            ("anthropic/claude-*", "bedrock/anthropic.claude-*"),
        ]))
        .await;

        assert_eq!(
            routed(&server, "anthropic/claude-sonnet-4")
                .await
                .as_deref(),
            Some("bedrock/anthropic.claude-sonnet-4")
        );
        assert_eq!(
            routed(&server, "anthropic/other").await.as_deref(),
            Some("anthropic/other")
        );
        assert_eq!(
            routed(&server, "qwen3:4b").await.as_deref(),
            Some("ollama_chat/qwen3:4b")
        );
    }

    #[tokio::test]
    async fn a_model_with_no_route_has_none() {
        let server = serving(deployments(&[("alias", "openai/remote")])).await;

        assert_eq!(routed(&server, "unlisted").await, None);
    }

    #[tokio::test]
    async fn a_missing_metadata_endpoint_has_no_route() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        assert_eq!(routed(&server, "alias").await, None);
    }

    #[tokio::test]
    async fn a_slow_metadata_endpoint_has_no_route() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(deployments(&[("alias", "openai/remote")]))
                    .set_delay(TIMEOUT + Duration::from_secs(1)),
            )
            .mount(&server)
            .await;

        assert_eq!(routed(&server, "alias").await, None);
    }

    #[tokio::test]
    async fn malformed_metadata_has_no_route() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"data\": [{\"model\""))
            .mount(&server)
            .await;

        assert_eq!(routed(&server, "alias").await, None);
    }

    #[tokio::test]
    async fn every_page_is_read() {
        let server = MockServer::start().await;
        let mut first = deployments(&[("alias", "openai/remote")]);
        first["total_pages"] = 2.into();
        let mut second = deployments(&[("*", "ollama_chat/*")]);
        second["total_pages"] = 2.into();
        for (page, body) in [("1", first), ("2", second)] {
            Mock::given(path(INFORMATION_PATH))
                .and(query_param("page", page))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
        }

        assert_eq!(
            routed(&server, "llama3:8b").await.as_deref(),
            Some("ollama_chat/llama3:8b")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_versioned_endpoint_reads_metadata_at_its_root() {
        let server = serving(deployments(&[("alias", "vertex_ai/gemini-2.5-pro")])).await;
        let tables = Tables::default();

        let route = lookup(
            &tables,
            &endpoint(&format!("{}/v1/", server.uri()), KEY),
            "alias",
        )
        .await;

        assert_eq!(route.as_deref(), Some("vertex_ai/gemini-2.5-pro"));
    }

    #[tokio::test]
    async fn models_on_one_endpoint_share_a_single_request() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(deployments(&[
                ("*", "ollama_chat/*"),
                ("alias", "openai/remote"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let tables = Tables::default();
        let endpoint = endpoint(&server.uri(), KEY);

        assert_eq!(
            lookup(&tables, &endpoint, "alias").await.as_deref(),
            Some("openai/remote")
        );
        assert_eq!(
            lookup(&tables, &endpoint, "llama3:8b").await.as_deref(),
            Some("ollama_chat/llama3:8b")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_failed_read_is_remembered_briefly() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let tables = Tables::default();
        let endpoint = endpoint(&server.uri(), KEY);

        assert_eq!(lookup(&tables, &endpoint, "alias").await, None);
        assert_eq!(lookup(&tables, &endpoint, "llama3:8b").await, None);
        server.verify().await;
    }

    #[tokio::test]
    async fn concurrent_lookups_share_one_read() {
        let server = MockServer::start().await;
        Mock::given(path(INFORMATION_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(deployments(&[("*", "ollama_chat/*")]))
                    .set_delay(Duration::from_millis(200)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let tables = Tables::default();
        let endpoint = endpoint(&server.uri(), KEY);
        let models: Vec<String> = (0..8).map(|index| format!("model-{index}")).collect();

        let routes =
            futures::future::join_all(models.iter().map(|model| lookup(&tables, &endpoint, model)))
                .await;

        for (model, route) in models.iter().zip(routes) {
            assert_eq!(route, Some(format!("ollama_chat/{model}")));
        }
        server.verify().await;
    }

    #[test]
    fn a_table_expires_after_its_ttl() {
        let tables = DashMap::new();
        let now = Instant::now();
        store(&tables, "http://first", Some(Arc::from([])), now);

        assert!(cached(&tables, "http://first", now + TTL - Duration::from_secs(1)).is_some());
        assert!(cached(&tables, "http://first", now + TTL).is_none());
    }

    #[test]
    fn a_failed_read_expires_long_before_a_table() {
        let tables = DashMap::new();
        let now = Instant::now();
        store(&tables, "http://unread", None, now);

        assert_eq!(
            cached(
                &tables,
                "http://unread",
                now + UNREAD_TTL - Duration::from_secs(1)
            ),
            Some(None)
        );
        assert_eq!(cached(&tables, "http://unread", now + UNREAD_TTL), None);
        assert!(UNREAD_TTL < TTL);
    }

    #[test]
    fn storing_a_table_evicts_expired_ones() {
        let tables = DashMap::new();
        let now = Instant::now();
        store(&tables, "http://first", Some(Arc::from([])), now);
        store(&tables, "http://second", Some(Arc::from([])), now + TTL / 2);

        store(&tables, "http://third", Some(Arc::from([])), now + TTL);

        assert!(!tables.contains_key("http://first"), "expired");
        assert!(tables.contains_key("http://second"), "still fresh");
        assert!(tables.contains_key("http://third"));
    }
}
