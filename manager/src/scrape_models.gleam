// =============================================================================
// Ollama Model Scraper
// =============================================================================
// Generates src/ollama_models.gleam from the model database
// Run: gleam run -m scrape_models
// =============================================================================

import gleam/int
import gleam/io
import gleam/list
import gleam/string
import simplifile

pub fn main() {
  io.println("Ollama Model Scraper")
  io.println("==================================================")

  let models = get_model_database()
  let sorted = list.sort(models, fn(a, b) { int.compare(b.pulls, a.pulls) })

  io.println("Total models: " <> int.to_string(list.length(sorted)))

  let content = generate_gleam_file(sorted)

  case simplifile.write("src/ollama_models.gleam", content) {
    Ok(_) -> {
      io.println("Generated src/ollama_models.gleam")
    }
    Error(err) -> {
      io.println("Error writing file: " <> string.inspect(err))
    }
  }
}

type Model {
  Model(name: String, description: String, pulls: Int, tags: List(String))
}

fn generate_gleam_file(models: List(Model)) -> String {
  let header =
    "// =============================================================================
// Ollama Library Models
// =============================================================================
// Auto-generated from model database
// Run: gleam run -m scrape_models
// =============================================================================

pub type OllamaModel {
  OllamaModel(
    name: String,
    description: String,
    pulls: Int,
    tags: List(String),
  )
}

pub fn get_all_models() -> List(OllamaModel) {
  [
"

  let footer =
    "  ]
}
"

  let model_lines =
    models
    |> list.index_map(fn(m, i) {
      let tags_str =
        m.tags
        |> list.map(fn(t) { "\"" <> t <> "\"" })
        |> string.join(", ")

      let comma = case i < list.length(models) - 1 {
        True -> ","
        False -> ""
      }

      "    OllamaModel(\""
      <> escape_string(m.name)
      <> "\", \""
      <> escape_string(m.description)
      <> "\", "
      <> format_number(m.pulls)
      <> ", ["
      <> tags_str
      <> "])"
      <> comma
    })
    |> string.join("\n")

  header <> model_lines <> "\n" <> footer
}

/// Escape special characters in strings for Gleam source code
/// @internal
pub fn escape_string(s: String) -> String {
  s
  |> string.replace("\\", "\\\\")
  |> string.replace("\"", "\\\"")
}

/// Format number with underscores for readability
/// @internal
pub fn format_number(n: Int) -> String {
  let s = int.to_string(n)
  format_with_underscores(s, "")
}

fn format_with_underscores(remaining: String, acc: String) -> String {
  let len = string.length(remaining)
  case len {
    0 -> acc
    1 | 2 | 3 -> remaining <> acc
    _ -> {
      let split_at = len - 3
      let #(left, right) = split_string_at(remaining, split_at)
      format_with_underscores(left, "_" <> right <> acc)
    }
  }
}

fn split_string_at(s: String, at: Int) -> #(String, String) {
  let left = string.slice(s, 0, at)
  let right = string.slice(s, at, string.length(s) - at)
  #(left, right)
}

fn has_any(text: String, keywords: List(String)) -> Bool {
  list.any(keywords, fn(kw) { string.contains(text, kw) })
}

