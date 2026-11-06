use abnegate_llm::catalog::ModelDetails;
use abnegate_llm::catalog::ModelEntry;

const FORMAT: &str = "gguf";
const LIBRARY_URL: &str = "https://ollama.com/library";

/// Name, family, parameter size, description and use cases of the models
/// Browse lists when ollama.com's search page lists none.
const POPULAR: &[(&str, &str, &str, &str, &[&str])] = &[
    (
        "llama3.2",
        "llama",
        "3B",
        "Meta's compact Llama 3.2 for multilingual chat and on-device assistants.",
        &["Chat", "Agents"],
    ),
    (
        "llama3.1",
        "llama",
        "8B",
        "Meta Llama 3.1 8B — a strong general-purpose local chat and instruction model.",
        &["Chat", "Tool use"],
    ),
    (
        "llama3.1:70b",
        "llama",
        "70B",
        "Meta Llama 3.1 70B for higher-quality reasoning, writing, and complex assistants.",
        &["Chat", "Reasoning"],
    ),
    (
        "mistral",
        "mistral",
        "7B",
        "Mistral 7B — fast, capable instruction following for chat and lightweight agents.",
        &["Chat"],
    ),
    (
        "mixtral",
        "mixtral",
        "47B",
        "Mixtral 8x7B mixture-of-experts for high-quality chat at a modest active-parameter cost.",
        &["Chat", "Reasoning"],
    ),
    (
        "qwen2.5",
        "qwen",
        "7B",
        "Qwen2.5 7B — strong multilingual chat, coding, and instruction following.",
        &["Chat", "Coding"],
    ),
    (
        "qwen2.5:72b",
        "qwen",
        "72B",
        "Qwen2.5 72B for demanding reasoning, coding, and long-form generation.",
        &["Chat", "Coding", "Reasoning"],
    ),
    (
        "phi3",
        "phi",
        "3.8B",
        "Microsoft Phi-3 Mini — small, efficient model for constrained hardware.",
        &["Chat"],
    ),
    (
        "gemma2",
        "gemma",
        "9B",
        "Google Gemma 2 9B for lightweight chat and instruction workloads.",
        &["Chat"],
    ),
    (
        "deepseek-r1",
        "deepseek",
        "7B",
        "DeepSeek-R1 distill focused on step-by-step reasoning and math.",
        &["Reasoning"],
    ),
    (
        "deepseek-r1:32b",
        "deepseek",
        "32B",
        "Larger DeepSeek-R1 distill for harder reasoning and analysis tasks.",
        &["Reasoning"],
    ),
    (
        "codellama",
        "codellama",
        "7B",
        "Code Llama 7B for code completion, explanation, and light refactoring.",
        &["Coding"],
    ),
    (
        "starcoder2",
        "starcoder",
        "7B",
        "StarCoder2 7B trained on permissively licensed code for completion and generation.",
        &["Coding"],
    ),
    (
        "qwen3-embedding:0.6b",
        "qwen",
        "0.6B",
        "Qwen3 Embedding 0.6B — 1024-d instruction-aware embeddings for code and text.",
        &["Embeddings"],
    ),
    (
        "nomic-embed-text",
        "nomic",
        "137M",
        "Nomic Embed Text — compact embedding model for search and retrieval.",
        &["Embeddings"],
    ),
    (
        "mxbai-embed-large",
        "mxbai",
        "335M",
        "mixedbread embed-large for higher-quality retrieval embeddings.",
        &["Embeddings"],
    ),
    (
        "command-r",
        "command",
        "35B",
        "Cohere Command R for retrieval-augmented chat and tool-using agents.",
        &["Chat", "Tool use", "Agents"],
    ),
    (
        "yi",
        "yi",
        "34B",
        "01.AI Yi 34B bilingual chat model for English and Chinese workloads.",
        &["Chat"],
    ),
    (
        "granite3-dense",
        "granite",
        "8B",
        "IBM Granite 3 Dense 8B for enterprise chat and instruction following.",
        &["Chat"],
    ),
    (
        "smollm2",
        "smollm",
        "1.7B",
        "SmolLM2 — very small on-device model for simple chat and experiments.",
        &["Chat"],
    ),
    (
        "dolphin-mixtral",
        "mixtral",
        "47B",
        "Uncensored Dolphin fine-tune of Mixtral for creative chat and roleplay.",
        &["Chat"],
    ),
];

pub(super) fn popular_models() -> Vec<ModelEntry> {
    POPULAR
        .iter()
        .map(|(name, family, parameter_size, description, use_cases)| {
            let mut details = ModelDetails::default();
            details.format = Some(FORMAT.to_string());
            details.family = Some((*family).to_string());
            details.parameter_size = Some((*parameter_size).to_string());

            let mut entry = ModelEntry::default();
            entry.name = (*name).to_string();
            entry.description = Some((*description).to_string());
            entry.url = Some(format!("{LIBRARY_URL}/{name}"));
            entry.use_cases = Some(use_cases.iter().map(|case| (*case).to_string()).collect());
            entry.details = Some(details);
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_popular_model_is_a_described_library_gguf() {
        let models = popular_models();
        assert_eq!(models.len(), POPULAR.len());
        for model in &models {
            let details = model.details.as_ref().expect("details");
            assert_eq!(details.format.as_deref(), Some(FORMAT), "{}", model.name);
            assert!(details.family.is_some(), "{}", model.name);
            assert!(details.parameter_size.is_some(), "{}", model.name);
            assert_eq!(
                model.url.as_deref(),
                Some(format!("https://ollama.com/library/{}", model.name).as_str())
            );
            assert!(
                model.size.is_none() && model.sizes.is_none(),
                "{}",
                model.name
            );
        }
    }

    #[test]
    fn the_list_keeps_its_order() {
        let names: Vec<String> = popular_models()
            .into_iter()
            .map(|model| model.name)
            .collect();
        assert_eq!(names.first().map(String::as_str), Some("llama3.2"));
        assert_eq!(names.last().map(String::as_str), Some("dolphin-mixtral"));
    }
}
