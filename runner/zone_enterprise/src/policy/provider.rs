use serde::{Deserialize, Serialize};

const SEPARATOR: char = '/';

const LOCAL: [&str; 5] = [
    "ollama",
    "ollama_chat",
    "hosted_vllm",
    "lm_studio",
    "llamafile",
];
const ANTHROPIC: [&str; 1] = ["anthropic"];
const OPENAI: [&str; 2] = ["openai", "text-completion-openai"];
const BEDROCK: [&str; 1] = ["bedrock"];
const VERTEX: [&str; 2] = ["vertex_ai", "vertex_ai_beta"];
const AZURE: [&str; 2] = ["azure", "azure_ai"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Local,
    Anthropic,
    #[serde(rename = "openai")]
    OpenAI,
    Bedrock,
    Vertex,
    Azure,
    Other,
}

impl Provider {
    pub const ALL: [Self; 7] = [
        Self::Local,
        Self::Anthropic,
        Self::OpenAI,
        Self::Bedrock,
        Self::Vertex,
        Self::Azure,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Bedrock => "bedrock",
            Self::Vertex => "vertex",
            Self::Azure => "azure",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local models",
            Self::Anthropic => "Anthropic API",
            Self::OpenAI => "OpenAI API",
            Self::Bedrock => "AWS Bedrock",
            Self::Vertex => "Google Vertex AI",
            Self::Azure => "Azure OpenAI",
            Self::Other => "Other OpenAI-compatible endpoints",
        }
    }

    pub fn routed(litellm_model: &str) -> Provider {
        let Some((prefix, _)) = litellm_model.trim().split_once(SEPARATOR) else {
            return Self::Other;
        };
        [
            (Self::Local, LOCAL.as_slice()),
            (Self::Anthropic, ANTHROPIC.as_slice()),
            (Self::OpenAI, OPENAI.as_slice()),
            (Self::Bedrock, BEDROCK.as_slice()),
            (Self::Vertex, VERTEX.as_slice()),
            (Self::Azure, AZURE.as_slice()),
        ]
        .into_iter()
        .find(|(_, prefixes)| prefixes.contains(&prefix))
        .map_or(Self::Other, |(provider, _)| provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_every_litellm_prefix() {
        for (model, provider) in [
            ("ollama/llama3", Provider::Local),
            ("ollama_chat/qwen3:8b", Provider::Local),
            ("hosted_vllm/meta-llama/Llama-3.1-8B", Provider::Local),
            ("lm_studio/gemma", Provider::Local),
            ("llamafile/mistral", Provider::Local),
            ("anthropic/claude-sonnet-4-5", Provider::Anthropic),
            ("openai/gpt-5", Provider::OpenAI),
            (
                "text-completion-openai/gpt-3.5-turbo-instruct",
                Provider::OpenAI,
            ),
            ("bedrock/anthropic.claude-sonnet-4-5", Provider::Bedrock),
            (
                "bedrock/converse/us.anthropic.claude-opus-4-1",
                Provider::Bedrock,
            ),
            ("vertex_ai/gemini-2.5-pro", Provider::Vertex),
            (
                "vertex_ai_beta/claude-sonnet-4-5@20250929",
                Provider::Vertex,
            ),
            ("azure/gpt-4o-deployment", Provider::Azure),
            ("azure_ai/mistral-large", Provider::Azure),
        ] {
            assert_eq!(Provider::routed(model), provider, "{model}");
        }
    }

    #[test]
    fn routes_an_unprefixed_name_to_other() {
        assert_eq!(Provider::routed("gpt-5"), Provider::Other);
        assert_eq!(Provider::routed(""), Provider::Other);
    }

    #[test]
    fn routes_an_unknown_prefix_to_other() {
        assert_eq!(Provider::routed("groq/llama-3.3-70b"), Provider::Other);
        assert_eq!(Provider::routed("Anthropic/claude"), Provider::Other);
        assert_eq!(Provider::routed("/claude"), Provider::Other);
    }

    #[test]
    fn serializes_its_wire_values() {
        let values: Vec<String> = Provider::ALL
            .iter()
            .map(|provider| serde_json::to_string(provider).unwrap())
            .collect();
        assert_eq!(
            values,
            [
                r#""local""#,
                r#""anthropic""#,
                r#""openai""#,
                r#""bedrock""#,
                r#""vertex""#,
                r#""azure""#,
                r#""other""#,
            ]
        );
    }

    #[test]
    fn as_str_is_the_wire_value() {
        for provider in Provider::ALL {
            assert_eq!(
                serde_json::to_value(provider).unwrap(),
                provider.as_str(),
                "{provider:?}"
            );
            assert_eq!(
                serde_json::from_value::<Provider>(provider.as_str().into()).unwrap(),
                provider
            );
        }
    }

    #[test]
    fn refuses_unknown_values() {
        for value in [r#""open_a_i""#, r#""OpenAI""#, r#""gemini""#, r#""""#] {
            assert!(serde_json::from_str::<Provider>(value).is_err(), "{value}");
        }
    }

    #[test]
    fn labels_every_provider() {
        let labels: Vec<&str> = Provider::ALL
            .iter()
            .map(|provider| provider.label())
            .collect();
        assert_eq!(
            labels,
            [
                "Local models",
                "Anthropic API",
                "OpenAI API",
                "AWS Bedrock",
                "Google Vertex AI",
                "Azure OpenAI",
                "Other OpenAI-compatible endpoints",
            ]
        );
    }
}
