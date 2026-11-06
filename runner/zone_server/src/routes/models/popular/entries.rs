use abnegate_llm::catalog::{ModelDetails, ModelEntry};

use super::entry::Entry;

const FORMAT: &str = "gguf";
const LIBRARY_URL: &str = "https://ollama.com/library";

/// The models Browse lists when ollama.com's search page lists none.
const POPULAR: &[Entry] = &[
    Entry {
        name: "llama3.2",
        family: "llama",
        parameter_size: "3B",
        description: "Meta's compact Llama 3.2 for multilingual chat and on-device assistants.",
        use_cases: &["Chat", "Agents"],
    },
    Entry {
        name: "llama3.1",
        family: "llama",
        parameter_size: "8B",
        description: "Meta Llama 3.1 8B — a strong general-purpose local chat and instruction model.",
        use_cases: &["Chat", "Tool use"],
    },
    Entry {
        name: "llama3.1:70b",
        family: "llama",
        parameter_size: "70B",
        description: "Meta Llama 3.1 70B for higher-quality reasoning, writing, and complex assistants.",
        use_cases: &["Chat", "Reasoning"],
    },
    Entry {
        name: "mistral",
        family: "mistral",
        parameter_size: "7B",
        description: "Mistral 7B — fast, capable instruction following for chat and lightweight agents.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "mixtral",
        family: "mixtral",
        parameter_size: "47B",
        description: "Mixtral 8x7B mixture-of-experts for high-quality chat at a modest active-parameter cost.",
        use_cases: &["Chat", "Reasoning"],
    },
    Entry {
        name: "qwen2.5",
        family: "qwen",
        parameter_size: "7B",
        description: "Qwen2.5 7B — strong multilingual chat, coding, and instruction following.",
        use_cases: &["Chat", "Coding"],
    },
    Entry {
        name: "qwen2.5:72b",
        family: "qwen",
        parameter_size: "72B",
        description: "Qwen2.5 72B for demanding reasoning, coding, and long-form generation.",
        use_cases: &["Chat", "Coding", "Reasoning"],
    },
    Entry {
        name: "phi3",
        family: "phi",
        parameter_size: "3.8B",
        description: "Microsoft Phi-3 Mini — small, efficient model for constrained hardware.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "gemma2",
        family: "gemma",
        parameter_size: "9B",
        description: "Google Gemma 2 9B for lightweight chat and instruction workloads.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "deepseek-r1",
        family: "deepseek",
        parameter_size: "7B",
        description: "DeepSeek-R1 distill focused on step-by-step reasoning and math.",
        use_cases: &["Reasoning"],
    },
    Entry {
        name: "deepseek-r1:32b",
        family: "deepseek",
        parameter_size: "32B",
        description: "Larger DeepSeek-R1 distill for harder reasoning and analysis tasks.",
        use_cases: &["Reasoning"],
    },
    Entry {
        name: "codellama",
        family: "codellama",
        parameter_size: "7B",
        description: "Code Llama 7B for code completion, explanation, and light refactoring.",
        use_cases: &["Coding"],
    },
    Entry {
        name: "starcoder2",
        family: "starcoder",
        parameter_size: "7B",
        description: "StarCoder2 7B trained on permissively licensed code for completion and generation.",
        use_cases: &["Coding"],
    },
    Entry {
        name: "qwen3-embedding:0.6b",
        family: "qwen",
        parameter_size: "0.6B",
        description: "Qwen3 Embedding 0.6B — 1024-d instruction-aware embeddings for code and text.",
        use_cases: &["Embeddings"],
    },
    Entry {
        name: "nomic-embed-text",
        family: "nomic",
        parameter_size: "137M",
        description: "Nomic Embed Text — compact embedding model for search and retrieval.",
        use_cases: &["Embeddings"],
    },
    Entry {
        name: "mxbai-embed-large",
        family: "mxbai",
        parameter_size: "335M",
        description: "mixedbread embed-large for higher-quality retrieval embeddings.",
        use_cases: &["Embeddings"],
    },
    Entry {
        name: "command-r",
        family: "command",
        parameter_size: "35B",
        description: "Cohere Command R for retrieval-augmented chat and tool-using agents.",
        use_cases: &["Chat", "Tool use", "Agents"],
    },
    Entry {
        name: "yi",
        family: "yi",
        parameter_size: "34B",
        description: "01.AI Yi 34B bilingual chat model for English and Chinese workloads.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "granite3-dense",
        family: "granite",
        parameter_size: "8B",
        description: "IBM Granite 3 Dense 8B for enterprise chat and instruction following.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "smollm2",
        family: "smollm",
        parameter_size: "1.7B",
        description: "SmolLM2 — very small on-device model for simple chat and experiments.",
        use_cases: &["Chat"],
    },
    Entry {
        name: "dolphin-mixtral",
        family: "mixtral",
        parameter_size: "47B",
        description: "Uncensored Dolphin fine-tune of Mixtral for creative chat and roleplay.",
        use_cases: &["Chat"],
    },
];

pub(super) fn popular_models() -> Vec<ModelEntry> {
    POPULAR
        .iter()
        .map(|popular| {
            let mut details = ModelDetails::default();
            details.format = Some(FORMAT.to_string());
            details.family = Some(popular.family.to_string());
            details.parameter_size = Some(popular.parameter_size.to_string());

            let mut entry = ModelEntry::default();
            entry.name = popular.name.to_string();
            entry.description = Some(popular.description.to_string());
            entry.url = Some(format!("{LIBRARY_URL}/{}", popular.name));
            entry.use_cases = Some(
                popular
                    .use_cases
                    .iter()
                    .map(|case| (*case).to_string())
                    .collect(),
            );
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
