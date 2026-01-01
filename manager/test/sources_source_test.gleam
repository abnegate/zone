/// Tests for sources/source.gleam - Common types and JSON serialization
import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import sources/source.{BrowseModel, BrowseResult, ModelInfo}

// =============================================================================
// BrowseModel JSON serialization tests
// =============================================================================

pub fn browse_model_to_json_basic_test() {
  let model =
    BrowseModel(
      id: "test-model",
      name: "Test Model",
      description: "A test model",
      downloads: 1000,
      tags: ["tag1", "tag2"],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let json_str = source.browse_model_to_json(model) |> json.to_string

  json_str |> string.contains("\"id\":\"test-model\"") |> should.be_true()
  json_str |> string.contains("\"name\":\"Test Model\"") |> should.be_true()
  json_str
  |> string.contains("\"description\":\"A test model\"")
  |> should.be_true()
  json_str |> string.contains("\"downloads\":1000") |> should.be_true()
  json_str
  |> string.contains("\"tags\":[\"tag1\",\"tag2\"]")
  |> should.be_true()
}

pub fn browse_model_to_json_with_optional_fields_test() {
  let model =
    BrowseModel(
      id: "hf-model",
      name: "HF Model",
      description: "A HuggingFace model",
      downloads: 50_000,
      tags: ["gguf", "llama"],
      install_name: Some("hf.co/author/model"),
      author: Some("TestAuthor"),
      likes: Some(250),
      last_modified: Some("2024-01-15"),
      url: Some("https://huggingface.co/author/model"),
    )

  let json_str = source.browse_model_to_json(model) |> json.to_string

  json_str
  |> string.contains("\"install_name\":\"hf.co/author/model\"")
  |> should.be_true()
  json_str |> string.contains("\"author\":\"TestAuthor\"") |> should.be_true()
  json_str |> string.contains("\"likes\":250") |> should.be_true()
  json_str
  |> string.contains("\"last_modified\":\"2024-01-15\"")
  |> should.be_true()
  json_str
  |> string.contains("\"url\":\"https://huggingface.co/author/model\"")
  |> should.be_true()
}

pub fn browse_model_to_json_empty_tags_test() {
  let model =
    BrowseModel(
      id: "no-tags",
      name: "No Tags Model",
      description: "Model without tags",
      downloads: 100,
      tags: [],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let json_str = source.browse_model_to_json(model) |> json.to_string

  json_str |> string.contains("\"tags\":[]") |> should.be_true()
}

// =============================================================================
// BrowseResult JSON serialization tests
// =============================================================================

pub fn browse_result_to_json_empty_test() {
  let result =
    BrowseResult(source: "test", models: [], total: Some(0), has_more: False)

  let json_str = source.browse_result_to_json(result)

  json_str |> string.contains("\"source\":\"test\"") |> should.be_true()
  json_str |> string.contains("\"models\":[]") |> should.be_true()
  json_str |> string.contains("\"total\":0") |> should.be_true()
  json_str |> string.contains("\"has_more\":false") |> should.be_true()
}

pub fn browse_result_to_json_with_models_test() {
  let model1 =
    BrowseModel(
      id: "model1",
      name: "Model 1",
      description: "First model",
      downloads: 1000,
      tags: ["tag1"],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let model2 =
    BrowseModel(
      id: "model2",
      name: "Model 2",
      description: "Second model",
      downloads: 2000,
      tags: ["tag2"],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let result =
    BrowseResult(
      source: "huggingface",
      models: [model1, model2],
      total: Some(100),
      has_more: True,
    )

  let json_str = source.browse_result_to_json(result)

  json_str |> string.contains("\"source\":\"huggingface\"") |> should.be_true()
  json_str |> string.contains("\"total\":100") |> should.be_true()
  json_str |> string.contains("\"has_more\":true") |> should.be_true()
  json_str |> string.contains("\"id\":\"model1\"") |> should.be_true()
  json_str |> string.contains("\"id\":\"model2\"") |> should.be_true()
}

pub fn browse_result_to_json_no_total_test() {
  let result =
    BrowseResult(source: "huggingface", models: [], total: None, has_more: True)

  let json_str = source.browse_result_to_json(result)

  // total should be null when None
  json_str |> string.contains("\"total\":null") |> should.be_true()
}

// =============================================================================
// ModelInfo JSON serialization tests
// =============================================================================

pub fn model_info_to_json_minimal_test() {
  let info =
    ModelInfo(
      id: "test-id",
      name: "Test Model",
      description: None,
      readme: None,
      size_bytes: None,
      downloads: None,
      author: None,
      url: None,
    )

  let json_str = source.model_info_to_json(info)

  json_str |> string.contains("\"success\":true") |> should.be_true()
  json_str |> string.contains("\"id\":\"test-id\"") |> should.be_true()
  json_str |> string.contains("\"name\":\"Test Model\"") |> should.be_true()
  json_str |> string.contains("\"description\":null") |> should.be_true()
  json_str |> string.contains("\"content\":null") |> should.be_true()
  json_str |> string.contains("\"gguf_size\":null") |> should.be_true()
}

pub fn model_info_to_json_full_test() {
  let info =
    ModelInfo(
      id: "author/model",
      name: "Full Model",
      description: Some("A complete model"),
      readme: Some("# Model README\n\nThis is a model."),
      size_bytes: Some(5_000_000_000),
      downloads: Some(100_000),
      author: Some("TestAuthor"),
      url: Some("https://example.com/model"),
    )

  let json_str = source.model_info_to_json(info)

  json_str |> string.contains("\"success\":true") |> should.be_true()
  json_str
  |> string.contains("\"description\":\"A complete model\"")
  |> should.be_true()
  json_str
  |> string.contains("\"content\":\"# Model README")
  |> should.be_true()
  json_str |> string.contains("\"gguf_size\":5000000000") |> should.be_true()
  json_str |> string.contains("\"downloads\":100000") |> should.be_true()
  json_str |> string.contains("\"author\":\"TestAuthor\"") |> should.be_true()
  json_str
  |> string.contains("\"url\":\"https://example.com/model\"")
  |> should.be_true()
}

// =============================================================================
// Edge cases and special characters
// =============================================================================

pub fn browse_model_special_characters_test() {
  let model =
    BrowseModel(
      id: "model/with/slashes",
      name: "Model \"Quoted\"",
      description: "Description with\nnewline",
      downloads: 0,
      tags: ["tag:special"],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let json_str = source.browse_model_to_json(model) |> json.to_string

  // Should properly escape special characters
  json_str |> string.contains("model/with/slashes") |> should.be_true()
  json_str |> string.contains("\\\"Quoted\\\"") |> should.be_true()
}

pub fn browse_model_unicode_test() {
  let model =
    BrowseModel(
      id: "unicode-model",
      name: "模型名称",
      description: "中文描述 with émojis 🤖",
      downloads: 500,
      tags: ["中文", "日本語"],
      install_name: None,
      author: None,
      likes: None,
      last_modified: None,
      url: None,
    )

  let json_str = source.browse_model_to_json(model) |> json.to_string

  // Unicode should be preserved
  json_str |> string.contains("模型名称") |> should.be_true()
  json_str |> string.contains("中文描述") |> should.be_true()
}

pub fn model_info_large_size_test() {
  let info =
    ModelInfo(
      id: "large-model",
      name: "Large Model",
      description: None,
      readme: None,
      size_bytes: Some(100_000_000_000),
      // 100GB
      downloads: Some(1_000_000_000),
      // 1 billion
      author: None,
      url: None,
    )

  let json_str = source.model_info_to_json(info)

  json_str |> string.contains("\"gguf_size\":100000000000") |> should.be_true()
  json_str |> string.contains("\"downloads\":1000000000") |> should.be_true()
}
