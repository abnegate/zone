import gleam/list
import gleeunit/should
import scrape_models

// =============================================================================
// escape_string tests
// =============================================================================

pub fn escape_string_no_special_chars_test() {
  scrape_models.escape_string("hello world")
  |> should.equal("hello world")
}

pub fn escape_string_backslash_test() {
  scrape_models.escape_string("path\\to\\file")
  |> should.equal("path\\\\to\\\\file")
}

pub fn escape_string_quotes_test() {
  scrape_models.escape_string("say \"hello\"")
  |> should.equal("say \\\"hello\\\"")
}

pub fn escape_string_mixed_test() {
  scrape_models.escape_string("path\\to\\\"file\"")
  |> should.equal("path\\\\to\\\\\\\"file\\\"")
}

pub fn escape_string_empty_test() {
  scrape_models.escape_string("")
  |> should.equal("")
}

pub fn escape_string_only_backslashes_test() {
  scrape_models.escape_string("\\\\\\")
  |> should.equal("\\\\\\\\\\\\")
}

pub fn escape_string_only_quotes_test() {
  scrape_models.escape_string("\"\"\"")
  |> should.equal("\\\"\\\"\\\"")
}

// =============================================================================
// format_number tests
// =============================================================================

pub fn format_number_small_test() {
  scrape_models.format_number(123)
  |> should.equal("123")
}

pub fn format_number_thousands_test() {
  scrape_models.format_number(1234)
  |> should.equal("1_234")
}

pub fn format_number_millions_test() {
  scrape_models.format_number(1_234_567)
  |> should.equal("1_234_567")
}

pub fn format_number_billions_test() {
  scrape_models.format_number(1_234_567_890)
  |> should.equal("1_234_567_890")
}

pub fn format_number_zero_test() {
  scrape_models.format_number(0)
  |> should.equal("0")
}

pub fn format_number_exact_thousands_test() {
  scrape_models.format_number(1000)
  |> should.equal("1_000")
}

pub fn format_number_exact_millions_test() {
  scrape_models.format_number(1_000_000)
  |> should.equal("1_000_000")
}

pub fn format_number_two_digits_test() {
  scrape_models.format_number(99)
  |> should.equal("99")
}

pub fn format_number_three_digits_test() {
  scrape_models.format_number(999)
  |> should.equal("999")
}

pub fn format_number_four_digits_test() {
  scrape_models.format_number(9999)
  |> should.equal("9_999")
}

// =============================================================================
// categorize tests
// =============================================================================

pub fn categorize_code_model_test() {
  scrape_models.categorize("codellama", "Code Llama - code generation")
  |> list.contains("code")
  |> should.be_true()
}

pub fn categorize_embedding_model_test() {
  scrape_models.categorize("nomic-embed-text", "High-performing embeddings")
  |> list.contains("embedding")
  |> should.be_true()
}

pub fn categorize_vision_model_test() {
  scrape_models.categorize("llava", "Vision encoder with language")
  |> list.contains("vision")
  |> should.be_true()
}

pub fn categorize_reasoning_model_test() {
  scrape_models.categorize("deepseek-r1", "Open reasoning model")
  |> list.contains("reasoning")
  |> should.be_true()
}

pub fn categorize_small_model_test() {
  scrape_models.categorize("tinyllama", "TinyLlama 1.1B")
  |> list.contains("small")
  |> should.be_true()
}

pub fn categorize_large_model_test() {
  scrape_models.categorize("llama3.3", "70B state-of-the-art model")
  |> list.contains("large")
  |> should.be_true()
}

pub fn categorize_uncensored_model_test() {
  scrape_models.categorize("dolphin-mistral", "Uncensored coding model")
  |> list.contains("uncensored")
  |> should.be_true()
}

pub fn categorize_multilingual_model_test() {
  scrape_models.categorize("aya", "23 languages by Cohere")
  |> list.contains("multilingual")
  |> should.be_true()
}

