use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;

use abnegate_comfy::inventory;
use abnegate_comfy::inventory::WeightSidecar;
use abnegate_comfy::train::Contract;
use axum::Json;
use axum::Router;
use axum::body::to_bytes;
use axum::extract::Path as RoutePath;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use tokio::net::TcpListener;

use super::ListModelsQuery;
use super::browse_models;
use super::catalog_endpoints::CatalogEndpoints;
use super::catalog_endpoints::ENDPOINTS;
use super::list;
use crate::auth::AuthUser;
use crate::auth::jwt::Claims;
use crate::config::Config;
use crate::state::AppState;
use crate::state::test_config;

const RECORD_ENVIRONMENT: &str = "ZONE_RECORD_CATALOG_SNAPSHOTS";
const CI_ENVIRONMENT: &str = "CI";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/catalog");
const LAZY_DATABASE: &str = "postgres://localhost/test";
const NEXT_CURSOR: &str = "{next_cursor}";
const ADAPTER_FILTER: &str = "base_model:adapter:";
const MANIFESTS: &str = "/manifests/";
const DEFAULT_TAG: &str = "latest";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";
const LAYER_MEDIA_TYPE: &str = "application/vnd.ollama.image.model";
const CONFIG_MEDIA_TYPE: &str = "application/vnd.docker.container.image.v1+json";
const MANIFEST_CONFIG_SIZE: u64 = 487;

#[derive(Debug, Clone, Copy)]
enum Source {
    Comfy,
    Gpt4All,
    HuggingFace,
    Ollama,
    OpenRouter,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Comfy => "comfy",
            Self::Gpt4All => "gpt4all",
            Self::HuggingFace => "huggingface",
            Self::Ollama => "ollama",
            Self::OpenRouter => "openrouter",
        }
    }

    fn directory(self) -> PathBuf {
        Path::new(FIXTURES).join(self.name())
    }
}

/// One `/api/models` request against one recorded upstream.
///
/// `world` names the `<world>.input.json` the mocks serve. When `follows` is
/// set, that query runs first and its `next_cursor` replaces [`NEXT_CURSOR`].
struct Scenario {
    source: Source,
    name: &'static str,
    world: &'static str,
    query: &'static str,
    follows: Option<&'static str>,
}

#[derive(Debug, Default, Deserialize)]
struct OllamaWorld {
    search: BTreeMap<String, String>,
    manifests: BTreeMap<String, Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct HuggingFacePage {
    models: Vec<Value>,
    next: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct HuggingFaceWorld {
    status: Option<u16>,
    #[serde(default)]
    pages: BTreeMap<String, HuggingFacePage>,
    #[serde(default)]
    adapters: BTreeMap<String, Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct ComfyWorld {
    lora: String,
    weight: String,
    sidecar: WeightSidecar,
    modified_at: u64,
}

#[derive(Default)]
struct Upstream {
    origin: String,
    ollama: OllamaWorld,
    huggingface: HuggingFaceWorld,
    gpt4all: Option<Value>,
    openrouter: Option<Value>,
    unanswered: Mutex<Vec<String>>,
}

impl Upstream {
    fn refuse(&self, request: String) -> Response {
        self.unanswered
            .lock()
            .expect("unanswered requests")
            .push(request);
        StatusCode::NOT_FOUND.into_response()
    }

    fn huggingface_catalog(&self) -> String {
        format!("{}/api/models", self.origin)
    }

    fn gpt4all_catalog(&self) -> String {
        format!("{}/models3.json", self.origin)
    }

    fn endpoints(&self) -> CatalogEndpoints {
        CatalogEndpoints {
            ollama_search: format!("{}/search", self.origin),
            ollama_registry: format!("{}/v2", self.origin),
            openrouter: format!("{}/api/v1/models", self.origin),
        }
    }
}

fn read_input(source: Source, file: &str) -> String {
    let path = source.directory().join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()))
}

fn read_world<T: for<'de> Deserialize<'de>>(source: Source, world: &str) -> T {
    let file = format!("{world}.input.json");
    serde_json::from_str(&read_input(source, &file))
        .unwrap_or_else(|error| panic!("{file} is not a {source:?} world: {error}"))
}

async fn search_ollama(
    State(upstream): State<Arc<Upstream>>,
    Query(parameters): Query<BTreeMap<String, String>>,
) -> Response {
    let key = ["q", "c"]
        .into_iter()
        .filter_map(|name| parameters.get(name).map(|value| format!("{name}={value}")))
        .collect::<Vec<_>>()
        .join("&");
    match upstream.ollama.search.get(&key) {
        Some(file) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            read_input(Source::Ollama, file),
        )
            .into_response(),
        None => upstream.refuse(format!("ollama search {key:?}")),
    }
}

