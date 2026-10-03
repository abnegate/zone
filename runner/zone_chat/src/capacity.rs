//! Effective limits come from the selected deployment, never model-name guesses.
//!
//! Cold Ollama uses the advertised native window from `/api/show`. A loaded
//! `/api/ps` runtime still wins, and an explicit LiteLLM route `num_ctx` is an
//! operator request bounded by that native window. `ZONE_CHAT_CONTEXT_TOKENS`
//! is fallback only when native capacity is unknown — never a ceiling on a
//! larger advertised window.
//!
//! LiteLLM 1.99.1 accepts top-level `num_ctx` and forwards it to Ollama's
//! `options.num_ctx` (llms/ollama/chat/transformation.py). Ordinary inference
//! and summarization must both apply the returned setting to this exact alias.
//! https://docs.litellm.ai/docs/proxy/model_management
//! https://docs.ollama.com/api/ps

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Fallback allocation when a deployment has not reported native capacity.
/// Production callers supply validated typed configuration through `with_context`.
pub const DEFAULT_CONTEXT: u64 = 32_768;

const THINKING: &str = "thinking";
const UNDISCLOSED: &str = "The endpoint publishes no deployment metadata.";
const ASSUMED: &str =
    "The endpoint publishes no deployment metadata, so the configured context is assumed.";
const VISION: &str = "vision";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Runtime,
    Configured,
    Provider,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Capacity {
    pub limit: Option<u64>,
    pub source: Source,
    /// Apply only to requests for the alias used to resolve this capacity.
    pub ollama: Option<u64>,
    /// Engine or provider advertised thinking / extended reasoning.
    pub reasoning: bool,
    /// Whether the engine declared it can read images; `None` when it has not said.
    pub vision: Option<bool>,
    pub reason: Option<String>,
    pub identity: String,
}

impl Capacity {
    fn unknown(model: &str, reason: &str) -> Self {
        Self {
            limit: None,
            source: Source::Unknown,
            ollama: None,
            reasoning: false,
            vision: None,
            reason: Some(reason.into()),
            identity: model.into(),
        }
    }

    /// Operator-chosen window for this chat. Never larger than the resolved
    /// native or runtime bound. Third-party endpoints still never receive
    /// `num_ctx`.
    pub fn with_request(self, requested: Option<u64>) -> Self {
        let Some(requested) = requested.filter(|value| *value > 0) else {
            return self;
        };
        let Some(ceiling) = self.limit else {
            return Self {
                limit: Some(requested),
                source: Source::Configured,
                ollama: self.ollama.map(|_| requested),
                reason: None,
                ..self
            };
        };
        let limit = requested.min(ceiling);
        Self {
            limit: Some(limit),
            source: Source::Configured,
            ollama: self.ollama.map(|_| limit),
            reason: (limit < requested).then(|| {
                "The requested context allocation is bounded by the model's reported native capacity."
                    .into()
            }),
            ..self
        }
    }
}

/// Refreshed per preparation: no stale cross-provider or unloaded-runtime cache.
pub struct Resolver {
    configured: Option<u64>,
    deployment: Option<Deployment>,
}