pub fn categorize_moe_model_test() {
  scrape_models.categorize("mixtral", "MoE mixture of experts")
  |> list.contains("moe")
  |> should.be_true()
}

pub fn categorize_tools_model_test() {
  scrape_models.categorize("llama3-groq-tool-use", "Function calling model")
  |> list.contains("tools")
  |> should.be_true()
}

pub fn categorize_medical_model_test() {
  scrape_models.categorize("meditron", "Medical domain model")
  |> list.contains("medical")
  |> should.be_true()
}

pub fn categorize_safety_model_test() {
  scrape_models.categorize("llama-guard3", "Safety classification")
  |> list.contains("safety")
  |> should.be_true()
}

pub fn categorize_sql_model_test() {
  scrape_models.categorize("sqlcoder", "SQL generation specialist")
  |> list.contains("sql")
  |> should.be_true()
}

pub fn categorize_default_chat_test() {
  // A generic model without specific category keywords should default to chat
  scrape_models.categorize("generic-model", "A general purpose language model")
  |> list.contains("chat")
  |> should.be_true()
}

pub fn categorize_code_not_chat_test() {
  // Code models should not have the chat tag
  let tags = scrape_models.categorize("codellama", "Code generation model")

  tags |> list.contains("code") |> should.be_true()
  tags |> list.contains("chat") |> should.be_false()
}

pub fn categorize_dolphincoder_not_uncensored_test() {
  // dolphincoder has "coder" so it should be code, not uncensored (because of the !has_any check)
  let tags = scrape_models.categorize("dolphincoder", "Uncensored coding model")

  // It has "coder" so it should be tagged as code
  tags |> list.contains("code") |> should.be_true()
  // And because it has "coder", it should NOT be tagged as uncensored
  // (per the logic: has_any(combined, ["uncensor", "dolphin"]) && !has_any(combined, ["coder"]))
  tags |> list.contains("uncensored") |> should.be_false()
}

pub fn categorize_multiple_tags_test() {
  // A model can have multiple tags
  let tags =
    scrape_models.categorize(
      "deepseek-coder-v2",
      "MoE coding model with reasoning",
    )

  // Should have both code and potentially other tags
  tags |> list.contains("code") |> should.be_true()
}

pub fn categorize_case_insensitive_test() {
  // Categorization should be case-insensitive
  scrape_models.categorize("CODELLAMA", "CODE GENERATION")
  |> list.contains("code")
  |> should.be_true()
}

// =============================================================================
// Integration tests - verify categories make sense
// =============================================================================

pub fn categorize_returns_non_empty_test() {
  // Any model should have at least one tag
  scrape_models.categorize("any-model", "any description")
  |> list.length()
  |> fn(len) { len > 0 }
  |> should.be_true()
}

pub fn categorize_known_code_models_test() {
  let code_models = [
    #("codellama", "Code Llama"),
    #("starcoder", "StarCoder"),
    #("codegemma", "CodeGemma"),
    #("deepseek-coder", "DeepSeek Coder"),
    #("wizardcoder", "WizardCoder"),
  ]

  list.all(code_models, fn(m) {
    let #(name, desc) = m
    scrape_models.categorize(name, desc) |> list.contains("code")
  })
  |> should.be_true()
}

pub fn categorize_known_embedding_models_test() {
  let embedding_models = [
    #("nomic-embed-text", "High-performing embeddings"),
    #("bge-m3", "Multi-functionality embeddings"),
    #("all-minilm", "Fast sentence embeddings"),
  ]

  list.all(embedding_models, fn(m) {
    let #(name, desc) = m
    scrape_models.categorize(name, desc) |> list.contains("embedding")
  })
  |> should.be_true()
}

pub fn categorize_known_vision_models_test() {
  let vision_models = [
    #("llava", "Vision encoder"),
    #("llama3.2-vision", "Llama Vision"),
    #("moondream", "Small vision for edge"),
  ]

  list.all(vision_models, fn(m) {
    let #(name, desc) = m
    scrape_models.categorize(name, desc) |> list.contains("vision")
  })
  |> should.be_true()
}
