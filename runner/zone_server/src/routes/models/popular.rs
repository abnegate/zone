mod entries;
mod manifest_size;
mod refine;

use abnegate_llm::catalog::BrowseQuery;
use abnegate_llm::catalog::CatalogError;
use abnegate_llm::catalog::ModelMediumFilter;
use abnegate_llm::catalog::ModelPage;
use abnegate_llm::catalog::ModelProvider;
use abnegate_llm::catalog::ModelSort;
use abnegate_llm::catalog::OllamaProvider;

const PROBE_LIMIT: usize = 1;

/// One page of ollama.com's library, or of Zone's popular models when the
/// library page lists no models at all, so a change to ollama.com's markup
/// leaves Browse with models to offer rather than an empty Ollama half.
pub(super) async fn search(
    provider: &OllamaProvider,
    registry_url: &str,
    proxy_url: Option<&str>,
    options: BrowseQuery<'_>,
) -> Result<ModelPage, CatalogError> {
    let page = provider.search(options).await?;
    if !page.models.is_empty() || !library_is_empty(provider, &options).await {
        return Ok(page);
    }
    fallback(registry_url, proxy_url, &options).await
}

/// Whether the search page `options` fetched lists no models before any
/// filter applies. The crate does not say why a page came back empty, so the
/// same search page is fetched again, unfiltered. A probe that fails leaves
/// the empty page as the answer, since the browse itself succeeded.
async fn library_is_empty(provider: &OllamaProvider, options: &BrowseQuery<'_>) -> bool {
    let terms = search_terms(options);
    let mut probe = BrowseQuery::default();
    probe.query = terms.as_deref();
    probe.limit = PROBE_LIMIT;
    probe.sort = ModelSort::Relevance;
    probe.medium = searched_category(options.medium);
    match provider.search(probe).await {
        Ok(page) => page.models.is_empty(),
        Err(error) => {
            tracing::warn!(%error, "Could not probe the Ollama library for listed models");
            false
        }
    }
}

/// The text ollama.com is searched for: the query, then the family unless the
/// query already names it.
fn search_terms(options: &BrowseQuery<'_>) -> Option<String> {
    let mut terms = Vec::new();
    if let Some(query) = options
        .query
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        terms.push(query);
    }
    if let Some(family) = options
        .family
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && !terms.iter().any(|term| term.eq_ignore_ascii_case(family))
    {
        terms.push(family);
    }
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// The medium kept on the probe: only one ollama.com searches by category,
/// because every other medium is a filter the crate applies after fetching.
fn searched_category(medium: ModelMediumFilter) -> ModelMediumFilter {
    match medium {
        ModelMediumFilter::Image
        | ModelMediumFilter::Tools
        | ModelMediumFilter::Embeddings
        | ModelMediumFilter::Reasoning => medium,
        _ => ModelMediumFilter::All,
    }
}

