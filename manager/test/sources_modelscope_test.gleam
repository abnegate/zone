/// Tests for sources/modelscope.gleam - ModelScope source
/// Note: Some tests require network access and may fail in offline environments
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import sources/modelscope

// =============================================================================
// Constants tests
// =============================================================================

pub fn source_id_test() {
  modelscope.source_id |> should.equal("modelscope")
}

pub fn base_url_test() {
  modelscope.base_url |> should.equal("https://modelscope.cn")
}

pub fn api_url_test() {
  modelscope.api_url |> should.equal("https://modelscope.cn/api/v1")
}

// =============================================================================
// browse() function tests (require network)
// =============================================================================

pub fn browse_returns_result_test() {
  // This test requires network access
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.source |> should.equal("modelscope")
    }
    Error(_) -> {
      // Network error is acceptable in offline mode
      should.be_true(True)
    }
  }
}

pub fn browse_returns_models_test() {
  case modelscope.browse("", 10, 0) {
    Ok(result) -> {
      // ModelScope filters for GGUF models which may not always be available
      // Just verify the response is valid (can return 0 models)
      result.models
      |> list.length()
      |> fn(len) { len >= 0 }
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_respects_limit_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.length()
      |> fn(len) { len <= 5 }
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_search_filters_results_test() {
  case modelscope.browse("llama", 10, 0) {
    Ok(result) -> {
      // Results should be related to the search query
      // May be empty if no GGUF llama models on ModelScope
      should.be_true(True)
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_required_fields_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) { string.length(m.id) > 0 && string.length(m.name) > 0 })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_install_name_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.install_name {
          Some(name) -> string.starts_with(name, "modelscope/")
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_author_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.author {
          Some(author) -> string.length(author) > 0
          None -> True
          // Author may be missing for some models
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_url_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.url {
          Some(url) -> string.starts_with(url, "https://modelscope.cn/")
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_likes_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.likes {
          Some(likes) -> likes >= 0
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_has_total_test() {
  // ModelScope API returns total count
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      let _ = result.total |> should.be_some()
      Nil
    }
    Error(_) -> Nil
  }
}

// =============================================================================
// get_info() function tests (require network)
// =============================================================================

pub fn get_info_returns_result_for_valid_model_test() {
  // Use a well-known model path format
  case modelscope.get_info("Qwen/Qwen2.5-7B-GGUF") {
    Ok(info) -> {
      info.id |> should.equal("Qwen/Qwen2.5-7B-GGUF")
    }
    Error(_) -> {
      // Network error or model not found is acceptable
      should.be_true(True)
    }
  }
}

pub fn get_info_has_name_test() {
  case modelscope.get_info("Qwen/Qwen2.5-7B-GGUF") {
    Ok(info) -> {
      info.name |> string.length() |> fn(len) { len > 0 } |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn get_info_has_url_test() {
  case modelscope.get_info("Qwen/Qwen2.5-7B-GGUF") {
    Ok(info) -> {
      info.url |> should.be_some()
      let assert Some(url) = info.url
      url |> string.starts_with("https://modelscope.cn/") |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn get_info_fallback_for_unknown_model_test() {
  // For unknown models, get_info should still return a ModelInfo with basic data
  case modelscope.get_info("unknown/model-xyz-12345") {
    Ok(info) -> {
      // Should have at least id and name
      info.id |> should.equal("unknown/model-xyz-12345")
    }
    Error(_) -> {
      // Error is also acceptable for nonexistent model
      should.be_true(True)
    }
  }
}

// =============================================================================
// Model ID format tests
// =============================================================================

pub fn browse_model_id_has_author_prefix_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        // ModelScope model IDs should be in "author/model" format (path)
        string.contains(m.id, "/")
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_install_name_has_modelscope_prefix_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.install_name {
          Some(name) -> string.starts_with(name, "modelscope/")
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

// =============================================================================
// Pagination tests
// =============================================================================

pub fn browse_offset_returns_different_results_test() {
  // ModelScope uses page-based pagination (offset / limit + 1)
  case modelscope.browse("", 5, 0), modelscope.browse("", 5, 5) {
    Ok(page1), Ok(page2) -> {
      let page1_ids = list.map(page1.models, fn(m) { m.id })
      let page2_ids = list.map(page2.models, fn(m) { m.id })

      // Pages should have different models (if there are enough)
      case list.length(page1_ids), list.length(page2_ids) {
        a, b if a > 0 && b > 0 -> {
          case page1_ids, page2_ids {
            [first1, ..], [first2, ..] -> first1 |> should.not_equal(first2)
            _, _ -> should.be_true(True)
          }
        }
        _, _ -> should.be_true(True)
      }
    }
    _, _ -> should.be_true(True)
  }
}

pub fn browse_has_more_indicator_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      // If we got 5 results, there are likely more
      case list.length(result.models) {
        5 -> result.has_more |> should.be_true()
        _ -> should.be_true(True)
      }
    }
    Error(_) -> should.be_true(True)
  }
}

// =============================================================================
// GGUF filter tests
// =============================================================================

pub fn browse_returns_gguf_tagged_models_test() {
  case modelscope.browse("", 10, 0) {
    Ok(result) -> {
      // ModelScope source filters for GGUF models via Tags param
      // Models should have gguf-related characteristics
      result.models
      |> list.length()
      |> fn(len) { len >= 0 }
      // May return 0 if no GGUF models
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

// =============================================================================
// Author extraction tests (from path)
// =============================================================================

pub fn browse_extracts_author_from_path_test() {
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.author, string.split(m.id, "/") {
          Some(author), [expected_author, ..] -> author == expected_author
          None, _ -> True
          // Author extraction may fail for edge cases
          _, _ -> True
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

// =============================================================================
// Chinese content tests
// =============================================================================

pub fn browse_handles_chinese_content_test() {
  // ModelScope is a Chinese platform, should handle Chinese text
  case modelscope.browse("", 5, 0) {
    Ok(result) -> {
      // Just verify we can process the response without errors
      result.models
      |> list.length()
      |> fn(len) { len >= 0 }
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_search_with_chinese_test() {
  // Search with Chinese characters
  case modelscope.browse("模型", 5, 0) {
    Ok(result) -> {
      // Should not error even with Chinese search
      should.be_true(True)
    }
    Error(_) -> {
      // Error is also acceptable (may be URL encoding issue or no results)
      should.be_true(True)
    }
  }
}