async fn ollama_manifest(
    State(upstream): State<Arc<Upstream>>,
    RoutePath(path): RoutePath<String>,
) -> Response {
    let Some((repository, tag)) = path.rsplit_once(MANIFESTS) else {
        return upstream.refuse(format!("ollama registry {path:?}"));
    };
    let tag = if tag.is_empty() { DEFAULT_TAG } else { tag };
    let Some(sizes) = upstream
        .ollama
        .manifests
        .get(&format!("{repository}:{tag}"))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let layers: Vec<Value> = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            json!({
                "mediaType": LAYER_MEDIA_TYPE,
                "digest": format!("sha256:{index:064x}"),
                "size": size,
            })
        })
        .collect();
    (
        [(header::CONTENT_TYPE, MANIFEST_MEDIA_TYPE)],
        Json(json!({
            "schemaVersion": 2,
            "mediaType": MANIFEST_MEDIA_TYPE,
            "config": {
                "mediaType": CONFIG_MEDIA_TYPE,
                "digest": format!("sha256:{:064x}", sizes.len()),
                "size": MANIFEST_CONFIG_SIZE,
            },
            "layers": layers,
        })),
    )
        .into_response()
}

async fn search_huggingface(
    State(upstream): State<Arc<Upstream>>,
    Query(parameters): Query<Vec<(String, String)>>,
) -> Response {
    let world = &upstream.huggingface;
    if let Some(status) = world.status {
        return StatusCode::from_u16(status)
            .expect("a recorded status")
            .into_response();
    }
    let adapter_base = parameters
        .iter()
        .filter(|(name, _)| name == "filter")
        .find_map(|(_, value)| value.strip_prefix(ADAPTER_FILTER));
    if let Some(base) = adapter_base {
        let adapters = world.adapters.get(base).cloned().unwrap_or_default();
        return Json(adapters).into_response();
    }
    let cursor = parameters
        .iter()
        .find_map(|(name, value)| (name == "cursor").then_some(value.as_str()))
        .unwrap_or_default();
    let Some(page) = world.pages.get(cursor) else {
        return upstream.refuse(format!("huggingface cursor {cursor:?}"));
    };
    let mut response = Json(page.models.clone()).into_response();
    if let Some(next) = &page.next {
        let link = format!(
            "<{}?cursor={next}>; rel=\"next\"",
            upstream.huggingface_catalog()
        );
        response.headers_mut().insert(
            header::LINK,
            HeaderValue::from_str(&link).expect("a Link header"),
        );
    }
    response
}

async fn gpt4all_catalog(State(upstream): State<Arc<Upstream>>) -> Response {
    match &upstream.gpt4all {
        Some(catalog) => Json(catalog.clone()).into_response(),
        None => upstream.refuse("gpt4all catalog".to_string()),
    }
}

async fn openrouter_catalog(State(upstream): State<Arc<Upstream>>) -> Response {
    match &upstream.openrouter {
        Some(catalog) => Json(catalog.clone()).into_response(),
        None => upstream.refuse("openrouter catalog".to_string()),
    }
}

async fn start_upstream(scenario: &Scenario) -> Arc<Upstream> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("a bound address");
    let mut upstream = Upstream {
        origin: format!("http://{address}"),
        ..Upstream::default()
    };
    match scenario.source {
        Source::Comfy => {}
        Source::Gpt4All => upstream.gpt4all = Some(read_world(Source::Gpt4All, scenario.world)),
        Source::HuggingFace => {
            upstream.huggingface = read_world(Source::HuggingFace, scenario.world)
        }
        Source::Ollama => upstream.ollama = read_world(Source::Ollama, scenario.world),
        Source::OpenRouter => {
            upstream.openrouter = Some(read_world(Source::OpenRouter, scenario.world))
        }
    }
    let upstream = Arc::new(upstream);
    let router = Router::new()
        .route("/search", get(search_ollama))
        .route("/v2/{*path}", get(ollama_manifest))
        .route("/api/models", get(search_huggingface))
        .route("/models3.json", get(gpt4all_catalog))
        .route("/api/v1/models", get(openrouter_catalog))
        .with_state(upstream.clone());
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the upstream mock serves");
    });
    upstream
}

