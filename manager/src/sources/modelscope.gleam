/// ModelScope model source
/// Browses GGUF models from modelscope.cn (Alibaba's model hub)
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

pub const source_id = "modelscope"

pub const source_name = "ModelScope"

pub const base_url = "https://modelscope.cn"

pub const api_url = "https://modelscope.cn/api/v1"

/// Get the handler for ModelScope source
pub fn handler() -> ModelSourceHandler {
  ModelSourceHandler(
    id: source_id,
    name: source_name,
    base_url: base_url,
    browse: browse,
    get_info: get_info,
  )
}

/// Browse ModelScope for GGUF models
pub fn browse(
  search: String,
  limit: Int,
  offset: Int,
) -> Result(BrowseResult, String) {
  // ModelScope uses page-based pagination
  let page = offset / limit + 1

  // Build search URL - filter for GGUF models
  let url =
    api_url
    <> "/models?PageSize="
    <> int.to_string(limit)
    <> "&PageNumber="
    <> int.to_string(page)
    <> "&SortBy=Downloads"
    <> "&Tags=gguf"
    <> case search {
      "" -> ""
      q -> "&Name=" <> uri.percent_encode(q)
    }

  case http_client.get(url, []) {
    Ok(body) -> Ok(parse_browse_response(body, limit))
    Error(err) -> Error("Failed to fetch from ModelScope: " <> err)
  }
}

/// Get detailed info for a specific model
pub fn get_info(model_id: String) -> Result(ModelInfo, String) {
  let info_url = api_url <> "/models/" <> model_id
  let readme_url = base_url <> "/" <> model_id <> "/resolve/master/README.md"

  case http_client.get(info_url, []) {
    Ok(api_body) -> {
      let info = parse_model_info(model_id, api_body)
      // Try to fetch README
      let readme = case http_client.get(readme_url, []) {
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
  // ModelScope returns: { "Data": { "Models": [...], "TotalCount": n }, "Success": true }
  let models_decoder =
    decode.list({
      use name <- decode.field("Name", decode.string)
      use path <- decode.field("Path", decode.string)
      use description <- decode.optional_field("Description", "", decode.string)
      use downloads <- decode.optional_field("Downloads", 0, decode.int)
      use tags <- decode.optional_field("Tags", [], decode.list(decode.string))
      use likes <- decode.optional_field("Likes", 0, decode.int)
      use updated_at <- decode.optional_field(
        "LastUpdatedTime",
        "",
        decode.string,
      )
      decode.success(#(
        name,
        path,
        description,
        downloads,
        tags,
        likes,
        updated_at,
      ))
    })

  let response_decoder = {
    use data <- decode.field("Data", {
      use models <- decode.field("Models", models_decoder)
      use total <- decode.optional_field("TotalCount", 0, decode.int)
      decode.success(#(models, total))
    })
    decode.success(data)
  }

  case json.parse(body, response_decoder) {
    Ok(#(models, total)) -> {
      let has_more = list.length(models) >= limit
      let browse_models =
        list.map(models, fn(m) {
          let #(name, path, description, downloads, tags, likes, updated_at) = m
          BrowseModel(
            id: path,
            name: name,
            description: description,
            downloads: downloads,
            tags: tags,
            install_name: Some("modelscope/" <> path),
            author: extract_author(path),
            likes: Some(likes),
            last_modified: Some(updated_at),
            url: Some(base_url <> "/" <> path),
          )
        })
      BrowseResult(
        source: source_id,
        models: browse_models,
        total: Some(total),
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

/// Extract author from path (format: "author/model-name")
fn extract_author(path: String) -> Option(String) {
  case path {
    "" -> None
    p -> {
      // Split on "/" and take first part
      let parts = split_on_slash(p)
      case parts {
        [author, ..] -> Some(author)
        _ -> None
      }
    }
  }
}

fn split_on_slash(s: String) -> List(String) {
  do_split_on_slash(s, "", [])
}

fn do_split_on_slash(
  s: String,
  current: String,
  acc: List(String),
) -> List(String) {
  case s {
    "" -> list.reverse([current, ..acc])
    "/" <> rest -> do_split_on_slash(rest, "", [current, ..acc])
    _ -> {
      case pop_grapheme(s) {
        Ok(#(char, rest)) -> do_split_on_slash(rest, current <> char, acc)
        Error(_) -> list.reverse([current, ..acc])
      }
    }
  }
}

@external(erlang, "string", "next_grapheme")
fn pop_grapheme(s: String) -> Result(#(String, String), Nil)

/// Parse model info from API response
fn parse_model_info(model_id: String, body: String) -> ModelInfo {
  let info_decoder = {
    use data <- decode.field("Data", {
      use name <- decode.field("Name", decode.string)
      use description <- decode.optional_field(
        "Description",
        None,
        decode.string |> decode.map(Some),
      )
      use downloads <- decode.optional_field(
        "Downloads",
        None,
        decode.int |> decode.map(Some),
      )
      use path <- decode.field("Path", decode.string)
      decode.success(#(name, description, downloads, path))
    })
    decode.success(data)
  }

  case json.parse(body, info_decoder) {
    Ok(#(name, description, downloads, path)) -> {
      ModelInfo(
        id: model_id,
        name: name,
        description: description,
        readme: None,
        size_bytes: None,
        downloads: downloads,
        author: extract_author(path),
        url: Some(base_url <> "/" <> path),
      )
    }
    Error(_) -> {
      ModelInfo(
        id: model_id,
        name: model_id,
        description: None,
        readme: None,
        size_bytes: None,
        downloads: None,
        author: None,
        url: Some(base_url <> "/" <> model_id),
      )
    }
  }
}
