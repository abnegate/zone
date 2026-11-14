/// A remote catalogue `/api/models` and the agent's browse tool can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogSource {
    Gpt4All,
    HuggingFace,
    Ollama,
    OpenRouter,
}

impl CatalogSource {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "gpt4all" => Some(Self::Gpt4All),
            "huggingface" => Some(Self::HuggingFace),
            "ollama" => Some(Self::Ollama),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }

    /// The upstream a failing status came from, in the words the crate's
    /// providers already use for it.
    pub(crate) fn upstream(self) -> &'static str {
        match self {
            Self::Gpt4All => "GPT4All API",
            Self::HuggingFace => "HuggingFace API",
            Self::Ollama => "Ollama library",
            Self::OpenRouter => "OpenRouter API",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_browsable_name_parses_and_nothing_else_does() {
        for (name, source) in [
            ("gpt4all", CatalogSource::Gpt4All),
            ("huggingface", CatalogSource::HuggingFace),
            ("ollama", CatalogSource::Ollama),
            ("openrouter", CatalogSource::OpenRouter),
        ] {
            assert_eq!(CatalogSource::parse(name), Some(source));
        }
        assert_eq!(CatalogSource::parse("comfy"), None);
        assert_eq!(CatalogSource::parse("Ollama"), None);
    }
}