fn seed_comfy(world: &ComfyWorld, models: &Path, contract: &Contract) {
    let weight = models.join(&world.lora);
    std::fs::create_dir_all(weight.parent().expect("a weight directory"))
        .expect("a LoRA directory");
    std::fs::write(&weight, &world.weight).expect("a LoRA weight");
    inventory::write_sidecar(&weight, &world.sidecar, contract).expect("a LoRA sidecar");
    std::fs::File::options()
        .write(true)
        .open(&weight)
        .and_then(|file| {
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(world.modified_at))
        })
        .expect("a fixed modification time");
}

fn scenario_config(upstream: &Upstream, models: &Path) -> Config {
    let mut config = Config {
        huggingface_models_url: upstream.huggingface_catalog(),
        gpt4all_models_url: upstream.gpt4all_catalog(),
        model_search_proxy_url: None,
        ..test_config()
    };
    config.comfyui.models_directory = models.to_path_buf();
    config
}

fn scenario_state(config: Config) -> AppState {
    let database = PgPool::connect_lazy(LAZY_DATABASE).expect("a lazy pool needs no server");
    let state = AppState::new(config, database, None);
    state.disable_mcp();
    state
}

fn viewer() -> AuthUser {
    AuthUser(Claims {
        sub: uuid::Uuid::nil().to_string(),
        email: "viewer@example.com".to_string(),
        roles: Vec::new(),
        permissions: Vec::new(),
        exp: i64::MAX,
        iat: 0,
        jti: uuid::Uuid::nil().to_string(),
        is_admin: false,
    })
}

fn parse_query(source: Source, query: &str) -> ListModelsQuery {
    let separator = if query.is_empty() { "" } else { "&" };
    let uri: Uri = format!("/api/models?source={}{separator}{query}", source.name())
        .parse()
        .expect("a request URI");
    let Query(parsed) = Query::<ListModelsQuery>::try_from_uri(&uri)
        .unwrap_or_else(|error| panic!("{query:?} is not a models query: {error}"));
    parsed
}

async fn list_response(state: &AppState, source: Source, query: &str) -> Value {
    let response = list(
        State(state.clone()),
        viewer(),
        Query(parse_query(source, query)),
    )
    .await
    .into_response();
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a buffered body");
    let body: Value = serde_json::from_slice(&body).expect("a JSON body");
    json!({ "status": status, "body": body })
}

async fn browse_response(state: &AppState, source: Source, query: &str) -> Value {
    match browse_models(state, source.name(), parse_query(source, query)).await {
        Ok(response) => json!({ "ok": response }),
        Err(message) => json!({ "error": message }),
    }
}

async fn run(scenario: &Scenario) -> Value {
    let upstream = start_upstream(scenario).await;
    let models = tempfile::tempdir().expect("a scratch models directory");
    let config = scenario_config(&upstream, models.path());
    if let Source::Comfy = scenario.source {
        let world = read_world(Source::Comfy, scenario.world);
        seed_comfy(&world, models.path(), &config.comfyui.contract);
    }
    let state = scenario_state(config);
    let snapshot = ENDPOINTS
        .scope(upstream.endpoints(), async {
            let mut query = scenario.query.to_string();
            if let Some(previous) = scenario.follows {
                let first = list_response(&state, scenario.source, previous).await;
                let cursor = first["body"]["next_cursor"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{previous:?} has no next page: {first}"));
                query = query.replace(NEXT_CURSOR, &urlencoding::encode(cursor));
            }
            json!({
                "list": list_response(&state, scenario.source, &query).await,
                "browse": browse_response(&state, scenario.source, &query).await,
            })
        })
        .await;
    let unanswered = upstream
        .unanswered
        .lock()
        .expect("unanswered requests")
        .clone();
    assert!(
        unanswered.is_empty(),
        "{}: requests no fixture answers: {unanswered:?}",
        scenario.name
    );
    snapshot
}

/// Whether this run records snapshots instead of checking them. CI only ever
/// checks, so a recording run there fails rather than passing on whatever
/// the code now answers.
fn records(is_set: impl Fn(&str) -> bool) -> bool {
    let recording = is_set(RECORD_ENVIRONMENT);
    assert!(
        !(recording && is_set(CI_ENVIRONMENT)),
        "{RECORD_ENVIRONMENT} is set under {CI_ENVIRONMENT}; record snapshots locally"
    );
    recording
}

#[test]
fn snapshots_are_recorded_only_outside_ci() {
    assert!(!records(|_| false));
    assert!(records(|name| name == RECORD_ENVIRONMENT));
    assert!(!records(|name| name == CI_ENVIRONMENT));
}

#[test]
#[should_panic(expected = "record snapshots locally")]
fn recording_under_ci_fails() {
    records(|_| true);
}

async fn check(scenario: Scenario) {
    let actual = run(&scenario).await;
    let path = scenario
        .source
        .directory()
        .join(format!("{}.expected.json", scenario.name));
    if records(|name| std::env::var_os(name).is_some()) {
        let pretty = serde_json::to_string_pretty(&actual).expect("a snapshot");
        std::fs::write(&path, format!("{pretty}\n")).expect("a recorded snapshot");
        return;
    }
    let recorded = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}); record it with {RECORD_ENVIRONMENT}=1",
            path.display()
        )
    });
    let expected: Value = serde_json::from_str(&recorded).expect("a recorded snapshot");
    assert_eq!(
        actual,
        expected,
        "{} no longer matches {}",
        scenario.name,
        path.display()
    );
}