async fn fallback(
    registry_url: &str,
    proxy_url: Option<&str>,
    options: &BrowseQuery<'_>,
) -> Result<ModelPage, CatalogError> {
    let page_size = options.page_size();
    let offset = refine::cursor_offset(options.cursor, page_size)?;
    let client = manifest_size::client(proxy_url)?;
    let models = entries::popular_models();
    if matches!(
        options.sort,
        ModelSort::SizeAscending | ModelSort::SizeDescending
    ) {
        let models = manifest_size::attach(models, &client, registry_url).await;
        return Ok(refine::paginate(
            refine::refine(models, options),
            offset,
            page_size,
        ));
    }
    let mut page = refine::paginate(refine::refine(models, options), offset, page_size);
    page.models = manifest_size::attach(page.models, &client, registry_url).await;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::Query;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::Html;
    use axum::response::IntoResponse;
    use axum::response::Response;
    use axum::routing::get;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    const NO_CARDS: &str = "<html><body><a href=\"/search\">Models</a></body></html>";
    const TOOL_CARD: &str = "<html><body><a href=\"/library/qwen3\"><p>Qwen 3</p>\
        <span>tools</span><span>8b</span></a></body></html>";
    const PLAIN_CARD: &str =
        "<html><body><a href=\"/library/mistral\"><p>Mistral</p><span>7b</span></a></body></html>";

    #[derive(Clone, Copy)]
    enum Library {
        NoCards,
        PlainCard,
        ToolCard,
        FailsAfterFirst,
    }

    struct Search {
        library: Library,
        requests: Mutex<Vec<String>>,
    }

    async fn search_page(
        State(search): State<Arc<Search>>,
        Query(parameters): Query<BTreeMap<String, String>>,
    ) -> Response {
        let key = ["q", "c"]
            .into_iter()
            .filter_map(|name| parameters.get(name).map(|value| format!("{name}={value}")))
            .collect::<Vec<_>>()
            .join("&");
        let mut requests = search.requests.lock().expect("requests");
        requests.push(key);
        match search.library {
            Library::NoCards => Html(NO_CARDS).into_response(),
            Library::PlainCard => Html(PLAIN_CARD).into_response(),
            Library::ToolCard => Html(TOOL_CARD).into_response(),
            Library::FailsAfterFirst if requests.len() == 1 => Html(PLAIN_CARD).into_response(),
            Library::FailsAfterFirst => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        }
    }

    async fn browse(library: Library, options: BrowseQuery<'_>) -> (ModelPage, Vec<String>) {
        let search = Arc::new(Search {
            library,
            requests: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/search", get(search_page))
            .route("/v2/{*path}", get(|| async { StatusCode::NOT_FOUND }))
            .with_state(search.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let origin = format!("http://{}", listener.local_addr().expect("an address"));
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serves") });

        let registry = format!("{origin}/v2");
        let provider = OllamaProvider::with_catalog(format!("{origin}/search"), &registry, None)
            .expect("a provider");
        let page = super::search(&provider, &registry, None, options)
            .await
            .expect("a page");
        let requests = search.requests.lock().expect("requests").clone();
        (page, requests)
    }

    #[tokio::test]
    async fn a_library_page_without_cards_falls_back_to_popular_models() {
        let (page, requests) = browse(Library::NoCards, BrowseQuery::default()).await;
        assert_eq!(page.models.len(), 20);
        assert_eq!(page.models[0].name, "llama3.2");
        assert_eq!(page.next_cursor.as_deref(), Some("offset:20"));
        assert_eq!(requests, ["", ""]);
    }

    #[tokio::test]
    async fn the_probe_searches_the_same_terms_unfiltered() {
        let mut options = BrowseQuery::default();
        options.query = Some("coder");
        options.family = Some("llama");
        options.size = abnegate_llm::catalog::ModelSizeFilter::Small;
        options.medium = ModelMediumFilter::Embeddings;
        options.sort = ModelSort::NameDescending;
        let (_, requests) = browse(Library::NoCards, options).await;
        assert_eq!(
            requests,
            ["q=coder llama&c=embedding", "q=coder llama&c=embedding"]
        );
    }

    #[tokio::test]
    async fn a_medium_ollama_cannot_search_by_is_left_off_the_probe() {
        let mut options = BrowseQuery::default();
        options.medium = ModelMediumFilter::Text;
        let (_, requests) = browse(Library::NoCards, options).await;
        assert_eq!(requests, ["", ""]);
    }

    #[tokio::test]
    async fn filters_that_empty_a_listed_page_keep_it_empty() {
        let mut options = BrowseQuery::default();
        options.family = Some("qwen");
        let (page, requests) = browse(Library::PlainCard, options).await;
        assert!(page.models.is_empty());
        assert_eq!(page.next_cursor, None);
        assert_eq!(requests, ["q=qwen", "q=qwen"]);
    }

    #[tokio::test]
    async fn a_category_page_whose_cards_lack_it_stays_empty_as_zone_left_it() {
        let mut options = BrowseQuery::default();
        options.medium = ModelMediumFilter::Tools;
        let (listed, _) = browse(Library::ToolCard, options).await;
        assert_eq!(listed.models.len(), 1);

        let (page, requests) = browse(Library::PlainCard, options).await;
        assert!(page.models.is_empty(), "{:?}", page.models);
        assert_eq!(requests, ["c=tools", "c=tools"]);
    }

    #[tokio::test]
    async fn a_failed_probe_keeps_the_empty_page() {
        let mut options = BrowseQuery::default();
        options.family = Some("qwen");
        let (page, requests) = browse(Library::FailsAfterFirst, options).await;
        assert!(page.models.is_empty());
        assert_eq!(page.next_cursor, None);
        assert_eq!(requests.len(), 2);
    }

    #[tokio::test]
    async fn a_listed_page_is_answered_without_a_probe() {
        let (page, requests) = browse(Library::PlainCard, BrowseQuery::default()).await;
        assert_eq!(page.models[0].name, "mistral");
        assert_eq!(requests, [""]);
    }

    #[test]
    fn a_family_the_query_names_is_searched_once() {
        let mut options = BrowseQuery::default();
        options.query = Some(" Llama ");
        options.family = Some("llama");
        assert_eq!(search_terms(&options).as_deref(), Some("Llama"));
        options.query = None;
        assert_eq!(search_terms(&options).as_deref(), Some("llama"));
        options.family = Some(" ");
        assert_eq!(search_terms(&options), None);
    }
}