/// A LiteLLM deployment of this instance and the Ollama behind it.
struct Deployment {
    client: Client,
    host: String,
    key: String,
    ollama: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct Parameters {
    model: String,
    #[serde(default)]
    api_base: Option<String>,
    #[serde(default)]
    num_ctx: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Route {
    model_name: String,
    litellm_params: Parameters,
    #[serde(default)]
    model_info: Value,
}

#[derive(Deserialize)]
struct Routes {
    data: Vec<Route>,
    #[serde(default)]
    total_pages: Option<u64>,
}

impl Resolver {
    pub fn new(host: &str, key: &str, ollama: &str) -> Self {
        Self::with_context(host, key, ollama, Some(DEFAULT_CONTEXT))
    }

    pub fn with_context(host: &str, key: &str, ollama: &str, configured: Option<u64>) -> Self {
        Self {
            configured: configured.filter(|value| *value > 0),
            deployment: Some(Deployment {
                client: Client::builder()
                    .connect_timeout(Duration::from_secs(2))
                    .timeout(Duration::from_secs(2))
                    .build()
                    .expect("Valid metadata HTTP client"),
                host: host.trim_end_matches('/').into(),
                key: key.into(),
                ollama: ollama.trim_end_matches('/').into(),
            }),
        }
    }

    /// For an endpoint that is not a LiteLLM deployment of this instance.
    /// Nothing is requested to learn a model's capacity: every model is given
    /// `configured`, and no Ollama `num_ctx` is ever set for it.
    pub fn undisclosed(configured: Option<u64>) -> Self {
        Self {
            configured: configured.filter(|value| *value > 0),
            deployment: None,
        }
    }

    pub async fn resolve(&self, model: &str) -> Capacity {
        match &self.deployment {
            Some(deployment) => deployment.resolve(model, self.configured).await,
            None => assumed(model, self.configured),
        }
    }
}

fn assumed(model: &str, configured: Option<u64>) -> Capacity {
    let Some(limit) = configured else {
        return Capacity::unknown(model, UNDISCLOSED);
    };
    Capacity {
        limit: Some(limit),
        source: Source::Configured,
        ollama: None,
        reasoning: false,
        vision: None,
        reason: Some(ASSUMED.into()),
        identity: model.into(),
    }
}

impl Deployment {
    async fn resolve(&self, model: &str, configured: Option<u64>) -> Capacity {
        let Some(mut routes) = self.routes(model).await else {
            return Capacity::unknown(
                model,
                "The selected model's deployment metadata is unavailable.",
            );
        };
        if routes.is_empty() {
            let Some(wildcard) = self.routes("*").await else {
                return Capacity::unknown(
                    model,
                    "The wildcard deployment metadata is unavailable.",
                );
            };
            routes = wildcard;
        }
        let route = match select(&routes, model) {
            Ok(route) => route,
            Err(reason) => return Capacity::unknown(model, reason),
        };
        let identity = format!(
            "{}|{}|{}",
            model,
            route.litellm_params.model,
            route.litellm_params.api_base.as_deref().unwrap_or("")
        );
        let native = route
            .litellm_params
            .model
            .strip_prefix("ollama_chat/")
            .or_else(|| route.litellm_params.model.strip_prefix("ollama/"));
        let Some(native) = native else {
            let limit = route
                .model_info
                .get("max_input_tokens")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0);
            return Capacity {
                limit,
                source: if limit.is_some() {
                    Source::Provider
                } else {
                    Source::Unknown
                },
                ollama: None,
                reasoning: provider_reasoning(&route.model_info),
                vision: None,
                reason: limit.is_none().then(|| {
                    "The selected provider has not reported an input context limit.".into()
                }),
                identity,
            };
        };
        if route
            .litellm_params
            .api_base
            .as_deref()
            .map(|base| base.trim_end_matches('/'))
            != Some(self.ollama.as_str())
        {
            return Capacity::unknown(
                model,
                "The model's Ollama deployment does not match the configured metadata endpoint.",
            );
        }
        if native.is_empty() || native.contains('*') {
            return Capacity::unknown(
                model,
                "The selected model's native deployment could not be resolved.",
            );
        }
        let (running, shown) = tokio::join!(self.running(), self.show(native));
        let runtime = running
            .as_ref()
            .and_then(|body| body.get("models"))
            .and_then(Value::as_array)
            .and_then(|models| {
                models.iter().find(|entry| {
                    ["name", "model"]
                        .into_iter()
                        .filter_map(|key| entry.get(key).and_then(Value::as_str))
                        .any(|name| normalize(name) == normalize(native))
                })
            })
            .and_then(|entry| entry.get("context_length"))
            .and_then(Value::as_u64)
            .filter(|value| *value > 0);
        let advertised = shown.as_ref().and_then(native_limit);
        let requested = route.litellm_params.num_ctx.filter(|value| *value > 0);
        let reasoning = shown.as_ref().is_some_and(ollama_thinking);
        let vision = shown.as_ref().and_then(ollama_vision);
        if let Some(runtime) = runtime {
            let limit = advertised.map_or(runtime, |advertised| runtime.min(advertised));
            return Capacity {
                limit: Some(limit),
                source: if limit == runtime {
                    Source::Runtime
                } else {
                    Source::Configured
                },
                ollama: Some(limit),
                reasoning,
                vision,
                reason: (limit != runtime).then(|| {
                    "The loaded context exceeds native capacity; this request uses the supported bound.".into()
                }),
                identity,
            };
        }
        if let Some(advertised) = advertised {
            if let Some(requested) = requested {
                let limit = requested.min(advertised);
                return Capacity {
                    limit: Some(limit),
                    source: Source::Configured,
                    ollama: Some(limit),
                    reasoning,
                    vision,
                    reason: (limit < requested).then(|| {
                        "The requested context allocation is bounded by the model's reported native capacity.".into()
                    }),
                    identity,
                };
            }
            return Capacity {
                limit: Some(advertised),
                source: Source::Provider,
                ollama: Some(advertised),
                reasoning,
                vision,
                reason: None,
                identity,
            };
        }
        let Some(limit) = requested
            .or_else(|| {
                shown
                    .as_ref()
                    .and_then(|body| body.get("parameters"))
                    .and_then(Value::as_str)
                    .and_then(configured_limit)
            })
            .or(configured)
        else {
            return Capacity::unknown(
                model,
                "The cold model has not reported a native capacity that can safely bound its runtime configuration.",
            );
        };
        Capacity {
            limit: Some(limit),
            source: Source::Configured,
            ollama: Some(limit),
            reasoning,
            vision,
            reason: None,
            identity,
        }
    }

    async fn routes(&self, model: &str) -> Option<Vec<Route>> {
        // v1 expands wildcard deployments to example models, losing route identity.
        // Installed 1.99.1 v2 returns the actual deployment for this filter.
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut page = 1_u64;
            let mut routes = Vec::new();
            loop {
                let body = self
                    .client
                    .get(format!(
                        "{}/v2/model/info?model={}&page={}",
                        self.host,
                        urlencoding::encode(model),
                        page
                    ))
                    .bearer_auth(&self.key)
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?
                    .json::<Routes>()
                    .await
                    .ok()?;
                let pages = body.total_pages.unwrap_or(1);
                routes.extend(
                    body.data
                        .into_iter()
                        .filter(|route| route.model_name == model),
                );
                if page >= pages {
                    return Some(routes);
                }
                page = page.checked_add(1)?;
            }
        })
        .await
        .ok()
        .flatten()
    }

    async fn running(&self) -> Option<Value> {
        self.client
            .get(format!("{}/api/ps", self.ollama))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()
    }

    async fn show(&self, model: &str) -> Option<Value> {
        self.client
            .post(format!("{}/api/show", self.ollama))
            .json(&serde_json::json!({"model": model}))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()
    }
}

