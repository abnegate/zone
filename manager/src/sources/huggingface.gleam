/// HuggingFace model source
/// Browses GGUF models from huggingface.co
import gleam/dynamic/decode
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/uri
import services/http_client
import sources/source.{
  type BrowseModel, type BrowseResult, type ModelInfo, type ModelSourceHandler,
  BrowseModel, BrowseResult, ModelInfo, ModelSourceHandler,
}

pub const source_id = "huggingface"

pub const source_name = "HuggingFace"

pub const base_url = "https://huggingface.co"

pub const api_url = "https://huggingface.co/api/models"

/// Get the handler for HuggingFace source
pub fn handler() -> ModelSourceHandler {
  ModelSourceHandler(
    id: source_id,
    name: source_name,
    base_url: base_url,
    browse: browse,
    get_info: get_info,
  )
}

/// Browse HuggingFace for GGUF models
pub fn browse(
  search: String,
  limit: Int,
  offset: Int,
) -> Result(BrowseResult, String) {
  let query_params = case search {
    "" ->
      "?filter=gguf&sort=downloads&direction=-1&limit="
      <> int.to_string(limit)
      <> "&skip="
      <> int.to_string(offset)
    q ->
      "?search="
      <> uri.percent_encode(q)
      <> "&filter=gguf&sort=downloads&direction=-1&limit="
      <> int.to_string(limit)
      <> "&skip="
      <> int.to_string(offset)
  }
  let url = api_url <> query_params

  case http_client.get(url, []) {
    Ok(body) -> Ok(parse_browse_response(body, limit))
    Error(err) -> Error("Failed to fetch from HuggingFace: " <> err)
  }
}

/// Get detailed info for a specific model
pub fn get_info(model_id: String) -> Result(ModelInfo, String) {
  let info_url = api_url <> "/" <> model_id
  let readme_url = base_url <> "/" <> model_id <> "/raw/main/README.md"

  // Fetch model API info
  let api_result = http_client.get(info_url, [])
  let readme_result = http_client.get(readme_url, [])

  case api_result {
    Ok(api_body) -> {
      let info = parse_model_info(model_id, api_body)
      let readme = case readme_result {
        Ok(content) -> Some(content)
        Error(_) -> None
      }
      Ok(ModelInfo(..info, readme: readme))
    }
    Error(err) -> Error("Failed to fetch model info: " <> err)
  }
}

/// Parse the browse API response
fn parse_browse_response(body: String, limit: Int) -> BrowseResult {
  let decoder =
    decode.list({
      use id <- decode.field("modelId", decode.string)
      use downloads <- decode.optional_field("downloads", 0, decode.int)
      use tags <- decode.optional_field("tags", [], decode.list(decode.string))
      use pipeline_tag <- decode.optional_field(
        "pipeline_tag",
        "",
        decode.string,
      )
      use author <- decode.optional_field("author", "", decode.string)
      use likes <- decode.optional_field("likes", 0, decode.int)
      use last_modified <- decode.optional_field(
        "lastModified",
        "",
        decode.string,
      )
      decode.success(#(
        id,
        downloads,
        tags,
        pipeline_tag,
        author,
        likes,
        last_modified,
      ))
    })

  case json.parse(body, decoder) {
    Ok(models) -> {
      let has_more = list.length(models) >= limit
      let browse_models =
        list.map(models, fn(m) {
          let #(id, downloads, tags, pipeline, author, likes, last_modified) = m
          BrowseModel(
            id: id,
            name: id,
            description: case pipeline {
              "" -> "HuggingFace GGUF model"
              p -> "HuggingFace GGUF model - " <> p
            },
            downloads: downloads,
            tags: tags,
            install_name: Some("hf.co/" <> id),
            author: Some(author),
            likes: Some(likes),
            last_modified: Some(last_modified),
            url: Some(base_url <> "/" <> id),
          )
        })
      BrowseResult(
        source: source_id,
        models: browse_models,
        total: None,
        has_more: has_more,
      )
    }
    Error(_) -> {
      BrowseResult(
        source: source_id,
        models: [],
        total: Some(0),
        has_more: False,
      )
    }
  }
}

/// Parse model info from API response
fn parse_model_info(model_id: String, body: String) -> ModelInfo {
  let size_decoder = {
    use gguf <- decode.optional_field(
      "gguf",
      None,
      {
        use total <- decode.field("total", decode.int)
        decode.success(total)
      }
        |> decode.map(Some),
    )
    decode.success(gguf)
  }

  let info_decoder = {
    use downloads <- decode.optional_field(
      "downloads",
      None,
      decode.int |> decode.map(Some),
    )
    use author <- decode.optional_field(
      "author",
      None,
      decode.string |> decode.map(Some),
    )
    use pipeline_tag <- decode.optional_field(
      "pipeline_tag",
      None,
      decode.string |> decode.map(Some),
    )
    decode.success(#(downloads, author, pipeline_tag))
  }

  let size = case json.parse(body, size_decoder) {
    Ok(Some(s)) -> Some(s)
    _ -> None
  }

  let #(downloads, author, description) = case json.parse(body, info_decoder) {
    Ok(info) -> info
    Error(_) -> #(None, None, None)
  }

  ModelInfo(
    id: model_id,
    name: model_id,
    description: description,
    readme: None,
    size_bytes: size,
    downloads: downloads,
    author: author,
    url: Some(base_url <> "/" <> model_id),
  )
}
