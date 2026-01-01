/// Tests for sources/ollama_library.gleam - Ollama Library source
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import sources/ollama_library

// =============================================================================
// browse() function tests
// =============================================================================

pub fn browse_returns_ok_test() {
  ollama_library.browse("", 20, 0)
  |> should.be_ok()
}

pub fn browse_returns_models_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.length()
  |> fn(len) { len > 0 }
  |> should.be_true()
}

pub fn browse_has_correct_source_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.source |> should.equal("ollama")
}

pub fn browse_respects_limit_test() {
  let assert Ok(result) = ollama_library.browse("", 5, 0)

  result.models
  |> list.length()
  |> should.equal(5)
}

pub fn browse_respects_offset_test() {
  let assert Ok(result1) = ollama_library.browse("", 5, 0)
  let assert Ok(result2) = ollama_library.browse("", 5, 5)

  // First model of page 2 should be different from first model of page 1
  case result1.models, result2.models {
    [first1, ..], [first2, ..] -> first1.id |> should.not_equal(first2.id)
    _, _ -> should.fail()
  }
}

pub fn browse_returns_total_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.total |> should.be_some()
  let assert Some(total) = result.total
  total |> fn(t) { t > 100 } |> should.be_true()
}

pub fn browse_has_more_when_more_results_test() {
  let assert Ok(result) = ollama_library.browse("", 5, 0)

  result.has_more |> should.be_true()
}

pub fn browse_no_more_at_end_test() {
  // Request beyond total models
  let assert Ok(result) = ollama_library.browse("", 20, 10_000)

  result.has_more |> should.be_false()
  result.models |> list.length() |> should.equal(0)
}

// =============================================================================
// browse() search functionality tests
// =============================================================================

pub fn browse_search_filters_results_test() {
  let assert Ok(all_result) = ollama_library.browse("", 100, 0)
  let assert Ok(search_result) = ollama_library.browse("llama", 100, 0)

  // Search results should be fewer than all results
  list.length(search_result.models)
  |> fn(len) { len < list.length(all_result.models) }
  |> should.be_true()
}

pub fn browse_search_matches_name_test() {
  let assert Ok(result) = ollama_library.browse("llama", 100, 0)

  // All results should contain "llama" in name, description, or tags
  result.models
  |> list.all(fn(m) {
    string.contains(string.lowercase(m.name), "llama")
    || string.contains(string.lowercase(m.description), "llama")
    || list.any(m.tags, fn(tag) {
      string.contains(string.lowercase(tag), "llama")
    })
  })
  |> should.be_true()
}

pub fn browse_search_case_insensitive_test() {
  let assert Ok(lower_result) = ollama_library.browse("llama", 100, 0)
  let assert Ok(upper_result) = ollama_library.browse("LLAMA", 100, 0)

  list.length(lower_result.models)
  |> should.equal(list.length(upper_result.models))
}

pub fn browse_search_no_results_test() {
  let assert Ok(result) = ollama_library.browse("xyznonexistent123", 20, 0)

  result.models |> list.length() |> should.equal(0)
  result.has_more |> should.be_false()
}

// =============================================================================
// browse() model data tests
// =============================================================================

pub fn browse_models_have_required_fields_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.all(fn(m) {
    string.length(m.id) > 0
    && string.length(m.name) > 0
    && string.length(m.description) > 0
    && m.downloads > 0
  })
  |> should.be_true()
}

pub fn browse_models_have_install_name_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.all(fn(m) {
    case m.install_name {
      Some(name) -> string.length(name) > 0
      None -> False
    }
  })
  |> should.be_true()
}

pub fn browse_models_have_url_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.all(fn(m) {
    case m.url {
      Some(url) -> string.starts_with(url, "https://ollama.com/library/")
      None -> False
    }
  })
  |> should.be_true()
}

pub fn browse_models_have_no_author_test() {
  // Ollama library models don't have authors
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.all(fn(m) { m.author == None })
  |> should.be_true()
}

pub fn browse_models_have_tags_test() {
  let assert Ok(result) = ollama_library.browse("", 20, 0)

  result.models
  |> list.all(fn(m) { list.length(m.tags) > 0 })
  |> should.be_true()
}

// =============================================================================
// get_info() function tests
// =============================================================================

pub fn get_info_returns_ok_for_existing_model_test() {
  ollama_library.get_info("llama3.1")
  |> should.be_ok()
}

pub fn get_info_returns_error_for_nonexistent_model_test() {
  ollama_library.get_info("nonexistent-model-xyz")
  |> should.be_error()
}

pub fn get_info_has_correct_id_test() {
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.id |> should.equal("llama3.1")
}

pub fn get_info_has_name_test() {
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.name |> should.equal("llama3.1")
}

pub fn get_info_has_description_test() {
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.description |> should.be_some()
  let assert Some(desc) = info.description
  desc |> string.length() |> fn(len) { len > 0 } |> should.be_true()
}

pub fn get_info_has_downloads_test() {
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.downloads |> should.be_some()
  let assert Some(downloads) = info.downloads
  downloads |> fn(d) { d > 0 } |> should.be_true()
}

pub fn get_info_has_url_test() {
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.url |> should.be_some()
  let assert Some(url) = info.url
  url |> should.equal("https://ollama.com/library/llama3.1")
}

pub fn get_info_no_readme_test() {
  // Ollama library doesn't fetch README
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.readme |> should.be_none()
}

pub fn get_info_no_size_test() {
  // Ollama library doesn't have size info
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.size_bytes |> should.be_none()
}

pub fn get_info_no_author_test() {
  // Ollama library models don't have authors
  let assert Ok(info) = ollama_library.get_info("llama3.1")

  info.author |> should.be_none()
}

// =============================================================================
// Popular models tests
// =============================================================================

pub fn browse_includes_llama_test() {
  let assert Ok(result) = ollama_library.browse("llama", 100, 0)

  result.models
  |> list.find(fn(m) { m.name == "llama3.1" })
  |> should.be_ok()
}

pub fn browse_includes_mistral_test() {
  let assert Ok(result) = ollama_library.browse("mistral", 100, 0)

  result.models
  |> list.find(fn(m) { m.name == "mistral" })
  |> should.be_ok()
}

pub fn browse_includes_qwen_test() {
  let assert Ok(result) = ollama_library.browse("qwen", 100, 0)

  result.models
  |> list.find(fn(m) { string.contains(m.name, "qwen") })
  |> should.be_ok()
}

// =============================================================================
// Pagination consistency tests
// =============================================================================

pub fn browse_pagination_no_duplicates_test() {
  let assert Ok(page1) = ollama_library.browse("", 10, 0)
  let assert Ok(page2) = ollama_library.browse("", 10, 10)

  let page1_ids = list.map(page1.models, fn(m) { m.id })
  let page2_ids = list.map(page2.models, fn(m) { m.id })

  // No overlap between pages
  list.all(page1_ids, fn(id) { !list.contains(page2_ids, id) })
  |> should.be_true()
}

pub fn browse_pagination_covers_all_test() {
  let assert Ok(result) = ollama_library.browse("", 100, 0)
  let assert Some(total) = result.total

  // If we paginate through all, we should get the total
  let assert Ok(all_at_once) = ollama_library.browse("", total + 10, 0)

  list.length(all_at_once.models) |> should.equal(total)
}
