import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Wiki entry source type enum
pub type SourceType {
  ChatSource
  ManualSource
  UrlSource
  TaskSource
  GithubSource
}

/// Wiki entry entity
pub type WikiEntry {
  WikiEntry(
    id: String,
    title: String,
    content: String,
    source_type: SourceType,
    source_id: Option(String),
    source_url: Option(String),
    created_at: String,
    updated_at: String,
  )
}

/// Wiki chunk entity (for vector embeddings)
pub type WikiChunk {
  WikiChunk(
    id: String,
    wiki_entry_id: String,
    chunk_index: Int,
    content: String,
    token_count: Option(Int),
    created_at: String,
  )
}

/// Search result combining entry and chunk with relevance score
pub type WikiSearchResult {
  WikiSearchResult(entry: WikiEntry, chunk: String, relevance_score: Float)
}

/// Request to ingest manual content
pub type IngestContentRequest {
  IngestContentRequest(title: String, content: String)
}

/// Request to ingest from URL
pub type IngestUrlRequest {
  IngestUrlRequest(url: String)
}

/// Convert SourceType to string for database
pub fn source_type_to_string(st: SourceType) -> String {
  case st {
    ChatSource -> "chat"
    ManualSource -> "manual"
    UrlSource -> "url"
    TaskSource -> "task"
    GithubSource -> "github"
  }
}

/// Parse string to SourceType
pub fn source_type_from_string(s: String) -> Result(SourceType, Nil) {
  case s {
    "chat" -> Ok(ChatSource)
    "manual" -> Ok(ManualSource)
    "url" -> Ok(UrlSource)
    "task" -> Ok(TaskSource)
    "github" -> Ok(GithubSource)
    _ -> Error(Nil)
  }
}

/// Decoder for SourceType from database string
pub fn source_type_decoder() -> decode.Decoder(SourceType) {
  decode.string
  |> decode.then(fn(s) {
    case source_type_from_string(s) {
      Ok(st) -> decode.success(st)
      Error(_) -> decode.failure(ManualSource, "SourceType")
    }
  })
}

/// Decode IngestContentRequest from JSON
pub fn decode_ingest_content_request(
  data: String,
) -> Result(IngestContentRequest, json.DecodeError) {
  let decoder = {
    use title <- decode.field("title", decode.string)
    use content <- decode.field("content", decode.string)
    decode.success(IngestContentRequest(title: title, content: content))
  }

  json.parse(data, decoder)
}

/// Decode IngestUrlRequest from JSON
pub fn decode_ingest_url_request(
  data: String,
) -> Result(IngestUrlRequest, json.DecodeError) {
  let decoder = {
    use url <- decode.field("url", decode.string)
    decode.success(IngestUrlRequest(url: url))
  }

  json.parse(data, decoder)
}

/// Convert WikiEntry to JSON
pub fn to_json(entry: WikiEntry) -> json.Json {
  json.object([
    #("id", json.string(entry.id)),
    #("title", json.string(entry.title)),
    #("content", json.string(entry.content)),
    #("source_type", json.string(source_type_to_string(entry.source_type))),
    #("source_id", option_to_json(entry.source_id)),
    #("source_url", option_to_json(entry.source_url)),
    #("created_at", json.string(entry.created_at)),
    #("updated_at", json.string(entry.updated_at)),
  ])
}

/// Convert WikiChunk to JSON
pub fn chunk_to_json(chunk: WikiChunk) -> json.Json {
  json.object([
    #("id", json.string(chunk.id)),
    #("wiki_entry_id", json.string(chunk.wiki_entry_id)),
    #("chunk_index", json.int(chunk.chunk_index)),
    #("content", json.string(chunk.content)),
    #("token_count", option_int_to_json(chunk.token_count)),
    #("created_at", json.string(chunk.created_at)),
  ])
}

/// Convert WikiSearchResult to JSON
pub fn search_result_to_json(result: WikiSearchResult) -> json.Json {
  json.object([
    #("entry", to_json(result.entry)),
    #("chunk", json.string(result.chunk)),
    #("relevance_score", json.float(result.relevance_score)),
  ])
}

fn option_to_json(opt: Option(String)) -> json.Json {
  case opt {
    Some(s) -> json.string(s)
    None -> json.null()
  }
}

fn option_int_to_json(opt: Option(Int)) -> json.Json {
  case opt {
    Some(i) -> json.int(i)
    None -> json.null()
  }
}