/// Categorize a model based on its name and description
/// @internal
pub fn categorize(name: String, desc: String) -> List(String) {
  let combined = string.lowercase(name <> " " <> desc)
  let tags = []

  // Size tags
  let tags = case
    has_any(combined, [
      "tiny", "nano", "mini", "small", "compact", "lightweight", "0.5b", "1b",
      "1.5b", "2b", "3b",
    ])
  {
    True -> ["small", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, [
      "70b", "72b", "90b", "100b", "104b", "110b", "120b", "123b", "405b",
      "671b",
    ])
  {
    True -> ["large", ..tags]
    False -> tags
  }

  // Category tags
  let tags = case
    has_any(combined, [
      "code", "coder", "coding", "starcoder", "codellama", "devstral",
      "sqlcoder", "codegeex",
    ])
  {
    True -> ["code", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, ["embed", "embedding", "bge", "minilm", "arctic-embed"])
  {
    True -> ["embedding", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, [
      "vision", "image", "visual", "llava", "moondream", "ocr", "-vl",
    ])
  {
    True -> ["vision", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, [
      "reason", "think", "math", "r1", "qwq", "o1", "deepscaler", "magistral",
    ])
  {
    True -> ["reasoning", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, ["uncensor", "dolphin"]) && !has_any(combined, ["coder"])
  {
    True -> ["uncensored", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, [
      "multilingual", "chinese", "arabic", "multi-lingual", "aya", "sailor",
    ])
  {
    True -> ["multilingual", ..tags]
    False -> tags
  }

  let tags = case has_any(combined, ["moe", "mixture", "8x7", "8x22"]) {
    True -> ["moe", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, ["function", "tool", "agent", "groq-tool", "nexusraven"])
  {
    True -> ["tools", ..tags]
    False -> tags
  }

  let tags = case has_any(combined, ["medical", "med", "health", "meditron"]) {
    True -> ["medical", ..tags]
    False -> tags
  }

  let tags = case
    has_any(combined, ["guard", "safety", "shield", "safeguard"])
  {
    True -> ["safety", ..tags]
    False -> tags
  }

  let tags = case has_any(combined, ["sql", "duckdb", "nsql"]) {
    True -> ["sql", ..tags]
    False -> tags
  }

  // Default to chat if no specific category
  let has_category =
    list.any(tags, fn(t) {
      list.contains(
        [
          "code", "embedding", "vision", "reasoning", "safety", "medical", "sql",
          "tools",
        ],
        t,
      )
    })

  case has_category {
    True -> list.reverse(tags)
    False -> list.reverse(["chat", ..tags])
  }
}

fn m(name: String, desc: String, pulls: Int) -> Model {
  Model(
    name: name,
    description: desc,
    pulls: pulls,
    tags: categorize(name, desc),
  )
}

fn get_model_database() -> List(Model) {
  [
    // Llama family
    m(
      "llama3.3",
      "Meta's 70B state-of-the-art model with 405B-equivalent performance",
      8_000_000,
    ),
    m(
      "llama3.2",
      "Meta's compact models (1B, 3B) for efficient performance",
      15_000_000,
    ),
    m(
      "llama3.2-vision",
      "Llama 3.2 Vision - Instruction-tuned image reasoning",
      2_000_000,
    ),
    m(
      "llama3.1",
      "Meta's Llama 3.1 available in 8B, 70B, 405B sizes",
      20_000_000,
    ),
    m("llama3", "Meta Llama 3 - Most capable openly available LLM", 10_000_000),
    m("llama2", "Meta Llama 2 foundation models (7B-70B)", 8_000_000),
    m("llama2-uncensored", "Llama 2 uncensored by George Sung", 3_000_000),
    m("llama2-chinese", "Llama 2 fine-tuned for Chinese dialogue", 600_000),
    m("llama4", "Meta Llama 4 - Latest multimodal collection", 500_000),
    m("llama-guard3", "Llama Guard 3 - Content safety classification", 800_000),
    m("llama-pro", "Llama Pro - Expanded with programming and math", 500_000),

    // DeepSeek family
    m(
      "deepseek-r1",
      "DeepSeek R1 - Open reasoning model near O3/Gemini 2.5 Pro",
      5_000_000,
    ),
    m("deepseek-v3", "DeepSeek V3 - Strong MoE with 671B parameters", 2_000_000),
    m(
      "deepseek-v3.1",
      "DeepSeek V3.1 - Hybrid thinking/non-thinking modes",
      1_500_000,
    ),
    m(
      "deepseek-v3.2",
      "DeepSeek V3.2 - Harmonized efficiency with reasoning",
      500_000,
    ),
    m("deepseek-v2", "DeepSeek V2 - Economical MoE language model", 1_500_000),
    m(
      "deepseek-v2.5",
      "DeepSeek V2.5 - Combined general and coding abilities",
      1_000_000,
    ),
    m("deepseek-coder", "DeepSeek Coder - Trained on 2T code tokens", 3_000_000),
    m(
      "deepseek-coder-v2",
      "DeepSeek Coder V2 - MoE comparable to GPT4-Turbo",
      1_500_000,
    ),
    m("deepseek-llm", "DeepSeek LLM - Bilingual language model", 1_000_000),
    m(
      "deepseek-ocr",
      "DeepSeek OCR - Vision-language for token-efficient OCR",
      300_000,
    ),
    m("deepcoder", "DeepCoder - Fine-tuned coder at O3-mini level", 600_000),
    m("deepscaler", "DeepScaler - Exceeds o1-preview performance", 400_000),

    // Qwen family
    m("qwen3", "Qwen 3 - Latest generation dense and MoE models", 4_000_000),
    m("qwen3-coder", "Qwen 3 Coder - Agentic and coding tasks", 500_000),
    m("qwen3-vl", "Qwen 3 VL - Most powerful Qwen vision-language", 400_000),
    m(
      "qwen3-embedding",
      "Qwen 3 Embedding - Comprehensive text embeddings",
      300_000,
    ),
    m("qwen2.5", "Qwen 2.5 - 18T tokens, 128K context models", 6_000_000),
    m("qwen2.5-coder", "Qwen 2.5 Coder - Code specialist models", 4_000_000),
    m("qwen2.5vl", "Qwen 2.5 VL - Flagship vision-language model", 1_200_000),
    m("qwen2", "Qwen 2 - Alibaba language model series", 4_000_000),
    m("qwen2-math", "Qwen 2 Math - Specialized math models", 800_000),
    m("qwen", "Qwen - Alibaba Cloud series (0.5B-110B)", 3_000_000),
    m("qwq", "QwQ - Reasoning model in Qwen series", 2_000_000),
    m("codeqwen", "CodeQwen - Pretrained on large code data", 1_500_000),

    // Google/Gemma family
    m("gemma3", "Google Gemma 3 - Most capable single-GPU model", 5_000_000),
    m("gemma3n", "Gemma 3N - Designed for everyday devices", 500_000),
    m("gemma2", "Google Gemma 2 - High-performing efficient models", 6_000_000),
    m("gemma", "Google Gemma v1.1 - Lightweight open models", 5_000_000),
    m("codegemma", "CodeGemma - Lightweight coding tasks", 3_000_000),
    m("embeddinggemma", "Embedding Gemma - 300M embedding model", 400_000),
    m("shieldgemma", "ShieldGemma - Safety evaluation models", 500_000),
    m("functiongemma", "FunctionGemma - Function calling fine-tuned", 200_000),

    // Microsoft Phi family
    m("phi4", "Microsoft Phi-4 14B - State-of-the-art open model", 4_000_000),
    m("phi4-mini", "Phi-4 Mini - Multilingual and function calling", 2_000_000),
    m("phi4-reasoning", "Phi-4 Reasoning - Rivaling larger models", 500_000),
    m(
      "phi4-mini-reasoning",
      "Phi-4 Mini Reasoning - Lightweight reasoning",
      400_000,
    ),
    m("phi3", "Microsoft Phi-3 - Lightweight state-of-the-art", 6_000_000),
    m("phi3.5", "Phi-3.5 - Overtaking larger model performance", 2_500_000),
    m("phi", "Phi-2 - Outstanding reasoning capabilities", 5_000_000),

    // Mistral family
    m("mistral", "Mistral 7B v0.3 - Excellent reasoning", 12_000_000),
    m("mistral-nemo", "Mistral Nemo 12B - 128k context by NVIDIA", 4_000_000),
    m("mistral-small", "Mistral Small - Benchmark-setting small LLM", 2_500_000),
    m(
      "mistral-small3.1",
      "Mistral Small 3.1 - Vision and 128k context",
      1_500_000,
    ),
    m(
      "mistral-small3.2",
      "Mistral Small 3.2 - Improved function calling",
      1_000_000,
    ),
    m(
      "mistral-large",
      "Mistral Large - Flagship 123B with 128k context",
      1_000_000,
    ),
    m("mistral-large-3", "Mistral Large 3 - Multimodal MoE", 500_000),
    m("mixtral", "Mixtral MoE - Open-weight mixture of experts", 5_000_000),
    m("mistral-openorca", "Mistral OpenOrca - Fine-tuned on OpenOrca", 800_000),
    m("mistrallite", "MistralLite - Long context processing", 400_000),
    m("codestral", "Codestral - Mistral's code generation model", 2_000_000),
    m("mathstral", "Mathstral - Math reasoning and discovery", 500_000),
    m("devstral", "Devstral - Best open-source coding agent", 600_000),
    m("devstral-2", "Devstral 2 - File editing agent 123B", 150_000),
    m("devstral-small-2", "Devstral Small 2 - Code exploration 24B", 300_000),
    m("magistral", "Magistral - Efficient 24B reasoning model", 300_000),

    // Code models
    m("codellama", "Code Llama - Generates and discusses code", 6_000_000),
    m("starcoder", "StarCoder - 80+ programming languages", 2_500_000),
    m("starcoder2", "StarCoder2 - Next-gen transparent code LLM", 2_000_000),
    m(
      "wizardcoder",
      "WizardCoder - State-of-the-art code generation",
      1_000_000,
    ),
    m("phind-codellama", "Phind CodeLlama - Code generation", 1_200_000),
    m("sqlcoder", "SQLCoder - SQL generation specialist", 1_000_000),
    m("duckdb-nsql", "DuckDB NSQL - Text-to-SQL model", 500_000),
    m("codegeex4", "CodeGeeX4 - AI software development", 1_000_000),
    m("stable-code", "Stable Code - Matches 7B Code Llama", 1_500_000),
    m("opencoder", "OpenCoder - Reproducible code LLM", 700_000),
    m("yi-coder", "Yi Coder - Open-source coding models", 800_000),
    m("magicoder", "Magicoder - Synthetic instruction data", 800_000),
    m("codebooga", "CodeBooga - High-performing code merge", 500_000),
    m("dolphincoder", "DolphinCoder - Uncensored coding", 1_000_000),
    m("codeup", "CodeUp - Code generation based on Llama2", 400_000),

    // Vision models
    m("llava", "LLaVA - Vision encoder with language understanding", 4_000_000),
    m("llava-llama3", "LLaVA Llama3 - Improved benchmark scores", 1_500_000),
    m("llava-phi3", "LLaVA Phi3 - Small vision model", 1_000_000),
    m("bakllava", "BakLLaVA - Mistral with LLaVA architecture", 1_200_000),
    m("moondream", "Moondream - Small vision for edge devices", 1_500_000),
    m("minicpm-v", "MiniCPM-V - Vision-language understanding", 1_000_000),
    m(
      "granite3.2-vision",
      "Granite 3.2 Vision - Document understanding",
      400_000,
    ),

    // Embedding models
    m(
      "nomic-embed-text",
      "Nomic Embed Text - High-performing embeddings",
      8_000_000,
    ),
    m("nomic-embed-text-v2-moe", "Nomic Embed V2 MoE - Multilingual", 500_000),
    m(
      "mxbai-embed-large",
      "MixedBread Embed Large - State-of-the-art",
      4_000_000,
    ),
    m("bge-m3", "BGE M3 - Multi-functionality embeddings", 2_000_000),
    m("bge-large", "BGE Large - Text to vector mapping", 1_500_000),
    m("all-minilm", "All-MiniLM - Fast sentence embeddings", 3_000_000),
    m(
      "snowflake-arctic-embed",
      "Snowflake Arctic Embed - Performance optimized",
      1_500_000,
    ),
    m(
      "snowflake-arctic-embed2",
      "Snowflake Arctic Embed 2 - Frontier multilingual",
      500_000,
    ),
    m("granite-embedding", "Granite Embedding - IBM biencoder", 600_000),
    m(
      "paraphrase-multilingual",
      "Paraphrase Multilingual - Clustering and search",
      800_000,
    ),

    // Dolphin family (uncensored)
    m("dolphin3", "Dolphin 3 - Next-gen instruct-tuned model", 2_000_000),
    m("dolphin-mixtral", "Dolphin Mixtral - Uncensored MoE coding", 2_000_000),
    m("dolphin-llama3", "Dolphin Llama 3 - By Eric Hartford", 1_500_000),
    m("dolphin-mistral", "Dolphin Mistral v2.8 - Uncensored coding", 2_000_000),
    m("dolphin-phi", "Dolphin Phi - Uncensored based on Phi", 1_500_000),
    m("tinydolphin", "TinyDolphin - Experimental 1.1B Dolphin", 1_000_000),
    m("megadolphin", "MegaDolphin - 120B interleaved Dolphin", 200_000),

    // Hermes/Nous family
    m("hermes3", "Hermes 3 - Nous Research flagship", 2_000_000),
    m("nous-hermes", "Nous Hermes - General models", 2_000_000),
    m("nous-hermes2", "Nous Hermes 2 - Scientific and coding", 1_500_000),
    m(
      "nous-hermes2-mixtral",
      "Nous Hermes 2 Mixtral - Trained over Mixtral",
      1_000_000,
    ),
    m("openhermes", "OpenHermes - Fine-tuned on Mistral", 2_000_000),

    // Granite family (IBM)
    m("granite-code", "Granite Code - IBM code intelligence", 1_000_000),
    m("granite3-dense", "Granite 3 Dense - Tool-based and RAG", 600_000),
    m("granite3.1-dense", "Granite 3.1 Dense - 12T tokens trained", 500_000),
    m("granite3-moe", "Granite 3 MoE - First Granite MoE", 400_000),
    m("granite3.1-moe", "Granite 3.1 MoE - Long-context MoE", 450_000),
    m("granite3.2", "Granite 3.2 - Thinking capabilities", 500_000),
    m("granite3.3", "Granite 3.3 - 128K context reasoning", 400_000),
    m("granite3-guardian", "Granite 3 Guardian - Risk detection", 400_000),
    m("granite4", "Granite 4 - Improved instruction following", 300_000),

    // Other popular models
    m("tinyllama", "TinyLlama 1.1B - Trained on 3T tokens", 5_000_000),
    m("smollm", "SmolLM - Small family (135M-1.7B)", 2_000_000),
    m("smollm2", "SmolLM 2 - Updated compact family", 2_500_000),
    m("olmo2", "OLMo 2 - Trained on 5T tokens", 1_500_000),
    m("olmo-3", "OLMo 3 - Dolma 3 dataset", 800_000),
    m("olmo-3.1", "OLMo 3.1 - Latest from Dolma 3", 400_000),
    m("vicuna", "Vicuna - General chat model", 3_000_000),
    m("openchat", "OpenChat v3.5 - Surpasses ChatGPT", 2_500_000),
    m("neural-chat", "Neural Chat - Fine-tuned Mistral", 1_500_000),
    m("starling-lm", "Starling LM - RLHF trained", 1_200_000),
    m("zephyr", "Zephyr - Fine-tuned as helpful assistant", 3_000_000),
    m("falcon", "Falcon - TII summarization and chat", 2_500_000),
    m("falcon2", "Falcon 2 - 11B trained on 5T tokens", 1_200_000),
    m("falcon3", "Falcon 3 - Efficient for science/math", 1_000_000),
    m("yi", "Yi - High-performing bilingual model", 2_000_000),
    m("solar", "Solar 10.7B - Single-turn conversation", 1_500_000),
    m("solar-pro", "Solar Pro 22B - Single GPU advanced", 800_000),
    m("glm4", "GLM-4 - Strong multi-lingual model", 1_200_000),
    m("glm-4.6", "GLM-4.6 - Advanced agentic and reasoning", 500_000),
    m("glm-4.7", "GLM-4.7 - Improved coding capability", 300_000),
    m(
      "command-r",
      "Cohere Command-R - Conversational and long context",
      1_500_000,
    ),
    m("command-r-plus", "Cohere Command-R Plus - Enterprise use cases", 800_000),
    m("command-r7b", "Cohere Command-R 7B - Efficient quality", 1_000_000),
    m(
      "command-r7b-arabic",
      "Cohere Command-R 7B Arabic - Arabic language",
      500_000,
    ),
    m("command-a", "Cohere Command-A - Enterprise optimized", 400_000),
    m("aya", "Aya - 23 languages by Cohere", 800_000),
    m("aya-expanse", "Aya Expanse - 23 languages trained", 700_000),
    m("orca-mini", "Orca Mini - General purpose", 2_500_000),
    m("orca2", "Orca 2 - Microsoft fine-tuned reasoning", 1_500_000),
    m("wizardlm", "WizardLM - General purpose", 2_000_000),
    m("wizardlm2", "WizardLM 2 - Chat and reasoning", 1_500_000),
    m("wizardlm-uncensored", "WizardLM Uncensored", 1_000_000),
    m("wizard-vicuna-uncensored", "Wizard Vicuna Uncensored", 1_500_000),
    m("wizard-vicuna", "Wizard Vicuna - Based on Llama 2", 800_000),
    m("wizard-math", "Wizard Math - Math and logic", 600_000),
    m("stable-beluga", "Stable Beluga - Orca-style fine-tuned", 1_000_000),
    m("stablelm2", "StableLM 2 - Multilingual", 1_500_000),
    m("stablelm-zephyr", "StableLM Zephyr - Lightweight chat", 1_000_000),
    m("dbrx", "DBRX - Databricks open LLM", 500_000),
    m("xwinlm", "XWin-LM - Competitive conversational", 800_000),
    m("internlm2", "InternLM2 - Practical scenarios with reasoning", 600_000),
    m("exaone3.5", "EXAONE 3.5 - Bilingual by LG", 600_000),
    m("exaone-deep", "EXAONE Deep - Superior reasoning by LG", 500_000),
    m("sailor2", "Sailor 2 - South-East Asian multilingual", 400_000),
    m("tulu3", "Tulu 3 - Instruction-following by Allen AI", 400_000),
    m("reflection", "Reflection - Reflection-tuning technique", 300_000),
    m("athene-v2", "Athene V2 - Code and math excellence", 300_000),
    m("cogito", "Cogito - Hybrid reasoning models", 500_000),
    m("cogito-2.1", "Cogito 2.1 - Instruction-tuned under MIT", 200_000),
    m("openthinker", "OpenThinker - Distilled from DeepSeek-R1", 600_000),
    m("smallthinker", "SmallThinker - Fine-tuned from Qwen 2.5", 500_000),
    m("r1-1776", "R1-1776 - Unbiased factual DeepSeek-R1", 200_000),
    m("marco-o1", "Marco-o1 - Open reasoning by Alibaba", 400_000),
    m("nemotron", "Nemotron - NVIDIA customized for helpfulness", 300_000),
    m("nemotron-mini", "Nemotron Mini - Roleplay and RAG by NVIDIA", 500_000),
    m("nemotron-3-nano", "Nemotron 3 Nano - Agentic model by NVIDIA", 200_000),
    m("reader-lm", "Reader LM - HTML to Markdown conversion", 500_000),
    m("nuextract", "NuExtract - Information extraction", 400_000),
    m("meditron", "Meditron - Medical domain Llama 2", 500_000),
    m("medllama2", "MedLlama2 - Medical questions", 400_000),
    m("llama3-chatqa", "Llama 3 ChatQA - Conversational QA and RAG", 600_000),
    m(
      "llama3-groq-tool-use",
      "Llama 3 Groq Tool Use - Open-source tools",
      500_000,
    ),
    m("llama3-gradient", "Llama 3 Gradient - 1M+ token context", 500_000),
    m(
      "firefunction-v2",
      "FireFunction V2 - Function calling on Llama 3",
      300_000,
    ),
    m("nexusraven", "NexusRaven - Function calling tasks", 400_000),
    m("samantha-mistral", "Samantha Mistral - Companion trained", 700_000),
    m("yarn-llama2", "YARN Llama 2 - 128k context", 500_000),
    m("yarn-mistral", "YARN Mistral - 64K/128K context", 600_000),
    m("everythinglm", "EverythingLM - 16K context uncensored", 600_000),
    m("notux", "Notux - Top MoE fine-tuned", 300_000),
    m("notus", "Notus - High-quality data fine-tuned", 400_000),
    m("open-orca-platypus2", "Open Orca Platypus2 - Chat and code", 400_000),
    m("goliath", "Goliath - Combined Llama 2 70B", 300_000),
    m("alfred", "Alfred - Robust conversational", 300_000),
    m("bespoke-minicheck", "Bespoke MiniCheck - Fact-checking", 300_000),
    m("gpt-oss", "GPT-OSS - OpenAI's open-weight reasoning", 500_000),
    m("gpt-oss-safeguard", "GPT-OSS Safeguard - Safety reasoning", 200_000),
    m("minimax-m2", "MiniMax M2 - Coding workflows", 300_000),
    m("minimax-m2.1", "MiniMax M2.1 - Multilingual code engineering", 100_000),
    m("ministral-3", "Ministral 3 - Edge deployment", 300_000),
    m(
      "gemini-3-pro-preview",
      "Gemini 3 Pro Preview - Google reasoning",
      200_000,
    ),
    m("gemini-3-flash-preview", "Gemini 3 Flash Preview - High-speed", 200_000),
    m("kimi-k2", "Kimi K2 - State-of-the-art MoE", 300_000),
    m("kimi-k2-thinking", "Kimi K2 Thinking - Moonshot thinking model", 200_000),
    m("qwen3-next", "Qwen3-Next - Parameter efficiency and speed", 200_000),
    m("rnj-1", "RNJ-1 - 8B optimized for code and STEM", 100_000),
  ]
}