macro_rules! snapshots {
    ($($test:ident: $source:ident, $name:literal, $world:literal, $query:literal $(, follows $follows:literal)?;)+) => {
        $(
            #[tokio::test]
            async fn $test() {
                check(Scenario {
                    source: Source::$source,
                    name: $name,
                    world: $world,
                    query: $query,
                    follows: None $(.or(Some($follows)))?,
                })
                .await;
            }
        )+
    };
}

snapshots! {
    ollama_default: Ollama, "default", "library", "";
    ollama_query: Ollama, "query-qwen", "library", "q=qwen";
    ollama_family: Ollama, "family-llama", "library", "family=llama";
    ollama_medium: Ollama, "medium-tools", "library", "medium=tools";
    ollama_first_page: Ollama, "limit-5", "library", "limit=5";
    ollama_offset_cursor: Ollama, "limit-5-offset-5", "library", "limit=5&cursor=offset:5";
    ollama_page_cursor: Ollama, "d2-page-2-limit-5", "library", "cursor=page:2&limit=5";
    ollama_zero_limit: Ollama, "d4-limit-0", "library", "limit=0";
    ollama_community_card: Ollama, "d10-community-card", "library", "q=coder";
    popular_default: Ollama, "d1-popular-default", "popular", "limit=20";
    popular_first_page: Ollama, "d1-popular-limit-5", "popular", "limit=5";
    popular_offset_cursor: Ollama, "d1-popular-limit-5-offset-5", "popular", "limit=5&cursor=offset:5";
    popular_query: Ollama, "d1-popular-query-nothing", "popular", "q=zzz-nothing";
    popular_family: Ollama, "d1-popular-family-llama", "popular", "family=llama";
    popular_size_ascending: Ollama, "d1-popular-sort-size-ascending", "popular", "sort=size_asc";
    popular_size_descending: Ollama, "d1-popular-sort-size-descending", "popular", "sort=size_desc&limit=5";
    popular_extra_large: Ollama, "d1-popular-size-xl", "popular", "size=xl";
    popular_small_by_name: Ollama, "d1-popular-small-name-descending", "popular", "size=small&sort=name_desc";
    popular_parameters: Ollama, "d1-popular-sort-parameters-descending", "popular", "sort=params_desc&limit=5";
    popular_embeddings: Ollama, "d1-popular-medium-embeddings", "popular", "medium=embeddings";
    huggingface_default: HuggingFace, "default", "catalog", "";
    huggingface_window: HuggingFace, "window-size-small", "catalog", "size=small&limit=2";
    huggingface_image_generation: HuggingFace, "medium-image-generation", "catalog", "medium=image_generation";
    huggingface_query_first_page: HuggingFace, "d18-query-llama-page-1", "catalog", "medium=all&q=llama";
    huggingface_query_second_page: HuggingFace, "d18-query-llama-page-2", "catalog", "medium=all&q=llama&cursor={next_cursor}", follows "medium=all&q=llama";
    huggingface_quantizations: HuggingFace, "d6-quantizations", "quantizations", "";
    huggingface_unavailable: HuggingFace, "d5-unavailable", "unavailable", "";
    gpt4all_default: Gpt4All, "default", "catalog", "";
    gpt4all_refined: Gpt4All, "refined", "catalog", "sort=name_asc&family=llama&size=medium&medium=text";
    openrouter_default: OpenRouter, "default", "catalog", "";
    openrouter_image: OpenRouter, "medium-image", "catalog", "medium=image";
    comfy_default: Comfy, "default", "default", "";
}
