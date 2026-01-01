/// Ollama Library model source
/// Browses curated models from the local ollama_models database
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import ollama_models
import sources/source.{
  type BrowseModel, type BrowseResult, type ModelInfo, type ModelSourceHandler,
  BrowseModel, BrowseResult, ModelInfo, ModelSourceHandler,
}

pub const source_id = "ollama"

pub const source_name = "Ollama Library"

pub const base_url = "https://ollama.com/library"

/// Get the handler for Ollama Library source
pub fn handler() -> ModelSourceHandler {
  ModelSourceHandler(
    id: source_id,
    name: source_name,
    base_url: base_url,
    browse: browse,
    get_info: get_info,
  )
}

/// Browse Ollama library models
pub fn browse(
  search: String,
  limit: Int,
  offset: Int,
) -> Result(BrowseResult, String) {
  let models = ollama_models.get_all_models()

  let filtered = case search {
    "" -> models
    q ->
      list.filter(models, fn(m: ollama_models.OllamaModel) {
        string.contains(string.lowercase(m.name), string.lowercase(q))
        || string.contains(string.lowercase(m.description), string.lowercase(q))
        || list.any(m.tags, fn(tag) {
          string.contains(string.lowercase(tag), string.lowercase(q))
        })
      })
  }

  let total = list.length(filtered)
  let paginated =
    filtered
    |> list.drop(offset)
    |> list.take(limit)
  let has_more = offset + limit < total

  let browse_models =
    list.map(paginated, fn(m) {
      BrowseModel(
        id: m.name,
        name: m.name,
        description: m.description,
        downloads: m.pulls,
        tags: m.tags,
        install_name: Some(m.name),
        author: None,
        likes: None,
        last_modified: None,
        url: Some(base_url <> "/" <> m.name),
      )
    })

  Ok(BrowseResult(
    source: source_id,
    models: browse_models,
    total: Some(total),
    has_more: has_more,
  ))
}

/// Get detailed info for an Ollama model
pub fn get_info(model_id: String) -> Result(ModelInfo, String) {
  let models = ollama_models.get_all_models()

  case list.find(models, fn(m) { m.name == model_id }) {
    Ok(model) -> {
      Ok(ModelInfo(
        id: model.name,
        name: model.name,
        description: Some(model.description),
        readme: None,
        size_bytes: None,
        downloads: Some(model.pulls),
        author: None,
        url: Some(base_url <> "/" <> model.name),
      ))
    }
    Error(_) -> Error("Model not found: " <> model_id)
  }
}
