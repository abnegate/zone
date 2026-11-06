//! The remote catalogues `/api/models` and the agent's browse tool read,
//! through abnegate-llm's providers.

use abnegate_comfy::recipe::RecipeCatalog;
use abnegate_llm::catalog::BrowseQuery;
use abnegate_llm::catalog::DEFAULT_PAGE_SIZE;
use abnegate_llm::catalog::Gpt4AllProvider;
use abnegate_llm::catalog::HuggingFaceProvider;
use abnegate_llm::catalog::MAXIMUM_PAGE_SIZE;
use abnegate_llm::catalog::ModelPage;
use abnegate_llm::catalog::ModelProvider;
use abnegate_llm::catalog::OllamaProvider;
use abnegate_llm::catalog::OpenRouterProvider;
use abnegate_llm::catalog::huggingface_repository_downloads;
use abnegate_llm::catalog::huggingface_repository_id;

use super::catalog_endpoints;
use super::catalog_failure::CatalogFailure;
use super::popular;
use super::recipe_catalog;
use super::types::BrowseResponse;
use super::types::HuggingFaceModelInfo;
use super::types::ListModelsQuery;
use crate::state::AppState;

const GPT4ALL: &str = "gpt4all";
const HUGGINGFACE: &str = "huggingface";
const OLLAMA: &str = "ollama";
const OPENROUTER: &str = "openrouter";

/// One page of the catalogue named `source`, as `query` asks for it.
pub(super) async fn browse(
    state: &AppState,
    source: &str,
    query: &ListModelsQuery,
) -> Result<BrowseResponse, CatalogFailure> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .min(MAXIMUM_PAGE_SIZE);
    let page = search(state, source, query.to_browse_query(limit)).await?;
    Ok(BrowseResponse::from(page))
}

async fn search(
    state: &AppState,
    source: &str,
    options: BrowseQuery<'_>,
) -> Result<ModelPage, CatalogFailure> {
    let config = state.config();
    let proxy = config.model_search_proxy_url.as_deref();
    let endpoints = catalog_endpoints::current();
    let page = match source {
        OLLAMA => {
            let provider = OllamaProvider::with_catalog(
                endpoints.ollama_search,
                endpoints.ollama_registry.as_str(),
                proxy,
            )?;
            popular::search(&provider, &endpoints.ollama_registry, proxy, options).await?
        }
        HUGGINGFACE => {
            HuggingFaceProvider::with_proxy(config.huggingface_models_url.as_str(), proxy)?
                .search_with_adapters(options, &adapter_bases(state))
                .await?
        }
        GPT4ALL => {
            Gpt4AllProvider::with_proxy(config.gpt4all_models_url.as_str(), proxy)?
                .search(options)
                .await?
        }
        OPENROUTER => {
            OpenRouterProvider::with_proxy(endpoints.openrouter, proxy)?
                .search(options)
                .await?
        }
        other => return Err(CatalogFailure::UnknownSource(other.to_string())),
    };
    Ok(page)
}

/// The base models whose HuggingFace adapters Browse lists beside a search.
fn adapter_bases(state: &AppState) -> Vec<String> {
    recipe_catalog(&state.config().comfyui)
        .unwrap_or_else(|_| RecipeCatalog::packaged().expect("packaged recipes"))
        .huggingface_bases()
}

/// The GGUF downloads of the HuggingFace repository `name` was pulled from,
/// for the details of a model that is not installed locally.
pub(super) async fn huggingface_details(
    state: &AppState,
    name: &str,
) -> Option<HuggingFaceModelInfo> {
    let repository = huggingface_repository_id(name)?;
    let config = state.config();
    let entry = huggingface_repository_downloads(
        &config.huggingface_models_url,
        config.model_search_proxy_url.as_deref(),
        repository,
    )
    .await
    .ok()?;
    let downloadable = entry.sizes.is_some() || entry.size.is_some();
    downloadable.then_some(HuggingFaceModelInfo {
        content: None,
        gguf_size: entry.size,
        sizes: entry.sizes,
    })
}
