import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import models/wiki

// =============================================================================
// SourceType conversion tests
// =============================================================================

pub fn source_type_to_string_chat_test() {
  wiki.source_type_to_string(wiki.ChatSource)
  |> should.equal("chat")
}

pub fn source_type_to_string_manual_test() {
  wiki.source_type_to_string(wiki.ManualSource)
  |> should.equal("manual")
}

pub fn source_type_to_string_url_test() {
  wiki.source_type_to_string(wiki.UrlSource)
  |> should.equal("url")
}

pub fn source_type_to_string_task_test() {
  wiki.source_type_to_string(wiki.TaskSource)
  |> should.equal("task")
}

pub fn source_type_to_string_github_test() {
  wiki.source_type_to_string(wiki.GithubSource)
  |> should.equal("github")
}

pub fn source_type_from_string_chat_test() {
  wiki.source_type_from_string("chat")
  |> should.be_ok()
  |> should.equal(wiki.ChatSource)
}

pub fn source_type_from_string_manual_test() {
  wiki.source_type_from_string("manual")
  |> should.be_ok()
  |> should.equal(wiki.ManualSource)
}

pub fn source_type_from_string_url_test() {
  wiki.source_type_from_string("url")
  |> should.be_ok()
  |> should.equal(wiki.UrlSource)
}

pub fn source_type_from_string_task_test() {
  wiki.source_type_from_string("task")
  |> should.be_ok()
  |> should.equal(wiki.TaskSource)
}

pub fn source_type_from_string_github_test() {
  wiki.source_type_from_string("github")
  |> should.be_ok()
  |> should.equal(wiki.GithubSource)
}

pub fn source_type_from_string_invalid_test() {
  wiki.source_type_from_string("invalid")
  |> should.be_error()
}

pub fn source_type_from_string_empty_test() {
  wiki.source_type_from_string("")
  |> should.be_error()
}

// =============================================================================
// IngestContentRequest decoding tests
// =============================================================================

pub fn decode_ingest_content_request_test() {
  let json_str = "{\"title\": \"Test Entry\", \"content\": \"Some content here\"}"

  wiki.decode_ingest_content_request(json_str)
  |> should.be_ok()
  |> fn(req: wiki.IngestContentRequest) {
    should.equal(req.title, "Test Entry")
    should.equal(req.content, "Some content here")
  }
}

pub fn decode_ingest_content_request_missing_title_test() {
  let json_str = "{\"content\": \"Some content\"}"

  wiki.decode_ingest_content_request(json_str)
  |> should.be_error()
}

pub fn decode_ingest_content_request_missing_content_test() {
  let json_str = "{\"title\": \"Test Entry\"}"

  wiki.decode_ingest_content_request(json_str)
  |> should.be_error()
}

pub fn decode_ingest_content_request_empty_fields_test() {
  let json_str = "{\"title\": \"\", \"content\": \"\"}"

  wiki.decode_ingest_content_request(json_str)
  |> should.be_ok()
  |> fn(req: wiki.IngestContentRequest) {
    should.equal(req.title, "")
    should.equal(req.content, "")
  }
}

pub fn decode_ingest_content_request_invalid_json_test() {
  wiki.decode_ingest_content_request("not json")
  |> should.be_error()
}

// =============================================================================
// IngestUrlRequest decoding tests
// =============================================================================

pub fn decode_ingest_url_request_test() {
  let json_str = "{\"url\": \"https://example.com/docs\"}"

  wiki.decode_ingest_url_request(json_str)
  |> should.be_ok()
  |> fn(req: wiki.IngestUrlRequest) {
    should.equal(req.url, "https://example.com/docs")
  }
}

pub fn decode_ingest_url_request_missing_url_test() {
  let json_str = "{}"

  wiki.decode_ingest_url_request(json_str)
  |> should.be_error()
}

pub fn decode_ingest_url_request_invalid_json_test() {
  wiki.decode_ingest_url_request("not json")
  |> should.be_error()
}

// =============================================================================
// WikiEntry to_json tests
// =============================================================================

pub fn wiki_entry_to_json_test() {
  let entry =
    wiki.WikiEntry(
      id: "entry-id",
      title: "Test Entry",
      content: "Some wiki content",
      source_type: wiki.ManualSource,
      source_id: None,
      source_url: None,
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    wiki.to_json(entry)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"entry-id\""))
  should.be_true(string.contains(json_str, "\"title\":\"Test Entry\""))
  should.be_true(string.contains(json_str, "\"source_type\":\"manual\""))
  should.be_true(string.contains(json_str, "\"source_id\":null"))
}

pub fn wiki_entry_to_json_with_source_test() {
  let entry =
    wiki.WikiEntry(
      id: "entry-id",
      title: "From Chat",
      content: "Extracted content",
      source_type: wiki.ChatSource,
      source_id: Some("chat-123"),
      source_url: None,
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    wiki.to_json(entry)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"source_type\":\"chat\""))
  should.be_true(string.contains(json_str, "\"source_id\":\"chat-123\""))
}

pub fn wiki_entry_to_json_url_source_test() {
  let entry =
    wiki.WikiEntry(
      id: "entry-id",
      title: "From URL",
      content: "Fetched content",
      source_type: wiki.UrlSource,
      source_id: None,
      source_url: Some("https://example.com/docs"),
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    wiki.to_json(entry)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"source_type\":\"url\""))
  should.be_true(string.contains(
    json_str,
    "\"source_url\":\"https://example.com/docs\"",
  ))
}

// =============================================================================
// WikiChunk to_json tests
// =============================================================================

pub fn wiki_chunk_to_json_test() {
  let chunk =
    wiki.WikiChunk(
      id: "chunk-id",
      wiki_entry_id: "entry-id",
      chunk_index: 0,
      content: "First chunk of content",
      token_count: Some(50),
      created_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    wiki.chunk_to_json(chunk)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"id\":\"chunk-id\""))
  should.be_true(string.contains(json_str, "\"wiki_entry_id\":\"entry-id\""))
  should.be_true(string.contains(json_str, "\"chunk_index\":0"))
  should.be_true(string.contains(json_str, "\"token_count\":50"))
}

pub fn wiki_chunk_to_json_no_token_count_test() {
  let chunk =
    wiki.WikiChunk(
      id: "chunk-id",
      wiki_entry_id: "entry-id",
      chunk_index: 1,
      content: "Second chunk",
      token_count: None,
      created_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    wiki.chunk_to_json(chunk)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"token_count\":null"))
}

// =============================================================================
// WikiSearchResult to_json tests
// =============================================================================

pub fn wiki_search_result_to_json_test() {
  let entry =
    wiki.WikiEntry(
      id: "entry-id",
      title: "Search Result",
      content: "Full content",
      source_type: wiki.ManualSource,
      source_id: None,
      source_url: None,
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let result =
    wiki.WikiSearchResult(
      entry: entry,
      chunk: "Relevant chunk text",
      relevance_score: 0.85,
    )

  let json_str =
    wiki.search_result_to_json(result)
    |> json.to_string()

  should.be_true(string.contains(json_str, "\"entry\":"))
  should.be_true(string.contains(json_str, "\"chunk\":\"Relevant chunk text\""))
  should.be_true(string.contains(json_str, "\"relevance_score\":0.85"))
}
