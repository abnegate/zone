import gleam/list
import gleam/string
import gleeunit/should
import ollama_models

// =============================================================================
// get_all_models tests
// =============================================================================

pub fn get_all_models_not_empty_test() {
  ollama_models.get_all_models()
  |> list.length()
  |> fn(len) { len > 0 }
  |> should.be_true()
}

pub fn get_all_models_count_test() {
  // Should have 200+ models
  ollama_models.get_all_models()
  |> list.length()
  |> fn(len) { len >= 200 }
  |> should.be_true()
}

pub fn get_all_models_has_llama_test() {
  ollama_models.get_all_models()
  |> list.find(fn(m) { m.name == "llama3.1" })
  |> should.be_ok()
}

pub fn get_all_models_has_qwen_test() {
  ollama_models.get_all_models()
  |> list.find(fn(m) { m.name == "qwen2.5" })
  |> should.be_ok()
}

pub fn get_all_models_has_mistral_test() {
  ollama_models.get_all_models()
  |> list.find(fn(m) { m.name == "mistral" })
  |> should.be_ok()
}

pub fn get_all_models_has_deepseek_test() {
  ollama_models.get_all_models()
  |> list.find(fn(m) { m.name == "deepseek-r1" })
  |> should.be_ok()
}

// =============================================================================
// Model data integrity tests
// =============================================================================

pub fn models_have_names_test() {
  ollama_models.get_all_models()
  |> list.all(fn(m) { string.length(m.name) > 0 })
  |> should.be_true()
}

pub fn models_have_descriptions_test() {
  ollama_models.get_all_models()
  |> list.all(fn(m) { string.length(m.description) > 0 })
  |> should.be_true()
}

pub fn models_have_pulls_test() {
  ollama_models.get_all_models()
  |> list.all(fn(m) { m.pulls > 0 })
  |> should.be_true()
}

pub fn models_have_tags_test() {
  ollama_models.get_all_models()
  |> list.all(fn(m) { m.tags != [] })
  |> should.be_true()
}

// =============================================================================
// Sorting tests (models should be sorted by popularity)
// =============================================================================

pub fn models_sorted_by_pulls_test() {
  let models = ollama_models.get_all_models()

  // Check that the list is sorted in descending order by pulls
  let is_sorted = check_sorted_descending(models)

  is_sorted |> should.be_true()
}

fn check_sorted_descending(models: List(ollama_models.OllamaModel)) -> Bool {
  case models {
    [] -> True
    [_] -> True
    [first, second, ..rest] ->
      case first.pulls >= second.pulls {
        True -> check_sorted_descending([second, ..rest])
        False -> False
      }
  }
}

// =============================================================================
// Tag category tests
// =============================================================================

pub fn code_models_have_code_tag_test() {
  let code_models = ollama_models.get_all_models()
    |> list.filter(fn(m) {
      string.contains(m.name, "code") ||
      string.contains(m.name, "coder") ||
      string.contains(m.name, "starcoder")
    })

  // Most code-related models should have the "code" tag
  let tagged_count = list.count(code_models, fn(m) { list.contains(m.tags, "code") })
  let total_count = list.length(code_models)

  // At least 80% should be tagged correctly
  case total_count > 0 {
    True -> tagged_count * 100 / total_count >= 80
    False -> True
  }
  |> should.be_true()
}

pub fn embedding_models_have_embedding_tag_test() {
  let embedding_models = ollama_models.get_all_models()
    |> list.filter(fn(m) {
      string.contains(m.name, "embed") ||
      string.contains(m.name, "bge")
    })

  // Embedding models should have the "embedding" tag
  let tagged_count = list.count(embedding_models, fn(m) { list.contains(m.tags, "embedding") })
  let total_count = list.length(embedding_models)

  case total_count > 0 {
    True -> tagged_count * 100 / total_count >= 80
    False -> True
  }
  |> should.be_true()
}

pub fn vision_models_have_vision_tag_test() {
  let vision_models = ollama_models.get_all_models()
    |> list.filter(fn(m) {
      string.contains(m.name, "vision") ||
      string.contains(m.name, "llava") ||
      string.contains(m.name, "-vl")
    })

  let tagged_count = list.count(vision_models, fn(m) { list.contains(m.tags, "vision") })
  let total_count = list.length(vision_models)

  case total_count > 0 {
    True -> tagged_count * 100 / total_count >= 80
    False -> True
  }
  |> should.be_true()
}

// =============================================================================
// Model name uniqueness test
// =============================================================================

pub fn model_names_are_unique_test() {
  let models = ollama_models.get_all_models()
  let names = list.map(models, fn(m) { m.name })
  let unique_names = list.unique(names)

  list.length(names) |> should.equal(list.length(unique_names))
}

// =============================================================================
// Popular models have high pull counts
// =============================================================================

pub fn popular_models_have_high_pulls_test() {
  // Top models like llama, mistral, qwen should have millions of pulls
  let popular_models = ["llama3.1", "mistral", "qwen2.5", "gemma2", "phi3"]

  let all_models = ollama_models.get_all_models()

  list.all(popular_models, fn(name) {
    case list.find(all_models, fn(m) { m.name == name }) {
      Ok(model) -> model.pulls >= 1_000_000
      Error(_) -> False
    }
  })
  |> should.be_true()
}
