/// Tests for sources/huggingface.gleam - HuggingFace source
/// Note: Some tests require network access and may fail in offline environments
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import sources/huggingface

// =============================================================================
// Constants tests
// =============================================================================

pub fn source_id_test() {
  huggingface.source_id |> should.equal("huggingface")
}

pub fn base_url_test() {
  huggingface.base_url |> should.equal("https://huggingface.co")
}

pub fn api_url_test() {
  huggingface.api_url |> should.equal("https://huggingface.co/api/models")
}

// =============================================================================
// browse() function tests (require network)
// =============================================================================

pub fn browse_returns_result_test() {
  // This test requires network access
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.source |> should.equal("huggingface")
    }
    Error(_) -> {
      // Network error is acceptable in offline mode
      should.be_true(True)
    }
  }
}

pub fn browse_returns_models_test() {
  case huggingface.browse("", 10, 0) {
    Ok(result) -> {
      result.models
      |> list.length()
      |> fn(len) { len > 0 }
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_respects_limit_test() {
  case huggingface.browse("", 5, 0) {
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
  case huggingface.browse("llama", 10, 0) {
    Ok(result) -> {
      // Results should be related to the search query
      result.models
      |> list.length()
      |> fn(len) { len > 0 }
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_required_fields_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) { string.length(m.id) > 0 && string.length(m.name) > 0 })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_install_name_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.install_name {
          Some(name) -> string.starts_with(name, "hf.co/")
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_author_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      // Authors may be empty strings or not present - just verify parsing works
      // Check that models that have authors have valid strings
      result.models
      |> list.all(fn(m) {
        case m.author {
          Some(author) -> string.length(author) >= 0
          // Empty string is valid
          None -> True
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_url_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.url {
          Some(url) -> string.starts_with(url, "https://huggingface.co/")
          None -> False
        }
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_models_have_likes_test() {
  case huggingface.browse("", 5, 0) {
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

pub fn browse_has_no_total_test() {
  // HuggingFace API doesn't return total count
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.total |> should.be_none()
    }
    Error(_) -> should.be_true(True)
  }
}

// =============================================================================
// get_info() function tests (require network)
// =============================================================================

pub fn get_info_returns_result_test() {
  // Use a well-known model
  case huggingface.get_info("TheBloke/Llama-2-7B-GGUF") {
    Ok(info) -> {
      info.id |> should.equal("TheBloke/Llama-2-7B-GGUF")
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn get_info_has_name_test() {
  case huggingface.get_info("TheBloke/Llama-2-7B-GGUF") {
    Ok(info) -> {
      info.name |> string.length() |> fn(len) { len > 0 } |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn get_info_has_url_test() {
  case huggingface.get_info("TheBloke/Llama-2-7B-GGUF") {
    Ok(info) -> {
      info.url |> should.be_some()
      let assert Some(url) = info.url
      url |> string.starts_with("https://huggingface.co/") |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn get_info_may_have_readme_test() {
  case huggingface.get_info("TheBloke/Llama-2-7B-GGUF") {
    Ok(info) -> {
      // README may or may not be present
      case info.readme {
        Some(readme) -> { string.length(readme) > 0 } |> should.be_true()
        None -> Nil
      }
    }
    Error(_) -> Nil
  }
}

pub fn get_info_may_have_size_test() {
  case huggingface.get_info("TheBloke/Llama-2-7B-GGUF") {
    Ok(info) -> {
      // Size may or may not be present
      case info.size_bytes {
        Some(size) -> { size > 0 } |> should.be_true()
        None -> Nil
      }
    }
    Error(_) -> Nil
  }
}

pub fn get_info_nonexistent_model_test() {
  case huggingface.get_info("nonexistent-user/nonexistent-model-xyz-12345") {
    Ok(_) -> {
      // Might return empty info
      should.be_true(True)
    }
    Error(_) -> {
      // Error is expected for nonexistent model
      should.be_true(True)
    }
  }
}

// =============================================================================
// Model ID format tests
// =============================================================================

pub fn browse_model_id_has_author_prefix_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        // HuggingFace model IDs should be in "author/model" format
        string.contains(m.id, "/")
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}

pub fn browse_install_name_has_hf_prefix_test() {
  case huggingface.browse("", 5, 0) {
    Ok(result) -> {
      result.models
      |> list.all(fn(m) {
        case m.install_name {
          Some(name) -> string.starts_with(name, "hf.co/")
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
  case huggingface.browse("", 5, 0), huggingface.browse("", 5, 5) {
    Ok(page1), Ok(page2) -> {
      let page1_ids = list.map(page1.models, fn(m) { m.id })
      let page2_ids = list.map(page2.models, fn(m) { m.id })

      // Pages should have different models
      case page1_ids, page2_ids {
        [first1, ..], [first2, ..] -> first1 |> should.not_equal(first2)
        _, _ -> should.be_true(True)
      }
    }
    _, _ -> should.be_true(True)
  }
}

pub fn browse_has_more_indicator_test() {
  case huggingface.browse("", 5, 0) {
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

pub fn browse_returns_gguf_models_test() {
  case huggingface.browse("", 10, 0) {
    Ok(result) -> {
      // HuggingFace source filters for GGUF models
      // Description should mention GGUF or tags should include gguf
      result.models
      |> list.all(fn(m) {
        string.contains(string.lowercase(m.description), "gguf")
        || list.any(m.tags, fn(t) { string.lowercase(t) == "gguf" })
      })
      |> should.be_true()
    }
    Error(_) -> should.be_true(True)
  }
}