fn select(routes: &[Route], model: &str) -> Result<Route, &'static str> {
    let exact: Vec<&Route> = routes
        .iter()
        .filter(|route| route.model_name == model)
        .collect();
    let matches = if exact.is_empty() {
        routes
            .iter()
            .filter(|route| route.model_name == "*")
            .collect::<Vec<_>>()
    } else {
        exact
    };
    let Some(first) = matches.first() else {
        return Err("The selected alias has no verified deployment route.");
    };
    if matches.iter().any(|route| {
        route.litellm_params != first.litellm_params
            || route.model_info.get("max_input_tokens") != first.model_info.get("max_input_tokens")
    }) {
        return Err("The selected alias routes to deployments with different context settings.");
    }
    let mut route = (*first).clone();
    if route.model_name == "*" {
        // The configured simple catch-all substitutes the entire submitted alias.
        if !matches!(
            route.litellm_params.model.as_str(),
            "ollama_chat/*" | "ollama/*"
        ) {
            return Err("The wildcard deployment does not expose a verifiable native model.");
        }
        route.litellm_params.model = route.litellm_params.model.replace('*', model);
    }
    Ok(route)
}

fn ollama_capability(shown: &Value, name: &str) -> Option<bool> {
    shown
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some(name))
        })
}

fn ollama_thinking(shown: &Value) -> bool {
    ollama_capability(shown, THINKING).unwrap_or(false)
}

