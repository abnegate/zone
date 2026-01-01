/// Common types for model sources
///
/// To add a new model source:
/// 1. Create a new module in sources/ (e.g., mysource.gleam)
/// 2. Implement browse() and get_info() functions
/// 3. Export a handler() function that returns a ModelSourceHandler
/// 4. Add the source to the registry in this file
import gleam/json
import gleam/option.{type Option, None, Some}

/// Handler functions for a model source
/// Each source module exports one of these
pub type ModelSourceHandler {
  ModelSourceHandler(
    id: String,
    name: String,
    base_url: String,
    browse: fn(String, Int, Int) -> Result(BrowseResult, String),
    get_info: fn(String) -> Result(ModelInfo, String),
  )
}

/// A model from a browse result
pub type BrowseModel {
  BrowseModel(
    id: String,
    name: String,
    description: String,
    downloads: Int,
    tags: List(String),
    /// Optional fields for richer sources
    install_name: Option(String),
    author: Option(String),
    likes: Option(Int),
    last_modified: Option(String),
    url: Option(String),
  )
}

/// Result of browsing a source
pub type BrowseResult {
  BrowseResult(
    source: String,
    models: List(BrowseModel),
    total: Option(Int),
    has_more: Bool,
  )
}

/// Detailed model info
pub type ModelInfo {
  ModelInfo(
    id: String,
    name: String,
    description: Option(String),
    readme: Option(String),
    size_bytes: Option(Int),
    downloads: Option(Int),
    author: Option(String),
    url: Option(String),
  )
}

/// Convert BrowseModel to JSON
pub fn browse_model_to_json(model: BrowseModel) -> json.Json {
  json.object([
    #("id", json.string(model.id)),
    #("name", json.string(model.name)),
    #("description", json.string(model.description)),
    #("downloads", json.int(model.downloads)),
    #("tags", json.array(model.tags, json.string)),
    #("install_name", option_to_json(model.install_name, json.string)),
    #("author", option_to_json(model.author, json.string)),
    #("likes", option_to_json(model.likes, json.int)),
    #("last_modified", option_to_json(model.last_modified, json.string)),
    #("url", option_to_json(model.url, json.string)),
  ])
}

/// Convert BrowseResult to JSON string
pub fn browse_result_to_json(result: BrowseResult) -> String {
  json.object([
    #("source", json.string(result.source)),
    #("models", json.array(result.models, browse_model_to_json)),
    #("total", option_to_json(result.total, json.int)),
    #("has_more", json.bool(result.has_more)),
  ])
  |> json.to_string
}

/// Convert ModelInfo to JSON string
pub fn model_info_to_json(info: ModelInfo) -> String {
  json.object([
    #("success", json.bool(True)),
    #("id", json.string(info.id)),
    #("name", json.string(info.name)),
    #("description", option_to_json(info.description, json.string)),
    #("content", option_to_json(info.readme, json.string)),
    #("gguf_size", option_to_json(info.size_bytes, json.int)),
    #("downloads", option_to_json(info.downloads, json.int)),
    #("author", option_to_json(info.author, json.string)),
    #("url", option_to_json(info.url, json.string)),
  ])
  |> json.to_string
}

/// Helper to convert Option to JSON
fn option_to_json(opt: Option(a), encoder: fn(a) -> json.Json) -> json.Json {
  case opt {
    Some(value) -> encoder(value)
    None -> json.null()
  }
}
