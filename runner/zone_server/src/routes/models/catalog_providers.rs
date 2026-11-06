use std::sync::{Arc, Mutex};

use abnegate_llm::catalog::{
    CatalogError, Gpt4AllProvider, HuggingFaceProvider, OllamaProvider, OpenRouterProvider,
};
use reqwest::Client;

use super::catalog_endpoints::{self, CatalogEndpoints};
use super::popular;
use crate::config::Config;

static BUILT: Mutex<Option<Arc<CatalogProviders>>> = Mutex::new(None);

/// The catalogue clients every browse shares, so their connection pools
/// outlive a request. Built again only when the origins or proxy they were
/// built for change.
pub(super) struct CatalogProviders {
    endpoints: CatalogEndpoints,
    huggingface_url: String,
    gpt4all_url: String,
    proxy_url: Option<String>,
    pub(super) ollama: OllamaProvider,
    pub(super) huggingface: HuggingFaceProvider,
    pub(super) gpt4all: Gpt4AllProvider,
    pub(super) openrouter: OpenRouterProvider,
    pub(super) sizes: Client,
}

impl CatalogProviders {
    /// The providers for `config` and the current catalogue endpoints.
    pub(super) fn current(config: &Config) -> Result<Arc<Self>, CatalogError> {
        let endpoints = catalog_endpoints::current();
        if let Some(built) = BUILT
            .lock()
            .expect("catalogue providers")
            .as_ref()
            .filter(|built| built.serves(&endpoints, config))
        {
            return Ok(Arc::clone(built));
        }
        let built = Arc::new(Self::build(endpoints, config)?);
        *BUILT.lock().expect("catalogue providers") = Some(Arc::clone(&built));
        Ok(built)
    }

    fn build(endpoints: CatalogEndpoints, config: &Config) -> Result<Self, CatalogError> {
        let proxy = config.model_search_proxy_url.as_deref();
        Ok(Self {
            ollama: OllamaProvider::with_catalog(
                endpoints.ollama_search.as_str(),
                endpoints.ollama_registry.as_str(),
                proxy,
            )?,
            huggingface: HuggingFaceProvider::with_proxy(
                config.huggingface_models_url.as_str(),
                proxy,
            )?,
            gpt4all: Gpt4AllProvider::with_proxy(config.gpt4all_models_url.as_str(), proxy)?,
            openrouter: OpenRouterProvider::with_proxy(endpoints.openrouter.as_str(), proxy)?,
            sizes: popular::size_client(proxy)?,
            huggingface_url: config.huggingface_models_url.clone(),
            gpt4all_url: config.gpt4all_models_url.clone(),
            proxy_url: config.model_search_proxy_url.clone(),
            endpoints,
        })
    }

    pub(super) fn registry_url(&self) -> &str {
        &self.endpoints.ollama_registry
    }

    fn serves(&self, endpoints: &CatalogEndpoints, config: &Config) -> bool {
        self.endpoints == *endpoints
            && self.huggingface_url == config.huggingface_models_url
            && self.gpt4all_url == config.gpt4all_models_url
            && self.proxy_url == config.model_search_proxy_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog_endpoints::ENDPOINTS;

    /// The providers are process-wide, so tests that compare them must not
    /// replace them under one another.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn endpoints(port: u16) -> CatalogEndpoints {
        CatalogEndpoints {
            ollama_search: format!("http://127.0.0.1:{port}/search"),
            ollama_registry: format!("http://127.0.0.1:{port}/v2"),
            openrouter: format!("http://127.0.0.1:{port}/api/v1/models"),
        }
    }

    async fn current_in(endpoints: CatalogEndpoints, config: &Config) -> Arc<CatalogProviders> {
        ENDPOINTS
            .scope(endpoints, async { CatalogProviders::current(config) })
            .await
            .expect("catalogue providers")
    }

    #[tokio::test]
    async fn browses_with_the_same_settings_share_one_set_of_providers() {
        let _serial = SERIAL.lock().await;
        let config = crate::state::test_config();
        let first = current_in(endpoints(1), &config).await;
        let second = current_in(endpoints(1), &config).await;
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn a_change_of_origin_or_proxy_builds_new_providers() {
        let _serial = SERIAL.lock().await;
        let mut config = crate::state::test_config();
        let first = current_in(endpoints(2), &config).await;
        let moved = current_in(endpoints(3), &config).await;
        assert!(!Arc::ptr_eq(&first, &moved));
        assert_eq!(moved.registry_url(), "http://127.0.0.1:3/v2");

        config.model_search_proxy_url = Some("http://127.0.0.1:3128".to_string());
        let proxied = current_in(endpoints(3), &config).await;
        assert!(!Arc::ptr_eq(&moved, &proxied));

        config.huggingface_models_url = "http://127.0.0.1:3/api/models".to_string();
        let mirrored = current_in(endpoints(3), &config).await;
        assert!(!Arc::ptr_eq(&proxied, &mirrored));
    }

    #[tokio::test]
    async fn a_malformed_proxy_is_reported_and_not_kept() {
        let mut config = crate::state::test_config();
        config.model_search_proxy_url = Some("not a url".to_string());
        let refused = ENDPOINTS
            .scope(endpoints(4), async { CatalogProviders::current(&config) })
            .await;
        assert!(matches!(refused, Err(CatalogError::Http(_))));
    }
}