fn ollama_vision(shown: &Value) -> Option<bool> {
    ollama_capability(shown, VISION)
}

fn provider_reasoning(info: &Value) -> bool {
    if info.get("supports_reasoning").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    info.get("supported_openai_params")
        .and_then(Value::as_array)
        .is_some_and(|params| {
            params.iter().any(|param| {
                matches!(
                    param.as_str(),
                    Some("reasoning_effort" | "thinking" | "reasoning")
                )
            })
        })
}

fn native_limit(body: &Value) -> Option<u64> {
    let metadata = body.get("model_info")?;
    let architecture = metadata.get("general.architecture")?.as_str()?;
    metadata
        .get(format!("{architecture}.context_length"))?
        .as_u64()
        .filter(|value| *value > 0)
}

fn configured_limit(parameters: &str) -> Option<u64> {
    parameters.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next()? == "num_ctx")
            .then(|| words.next()?.parse::<u64>().ok().filter(|value| *value > 0))
            .flatten()
    })
}

fn normalize(name: &str) -> String {
    if name
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains(':'))
    {
        name.into()
    } else {
        format!("{name}:latest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn an_undisclosed_endpoint_is_given_the_configured_context_without_a_num_ctx() {
        let capacity = Resolver::undisclosed(Some(DEFAULT_CONTEXT))
            .resolve("gpt-4o")
            .await;

        assert_eq!(capacity.limit, Some(DEFAULT_CONTEXT));
        assert_eq!(capacity.source, Source::Configured);
        assert_eq!(capacity.ollama, None, "no num_ctx may reach a third party");
        assert!(!capacity.reasoning);
        assert_eq!(capacity.vision, None);
        assert_eq!(capacity.identity, "gpt-4o");
    }

    #[tokio::test]
    async fn an_undisclosed_endpoint_without_a_configured_context_is_unknown() {
        for configured in [None, Some(0)] {
            let capacity = Resolver::undisclosed(configured).resolve("gpt-4o").await;

            assert_eq!(capacity.source, Source::Unknown, "{configured:?}");
            assert_eq!(capacity.limit, None, "{configured:?}");
            assert_eq!(capacity.ollama, None, "{configured:?}");
        }
    }

    async fn fixture(parameters: Value, running: Value, shown: Value) -> (MockServer, Resolver) {
        let server = MockServer::start().await;
        let mut parameters = parameters;
        parameters["api_base"] = json!(server.uri());
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data":[{"model_name":"alias","litellm_params":parameters}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(running))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .and(body_json(json!({"model":"native:latest"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(shown))
            .mount(&server)
            .await;
        let resolver =
            Resolver::with_context(&server.uri(), "key", &server.uri(), Some(DEFAULT_CONTEXT));
        (server, resolver)
    }

    #[tokio::test]
    async fn cold_model_uses_explicit_bounded_context_without_loaded_runtime() {
        let (_server,resolver) = fixture(json!({"model":"ollama_chat/native:latest"}), json!({"models":[]}),json!({"parameters":"temperature 0.7", "model_info":{"general.architecture":"qwen3","qwen3.context_length":16384}})).await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(16384));
        assert_eq!(capacity.ollama, Some(16384));
        assert_eq!(capacity.source, Source::Provider);
    }

    #[tokio::test]
    async fn cold_model_uses_advertised_native_window_instead_of_default_clamp() {
        let (_server, resolver) = fixture(
            json!({"model": "ollama_chat/native:latest"}),
            json!({"models": []}),
            json!({"model_info": {"general.architecture": "qwen3", "qwen3.context_length": 262144}}),
        )
        .await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(262144));
        assert_eq!(capacity.ollama, Some(262144));
        assert_eq!(capacity.source, Source::Provider);
    }

    #[tokio::test]
    async fn explicit_route_num_ctx_is_an_operator_request_bounded_by_native() {
        let (_server, resolver) = fixture(
            json!({"model": "ollama_chat/native:latest", "num_ctx": 8192}),
            json!({"models": []}),
            json!({"model_info": {"general.architecture": "qwen3", "qwen3.context_length": 65536}}),
        )
        .await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(8192));
        assert_eq!(capacity.ollama, Some(8192));
        assert_eq!(capacity.source, Source::Configured);
    }

    #[tokio::test]
    async fn explicit_route_num_ctx_never_exceeds_native() {
        let (_server, resolver) = fixture(
            json!({"model": "ollama_chat/native:latest", "num_ctx": 131072}),
            json!({"models": []}),
            json!({"model_info": {"general.architecture": "qwen3", "qwen3.context_length": 65536}}),
        )
        .await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(65536));
        assert_eq!(capacity.ollama, Some(65536));
        assert_eq!(capacity.source, Source::Configured);
        assert!(capacity.reason.is_some());
    }

    #[tokio::test]
    async fn show_parameter_num_ctx_does_not_shrink_advertised_native() {
        let (_server, resolver) = fixture(
            json!({"model": "ollama_chat/native:latest"}),
            json!({"models": []}),
            json!({
                "parameters": "temperature 0.7\nnum_ctx 4096",
                "model_info": {"general.architecture": "qwen3", "qwen3.context_length": 262144}
            }),
        )
        .await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(262144));
        assert_eq!(capacity.ollama, Some(262144));
        assert_eq!(capacity.source, Source::Provider);
    }

    #[tokio::test]
    async fn unknown_native_capacity_uses_configured_fallback() {
        let (_server, resolver) = fixture(
            json!({"model": "ollama_chat/native:latest"}),
            json!({"models": []}),
            json!({"parameters": "temperature 0.7"}),
        )
        .await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(DEFAULT_CONTEXT));
        assert_eq!(capacity.ollama, Some(DEFAULT_CONTEXT));
        assert_eq!(capacity.source, Source::Configured);
    }

    #[tokio::test]
    async fn unknown_native_capacity_without_fallback_stays_unknown() {
        let server = MockServer::start().await;
        let mut parameters = json!({"model": "ollama_chat/native:latest"});
        parameters["api_base"] = json!(server.uri());
        Mock::given(method("GET"))
            .and(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data":[{"model_name":"alias","litellm_params":parameters}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .and(body_json(json!({"model": "native:latest"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let resolver = Resolver::with_context(&server.uri(), "key", &server.uri(), None);
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, None);
        assert_eq!(capacity.ollama, None);
        assert_eq!(capacity.source, Source::Unknown);
    }

    #[tokio::test]
    async fn exact_runtime_wins_over_training_and_unrelated_loaded_models() {
        let (_server,resolver) = fixture(json!({"model":"ollama_chat/native:latest","num_ctx":32768}), json!({"models":[{"name":"other","context_length":131072},{"name":"native:latest","context_length":8192}]}),json!({"model_info":{"general.architecture":"qwen3","qwen3.context_length":32768}})).await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(8192));
        assert_eq!(capacity.ollama, Some(8192));
        assert_eq!(capacity.source, Source::Runtime);
    }

    #[tokio::test]
    async fn provider_alias_never_inherits_ollama_metadata_or_options() {
        let server = MockServer::start().await;
        Mock::given(path("/v2/model/info")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"model_name":"alias","litellm_params":{"model":"openai/remote"},"model_info":{"max_input_tokens":128000}}]}))).mount(&server).await;
        let resolver = Resolver::new(&server.uri(), "key", &server.uri());
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(128000));
        assert_eq!(capacity.ollama, None);
        assert_eq!(capacity.source, Source::Provider);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn provider_changes_invalidate_runtime_observation_immediately() {
        let (server, resolver) = fixture(
            json!({"model":"ollama_chat/native:latest"}),
            json!({"models":[{"name":"native:latest","context_length":8192}]}),
            json!({}),
        )
        .await;
        assert_eq!(resolver.resolve("alias").await.limit, Some(8192));
        server.reset().await;
        Mock::given(path("/v2/model/info")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"model_name":"alias","litellm_params":{"model":"openai/remote"},"model_info":{"max_input_tokens":64000}}]}))).mount(&server).await;
        let capacity = resolver.resolve("alias").await;
        assert_eq!(capacity.limit, Some(64000));
        assert_eq!(capacity.ollama, None);
    }

    #[test]
    fn wildcard_selection_does_not_override_exact_alias_or_guess_ambiguous_routes() {
        let routes: Routes = serde_json::from_value(json!({"data":[{"model_name":"*","litellm_params":{"model":"ollama_chat/*"}},{"model_name":"alias","litellm_params":{"model":"openai/remote"}}]})).unwrap();
        assert_eq!(
            select(&routes.data, "alias").unwrap().litellm_params.model,
            "openai/remote"
        );
        assert_eq!(
            select(&routes.data, "local:1b")
                .unwrap()
                .litellm_params
                .model,
            "ollama_chat/local:1b"
        );
        let mut duplicates = routes.data;
        duplicates.push(Route {
            model_name: "alias".into(),
            litellm_params: Parameters {
                model: "ollama_chat/unrelated".into(),
                api_base: None,
                num_ctx: None,
            },
            model_info: Value::Null,
        });
        assert!(select(&duplicates, "alias").is_err());
    }
    #[tokio::test]
    async fn wildcard_metadata_resolves_cold_nonpreset_model_without_v1_example_substitution() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        Mock::given(path("/v2/model/info"))
            .and(query_param("model", "custom:1b"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data":[],"total_pages":0})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/v2/model/info")).and(query_param("model","*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"model_name":"*","litellm_params":{"model":"ollama_chat/*","api_base":server.uri()}}],"total_pages":1}))).mount(&server).await;
        Mock::given(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models":[]})))
            .mount(&server)
            .await;
        Mock::given(path("/api/show")).and(body_json(json!({"model":"custom:1b"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"model_info":{"general.architecture":"custom","custom.context_length":65536}}))).mount(&server).await;
        let resolver = Resolver::with_context(&server.uri(), "key", &server.uri(), Some(32768));
        let capacity = resolver.resolve("custom:1b").await;
        assert_eq!(capacity.limit, Some(65536));
        assert_eq!(capacity.source, Source::Provider);
        assert_eq!(capacity.ollama, Some(65536));
        assert!(capacity.identity.contains("ollama_chat/custom:1b"));
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn ollama_thinking_capability_enables_reasoning() {
        let (_server, resolver) = fixture(
            json!({"model":"ollama_chat/native:latest"}),
            json!({"models":[]}),
            json!({
                "capabilities": ["completion", "thinking"],
                "model_info": {"general.architecture":"qwen3","qwen3.context_length":16384}
            }),
        )
        .await;
        assert!(resolver.resolve("alias").await.reasoning);
    }

    #[tokio::test]
    async fn provider_supports_reasoning_without_guessing_the_name() {
        let server = MockServer::start().await;
        Mock::given(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{
                    "model_name":"alias",
                    "litellm_params":{"model":"anthropic/claude-sonnet"},
                    "model_info":{"max_input_tokens":200000,"supports_reasoning":true}
                }]
            })))
            .mount(&server)
            .await;
        let resolver = Resolver::new(&server.uri(), "key", &server.uri());
        let capacity = resolver.resolve("alias").await;
        assert!(capacity.reasoning);
        assert_eq!(capacity.ollama, None);
    }

    #[tokio::test]
    async fn ollama_vision_capability_is_read_from_the_same_show_response() {
        let (server, resolver) = fixture(
            json!({"model":"ollama_chat/native:latest"}),
            json!({"models":[]}),
            json!({
                "capabilities": ["completion", "vision"],
                "model_info": {"general.architecture":"gemma3","gemma3.context_length":16384}
            }),
        )
        .await;
        assert_eq!(resolver.resolve("alias").await.vision, Some(true));
        let shows = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/api/show")
            .count();
        assert_eq!(shows, 1, "Vision must not cost a second /api/show");
    }

    #[tokio::test]
    async fn a_text_only_ollama_model_reports_no_vision() {
        let (_server, resolver) = fixture(
            json!({"model":"ollama_chat/native:latest"}),
            json!({"models":[]}),
            json!({
                "capabilities": ["completion", "tools"],
                "model_info": {"general.architecture":"qwen3","qwen3.context_length":16384}
            }),
        )
        .await;
        assert_eq!(resolver.resolve("alias").await.vision, Some(false));
    }

    #[tokio::test]
    async fn vision_is_unknown_without_capabilities() {
        let (_server, resolver) = fixture(
            json!({"model":"ollama_chat/native:latest"}),
            json!({"models":[]}),
            json!({"model_info": {"general.architecture":"qwen3","qwen3.context_length":16384}}),
        )
        .await;
        assert_eq!(resolver.resolve("alias").await.vision, None);
    }

    #[tokio::test]
    async fn vision_is_unknown_for_a_provider_endpoint() {
        let server = MockServer::start().await;
        Mock::given(path("/v2/model/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data":[{
                    "model_name":"alias",
                    "litellm_params":{"model":"openai/remote"},
                    "model_info":{"max_input_tokens":128000}
                }]
            })))
            .mount(&server)
            .await;
        let resolver = Resolver::new(&server.uri(), "key", &server.uri());
        assert_eq!(resolver.resolve("alias").await.vision, None);
    }

    #[test]
    fn vision_is_unknown_for_an_unknown_capacity() {
        assert_eq!(Capacity::unknown("auto", "unresolved").vision, None);
    }

    #[test]
    fn vision_is_only_the_engine_declared_capability() {
        assert_eq!(
            ollama_vision(&json!({"capabilities":["vision"]})),
            Some(true)
        );
        assert_eq!(
            ollama_vision(&json!({"capabilities":["completion"]})),
            Some(false)
        );
        assert_eq!(ollama_vision(&json!({"capabilities":null})), None);
        assert_eq!(ollama_vision(&json!({})), None);
    }

    #[test]
    fn thinking_is_only_the_engine_declared_capability() {
        assert!(ollama_thinking(&json!({"capabilities":["thinking"]})));
        assert!(!ollama_thinking(&json!({"capabilities":["completion"]})));
        assert!(!provider_reasoning(&json!({"supports_reasoning":false})));
        assert!(provider_reasoning(&json!({"supports_reasoning":true})));
        assert!(provider_reasoning(&json!({
            "supported_openai_params": ["temperature", "reasoning_effort"]
        })));
    }

    fn local(limit: u64) -> Capacity {
        Capacity {
            limit: Some(limit),
            source: Source::Provider,
            ollama: Some(limit),
            reasoning: false,
            vision: None,
            reason: None,
            identity: "qwen".into(),
        }
    }

    #[test]
    fn a_chosen_window_caps_ollama_below_native() {
        let capacity = local(262_144).with_request(Some(32_768));
        assert_eq!(capacity.limit, Some(32_768));
        assert_eq!(capacity.ollama, Some(32_768));
        assert_eq!(capacity.source, Source::Configured);
        assert!(capacity.reason.is_none());
    }

    #[test]
    fn a_chosen_window_never_exceeds_native() {
        let capacity = local(32_768).with_request(Some(262_144));
        assert_eq!(capacity.limit, Some(32_768));
        assert_eq!(capacity.ollama, Some(32_768));
        assert!(capacity.reason.is_some());
    }

    #[test]
    fn a_chosen_window_does_not_invent_ollama_options_for_a_third_party() {
        let capacity = Capacity {
            limit: Some(128_000),
            source: Source::Provider,
            ollama: None,
            reasoning: false,
            vision: None,
            reason: None,
            identity: "gpt".into(),
        }
        .with_request(Some(8_192));
        assert_eq!(capacity.limit, Some(8_192));
        assert_eq!(capacity.ollama, None);
    }

    #[test]
    fn omitting_a_choice_leaves_resolved_capacity() {
        let capacity = local(262_144).with_request(None);
        assert_eq!(capacity.limit, Some(262_144));
        assert_eq!(capacity.ollama, Some(262_144));
        assert_eq!(capacity.source, Source::Provider);
    }
}
