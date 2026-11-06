//! The remote catalogues `/api/models` and the agent's browse tool read,
//! through abnegate-llm's providers.

use abnegate_comfy::recipe::RecipeCatalog;
use abnegate_llm::catalog::{
    BrowseQuery, CatalogError, DEFAULT_PAGE_SIZE, MAXIMUM_PAGE_SIZE, ModelMediumFilter, ModelPage,
    ModelProvider, huggingface_repository_downloads, huggingface_repository_id,
};

use super::catalog_failure::CatalogFailure;
use super::catalog_providers::CatalogProviders;
use super::catalog_source::CatalogSource;
use super::popular;
use super::recipe_catalog;
use super::types::{BrowseResponse, HuggingFaceModelInfo, ListModelsQuery};
use crate::state::AppState;

/// One page of the catalogue named `source`, as `query` asks for it.
pub(super) async fn browse(
    state: &AppState,
    source: &str,
    query: &ListModelsQuery,
) -> Result<BrowseResponse, CatalogFailure> {
    let source = CatalogSource::parse(source)
        .ok_or_else(|| CatalogFailure::UnknownSource(source.to_string()))?;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .min(MAXIMUM_PAGE_SIZE);
    let page = search(state, source, query.to_browse_query(limit))
        .await
        .map_err(|error| CatalogFailure::catalog(source, error))?;
    Ok(BrowseResponse::from(page))
}

async fn search(
    state: &AppState,
    source: CatalogSource,
    options: BrowseQuery<'_>,
) -> Result<ModelPage, CatalogError> {
    let providers = CatalogProviders::current(state.config())?;
    match source {
        CatalogSource::Ollama => {
            popular::search(
                &providers.ollama,
                &providers.sizes,
                providers.registry_url(),
                options,
            )
            .await
        }
        CatalogSource::HuggingFace if lists_adapters(&options) => {
            providers
                .huggingface
                .search_with_adapters(options, &adapter_bases(state))
                .await
        }
        CatalogSource::HuggingFace => providers.huggingface.search(options).await,
        CatalogSource::Gpt4All => providers.gpt4all.search(options).await,
        CatalogSource::OpenRouter => providers.openrouter.search(options).await,
    }
}

/// Whether HuggingFace's `search_with_adapters` lists adapters for
/// `options`: an image-generation browse, or the first page of a search
/// across every medium.
fn lists_adapters(options: &BrowseQuery<'_>) -> bool {
    match options.medium {
        ModelMediumFilter::ImageGeneration => true,
        ModelMediumFilter::All => {
            options.cursor.is_none()
                && options
                    .query
                    .map(str::trim)
                    .is_some_and(|query| !query.is_empty())
        }
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_are_listed_only_where_the_crate_would_list_them() {
        let mut options = BrowseQuery::default();
        assert!(!lists_adapters(&options));
        options.query = Some("  ");
        assert!(!lists_adapters(&options));
        options.query = Some("flux");
        assert!(lists_adapters(&options));
        options.cursor = Some("next");
        assert!(!lists_adapters(&options));
        options.medium = ModelMediumFilter::ImageGeneration;
        assert!(lists_adapters(&options));
        options.medium = ModelMediumFilter::Image;
        options.cursor = None;
        assert!(!lists_adapters(&options));
    }
}
