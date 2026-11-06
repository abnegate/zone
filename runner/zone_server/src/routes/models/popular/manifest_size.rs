use std::time::Duration;

use abnegate_llm::catalog::CatalogError;
use abnegate_llm::catalog::ModelEntry;
use futures::stream;
use futures::stream::StreamExt;
use reqwest::Client;
use reqwest::Proxy;
use reqwest::header::ACCEPT;
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POOL_MAXIMUM_IDLE_PER_HOST: usize = 10;
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const USER_AGENT: &str = "ZoneManager/1.0";
const LOOKUP_CONCURRENCY: usize = 8;
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";
const OFFICIAL_NAMESPACE: &str = "library";
const DEFAULT_TAG: &str = "latest";

/// The client Zone has always looked popular models' sizes up with.
pub(super) fn client(proxy_url: Option<&str>) -> Result<Client, CatalogError> {
    let mut builder = Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_max_idle_per_host(POOL_MAXIMUM_IDLE_PER_HOST)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .user_agent(USER_AGENT);
    if let Some(proxy_url) = proxy_url {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }
    Ok(builder.build()?)
}

/// `models` with each download size the registry publishes; a model whose
/// manifest cannot be read keeps no size rather than failing the listing.
pub(super) async fn attach(
    models: Vec<ModelEntry>,
    client: &Client,
    registry_url: &str,
) -> Vec<ModelEntry> {
    stream::iter(models)
        .map(|mut model| async move {
            model.size = fetch(&model.name, client, registry_url).await;
            if let Some(sizes) = model.sizes.as_mut() {
                for variant in sizes.iter_mut() {
                    variant.size = fetch(&variant.name, client, registry_url).await;
                }
            }
            model
        })
        .buffered(LOOKUP_CONCURRENCY)
        .collect()
        .await
}

/// The sum of the layer sizes in `name`'s manifest.
async fn fetch(name: &str, client: &Client, registry_url: &str) -> Option<u64> {
    let (repository, tag) = reference(name);
    let url = format!("{registry_url}/{OFFICIAL_NAMESPACE}/{repository}/manifests/{tag}");
    let response = client
        .get(&url)
        .header(ACCEPT, MANIFEST_MEDIA_TYPE)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let manifest: Value = response.json().await.ok()?;
    let total: u64 = manifest
        .get("layers")?
        .as_array()?
        .iter()
        .filter_map(|layer| layer.get("size")?.as_u64())
        .sum();
    (total > 0).then_some(total)
}

/// `llama3.2:1b` as repository and tag; an untagged name is `latest`.
fn reference(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((repository, tag)) if !repository.is_empty() && !tag.is_empty() => (repository, tag),
        _ => (name, DEFAULT_TAG),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use axum::Router;
    use axum::http::HeaderMap;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[test]
    fn a_reference_defaults_its_tag() {
        assert_eq!(reference("llama3.2:1b"), ("llama3.2", "1b"));
        assert_eq!(reference("mistral"), ("mistral", "latest"));
        assert_eq!(reference("mistral:"), ("mistral:", "latest"));
        assert_eq!(reference(":7b"), (":7b", "latest"));
    }

    #[test]
    fn a_malformed_proxy_is_refused() {
        assert!(client(Some("not a url")).is_err());
        assert!(client(Some("http://127.0.0.1:3128")).is_ok());
    }

    #[tokio::test]
    async fn sizes_sum_layers_only_and_unreadable_manifests_stay_unsized() {
        let agents = Arc::new(Mutex::new(Vec::new()));
        let seen = agents.clone();
        let router = Router::new()
            .route(
                "/v2/library/mistral/manifests/latest",
                get(move |headers: HeaderMap| {
                    let seen = seen.clone();
                    async move {
                        seen.lock().expect("agents").push((
                            headers.get("user-agent").cloned(),
                            headers.get("accept").cloned(),
                        ));
                        Json(json!({
                            "config": {"size": 487},
                            "layers": [{"size": 4_000}, {"size": 2}],
                        }))
                    }
                }),
            )
            .route(
                "/v2/library/empty/manifests/latest",
                get(|| async { Json(json!({"layers": []})) }),
            )
            .route(
                "/v2/library/broken/manifests/latest",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR.into_response() }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let registry = format!("http://{}/v2", listener.local_addr().expect("an address"));
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serves") });

        let models = ["mistral", "empty", "broken"]
            .into_iter()
            .map(|name| {
                let mut entry = ModelEntry::default();
                entry.name = name.to_string();
                entry
            })
            .collect();
        let sized = attach(models, &client(None).expect("a client"), &registry).await;

        let sizes: Vec<Option<u64>> = sized.iter().map(|model| model.size).collect();
        assert_eq!(sizes, [Some(4_002), None, None]);
        let agents = agents.lock().expect("agents");
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].0.as_ref().map(|value| value.as_bytes()),
            Some(USER_AGENT.as_bytes())
        );
        assert_eq!(
            agents[0].1.as_ref().map(|value| value.as_bytes()),
            Some(MANIFEST_MEDIA_TYPE.as_bytes())
        );
    }
}
