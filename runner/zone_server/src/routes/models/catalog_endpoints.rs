use abnegate_llm::catalog::{
    DEFAULT_OLLAMA_REGISTRY_URL, DEFAULT_OLLAMA_SEARCH_URL, DEFAULT_OPENROUTER_MODELS_URL,
};

/// The catalogue origins Zone browses that are not operator configuration.
///
/// Tests override them per task with `ENDPOINTS`, so every catalogue request
/// must be made from the task that reads [`current`], never a spawned one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogEndpoints {
    pub(crate) ollama_search: String,
    pub(crate) ollama_registry: String,
    pub(crate) openrouter: String,
}

impl Default for CatalogEndpoints {
    fn default() -> Self {
        Self {
            ollama_search: DEFAULT_OLLAMA_SEARCH_URL.to_string(),
            ollama_registry: DEFAULT_OLLAMA_REGISTRY_URL.to_string(),
            openrouter: DEFAULT_OPENROUTER_MODELS_URL.to_string(),
        }
    }
}

#[cfg(test)]
tokio::task_local! {
    pub(crate) static ENDPOINTS: CatalogEndpoints;
}

#[cfg(test)]
pub(crate) fn current() -> CatalogEndpoints {
    ENDPOINTS
        .try_with(CatalogEndpoints::clone)
        .unwrap_or_default()
}

#[cfg(not(test))]
pub(crate) fn current() -> CatalogEndpoints {
    CatalogEndpoints::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn outside_a_scope_the_production_catalogues_are_browsed() {
        assert_eq!(
            current(),
            CatalogEndpoints {
                ollama_search: "https://ollama.com/search".to_string(),
                ollama_registry: "https://registry.ollama.ai/v2".to_string(),
                openrouter: "https://openrouter.ai/api/v1/models".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn a_scope_overrides_the_catalogues_for_its_own_task() {
        let local = CatalogEndpoints {
            ollama_search: "http://127.0.0.1:1/search".to_string(),
            ollama_registry: "http://127.0.0.1:1/v2".to_string(),
            openrouter: "http://127.0.0.1:1/api/v1/models".to_string(),
        };
        let seen = ENDPOINTS.scope(local.clone(), async { current() }).await;
        assert_eq!(seen, local);
        assert_eq!(current(), CatalogEndpoints::default());
    }
}
