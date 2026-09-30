//! The OpenAI-compatible endpoint a workspace's completions are sent to, and
//! the key they carry there.
//!
//! The instance key only ever goes to the instance's `LITELLM_HOST`, and a
//! URL saved in AI Settings only ever receives the key saved beside it.

use std::fmt;

use reqwest::Url;
use uuid::Uuid;
use zone_chat::capacity::Resolver;
use zone_context::embeddings::providers::{PROVIDER_OPENAI, PROVIDER_SELF_HOSTED};
use zone_core::llm::{LlmBackend, LlmConfig};
use zone_core::secret::{REDACTED, SecretValue, redact};

use crate::config::Config;
use crate::db::ai_settings::{self, EffectiveAiSettings, PROVIDER_ANTHROPIC};
use crate::db::workspaces;
use crate::services::stages::{self, Catalog};
use crate::state::AppState;

pub const OPENAI_URL: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";

const VERSION_PATH: &str = "/v1";
const SCHEMES: [&str; 2] = ["http", "https"];

/// The shortest run of a key an echo must share with it to be taken for it.
const MINIMUM_ECHO: usize = 3;
const MASK: char = '*';

/// Who chose where completions go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The instance's own `LITELLM_HOST`.
    Instance,
    /// A URL an organization or workspace saved in AI Settings.
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("The URL must be absolute.")]
    Unparseable,
    #[error("The URL must use http or https.")]
    Scheme,
    #[error("The URL must include a host.")]
    Host,
    #[error("The URL must not include a username or password.")]
    Userinfo,
    #[error("The URL must not include a query.")]
    Query,
    #[error("The URL must not include a fragment.")]
    Fragment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "No model is set for this provider. Set a model in AI Settings for this provider; Zone cannot choose one on an endpoint it does not run."
    )]
    ModelUnset,
}

/// Check the shape of an endpoint URL an organization or workspace saves.
///
/// Private, LAN and loopback hosts pass: a self-hosted Zone points at them.
pub fn validate_url(raw: &str) -> Result<Url, UrlError> {
    let url = Url::parse(raw.trim()).map_err(|_| UrlError::Unparseable)?;
    if !SCHEMES.contains(&url.scheme()) {
        return Err(UrlError::Scheme);
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(UrlError::Host);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlError::Userinfo);
    }
    if url.query().is_some() {
        return Err(UrlError::Query);
    }
    if url.fragment().is_some() {
        return Err(UrlError::Fragment);
    }
    Ok(url)
}

#[derive(Clone)]
pub struct Endpoint {
    url: String,
    key: SecretValue,
    origin: Origin,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("url", &self.url)
            .field("key", &REDACTED)
            .field("origin", &self.origin)
            .finish()
    }
}

impl Endpoint {
    /// The instance's `LITELLM_HOST` and `LITELLM_KEY`.
    pub fn instance(config: &Config) -> Self {
        Self {
            url: config.litellm_host.clone(),
            key: SecretValue::new(config.litellm_key.clone()),
            origin: Origin::Instance,
        }
    }

    /// The endpoint `settings` name. Coding agents and Bedrock run on the
    /// instance, as does any provider whose settings name no endpoint or
    /// save a URL that fails [`validate_url`].
    pub fn resolve(config: &Config, settings: &EffectiveAiSettings) -> Self {
        let resolved = match settings.provider.as_str() {
            PROVIDER_SELF_HOSTED => Self::self_hosted(
                config,
                settings.litellm_host.as_deref(),
                settings.litellm_key.as_ref(),
            ),
            PROVIDER_OPENAI => Self::provider(
                settings.openai_base_url.as_deref(),
                settings.openai_api_key.as_ref(),
                OPENAI_URL,
            ),
            PROVIDER_ANTHROPIC => Self::provider(
                settings.anthropic_base_url.as_deref(),
                settings.anthropic_api_key.as_ref(),
                ANTHROPIC_URL,
            ),
            _ => Ok(None),
        };
        match resolved {
            Ok(Some(endpoint)) => endpoint,
            Ok(None) => Self::instance(config),
            Err(error) => {
                tracing::warn!(
                    provider = %settings.provider,
                    %error,
                    "The endpoint URL saved in AI Settings is invalid; using the instance's endpoint"
                );
                Self::instance(config)
            }
        }
    }

    /// The endpoint a workspace's settings name, or the instance's when the
    /// workspace or its settings cannot be read.
    pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Self {
        let organization = match workspaces::get_workspace(state.db(), workspace).await {
            Ok(Some(row)) => row.organization_id,
            Ok(None) => {
                tracing::warn!(%workspace, "No such workspace; using the instance's endpoint");
                return Self::instance(state.config());
            }
            Err(error) => {
                tracing::warn!(
                    %workspace,
                    %error,
                    "Could not read the workspace; using the instance's endpoint"
                );
                return Self::instance(state.config());
            }
        };
        match ai_settings::get_effective_ai_settings(state.db(), organization, workspace).await {
            Ok(settings) => Self::resolve(state.config(), &settings),
            Err(error) => {
                tracing::warn!(
                    %workspace,
                    %error,
                    "Could not read the AI settings; using the instance's endpoint"
                );
                Self::instance(state.config())
            }
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn key(&self) -> &SecretValue {
        &self.key
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn llm(
        &self,
        model: impl Into<String>,
        temperature: f32,
        max_tokens: u32,
        backend: LlmBackend,
    ) -> LlmConfig {
        LlmConfig {
            base_url: self.url.clone(),
            api_key: self.key.expose().to_string(),
            default_model: model.into(),
            temperature,
            max_tokens,
            backend,
        }
    }

    /// Where a model's context capacity is learned. Only the instance's own
    /// LiteLLM deployment is asked: a third party publishes no deployment
    /// metadata and must never be sent an Ollama `num_ctx`.
    pub fn capacity(&self, config: &Config) -> Resolver {
        match self.origin {
            Origin::Instance => Resolver::with_context(
                &self.url,
                self.key.expose(),
                &config.ollama_host,
                Some(config.chat.context),
            ),
            Origin::Settings => Resolver::undisclosed(),
        }
    }

    /// The models `backend` can run here. The instance's Ollama lists none
    /// an endpoint the settings name serves.
    pub async fn catalog(&self, ollama_host: &str, backend: &LlmBackend) -> Catalog {
        match (self.origin, backend) {
            (Origin::Settings, LlmBackend::Http) => Catalog::default(),
            _ => Catalog::for_backend(ollama_host, backend).await,
        }
    }

    /// `chosen`, when this endpoint can run it. [`stages::AUTO`] on an
    /// endpoint the settings name has no model to run.
    pub fn model<'a>(&self, chosen: &'a str) -> Result<&'a str, Error> {
        match self.origin {
            Origin::Settings if stages::is_auto(chosen) => Err(Error::ModelUnset),
            _ => Ok(chosen),
        }
    }

    /// `text`, a failure this endpoint reported, without its key: whole,
    /// masked the way providers echo a rejected key, or any credential
    /// [`redact`] recognises.
    pub fn scrub(&self, text: &str) -> String {
        let key = self.key.expose().trim();
        if key.is_empty() {
            return redact(text).into_owned();
        }
        let whole = text.replace(key, REDACTED);
        redact(&without_echoes(&whole, key)).into_owned()
    }

    fn self_hosted(
        config: &Config,
        host: Option<&str>,
        key: Option<&SecretValue>,
    ) -> Result<Option<Self>, UrlError> {
        let Some(url) = saved_url(host)? else {
            return Ok(None);
        };
        let key = saved_key(key);
        if same_origin(&url, &config.litellm_host) {
            return Ok(Some(Self {
                url: config.litellm_host.clone(),
                key: key.unwrap_or_else(|| SecretValue::new(config.litellm_key.clone())),
                origin: Origin::Instance,
            }));
        }
        Ok(Some(Self::settings(url, key)))
    }

    fn provider(
        base: Option<&str>,
        key: Option<&SecretValue>,
        default: &str,
    ) -> Result<Option<Self>, UrlError> {
        let url = saved_url(base)?;
        match (saved_key(key), url) {
            (Some(key), Some(url)) => Ok(Some(Self::settings(url, Some(key)))),
            (Some(key), None) => Ok(Some(Self {
                url: default.to_string(),
                key,
                origin: Origin::Settings,
            })),
            (None, Some(url)) => Ok(Some(Self::settings(url, None))),
            (None, None) => Ok(None),
        }
    }

    fn settings(url: Url, key: Option<SecretValue>) -> Self {
        Self {
            url: normalized(url),
            key: key.unwrap_or_else(|| SecretValue::new(String::new())),
            origin: Origin::Settings,
        }
    }
}

fn saved_url(raw: Option<&str>) -> Result<Option<Url>, UrlError> {
    raw.map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(validate_url)
        .transpose()
}

fn saved_key(key: Option<&SecretValue>) -> Option<SecretValue> {
    key.map(|key| key.expose().trim())
        .filter(|key| !key.is_empty())
        .map(SecretValue::new)
}

/// `url` without trailing slashes, versioned when it names only a server.
fn normalized(mut url: Url) -> String {
    let path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() {
        url.set_path(VERSION_PATH);
    } else {
        url.set_path(&path);
    }
    url.into()
}

/// Whether `url` names the server `instance` does: scheme, host and port,
/// with each scheme's default port filled in.
fn same_origin(url: &Url, instance: &str) -> bool {
    Url::parse(instance.trim())
        .is_ok_and(|instance| instance.origin().is_tuple() && instance.origin() == url.origin())
}

/// `text` with every masked echo of `key`, as `sk-ab****wxyz`, redacted.
fn without_echoes(text: &str, key: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut word = String::new();
    for character in text.chars() {
        if separates(character) {
            output.push_str(unechoed(&word, key));
            word.clear();
            output.push(character);
        } else {
            word.push(character);
        }
    }
    output.push_str(unechoed(&word, key));
    output
}

fn separates(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
        )
}

fn unechoed<'a>(word: &'a str, key: &str) -> &'a str {
    let (Some(first), Some(last)) = (word.find(MASK), word.rfind(MASK)) else {
        return word;
    };
    let prefix = &word[..first];
    let suffix = word[last + MASK.len_utf8()..].trim_end_matches(['.', ':', '!', '?']);
    let echoes = |part: &str| part.len() >= MINIMUM_ECHO;
    if (echoes(prefix) && key.starts_with(prefix)) || (echoes(suffix) && key.ends_with(suffix)) {
        REDACTED
    } else {
        word
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::db::ai_settings::EffectiveAiSettings;

    /// `provider` with nothing else saved.
    pub fn settings(provider: &str) -> EffectiveAiSettings {
        EffectiveAiSettings {
            provider: provider.to_string(),
            litellm_host: None,
            litellm_key: None,
            openai_api_key: None,
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
            bedrock_region: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_use_iam_role: false,
            model_fast: None,
            model_reasoning: None,
            model_embedding: None,
            model_image: None,
            model_video: None,
            model_audio: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::settings;
    use super::*;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zone_chat::capacity::Source;
    use zone_context::embeddings::providers::PROVIDER_BEDROCK;

    use crate::db::ai_settings::{PROVIDER_CLAUDE_CODE, PROVIDER_CODEX};

    const INSTANCE_HOST: &str = "http://litellm:4000";
    const INSTANCE_KEY: &str = "sk-instance-key-0123456789";
    const SAVED_KEY: &str = "sk-saved-key-abcdefghijkl";

    fn config() -> Config {
        Config {
            litellm_host: INSTANCE_HOST.to_string(),
            litellm_key: INSTANCE_KEY.to_string(),
            ..crate::state::test_config()
        }
    }

    #[derive(Clone, Copy)]
    struct Saved {
        url: Option<&'static str>,
        key: Option<&'static str>,
    }

    const NOTHING: Saved = Saved {
        url: None,
        key: None,
    };

    /// `provider`'s settings with `values` saved in every provider's fields,
    /// so each provider is shown to read only its own.
    fn saved(provider: &str, values: Saved) -> EffectiveAiSettings {
        let url = values.url.map(str::to_string);
        let key = values.key.map(SecretValue::new);
        EffectiveAiSettings {
            litellm_host: url.clone(),
            litellm_key: key.clone(),
            openai_base_url: url.clone(),
            openai_api_key: key.clone(),
            anthropic_base_url: url,
            anthropic_api_key: key,
            ..settings(provider)
        }
    }

    struct Case {
        name: &'static str,
        provider: &'static str,
        saved: Saved,
        url: &'static str,
        key: &'static str,
        origin: Origin,
    }

    fn cases() -> Vec<Case> {
        vec![
            Case {
                name: "self_hosted with nothing saved runs on the instance",
                provider: PROVIDER_SELF_HOSTED,
                saved: NOTHING,
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted with only a key keeps the instance key on the instance host",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted with a whitespace host keeps the instance key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("   "),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted on the instance's own host without a key keeps the instance key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://LITELLM:4000/"),
                    key: None,
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted on the instance's own host with a key uses that key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://litellm:4000/v1"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: SAVED_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted on another host without a key sends it no key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://gateway.example:4000"),
                    key: None,
                },
                url: "http://gateway.example:4000/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on another host with a whitespace key sends it no key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://gateway.example:4000"),
                    key: Some("  \t "),
                },
                url: "http://gateway.example:4000/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on another host sends it the saved key",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://gateway.example:4000"),
                    key: Some(SAVED_KEY),
                },
                url: "http://gateway.example:4000/v1",
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on a look-alike of the instance host that is no URL runs on the instance",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://litellm:4000.evil.example"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted on a look-alike of the instance host is another host",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://litellm.evil.example:4000"),
                    key: None,
                },
                url: "http://litellm.evil.example:4000/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on the instance host behind credentials runs on the instance",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://litellm:4000@evil.example"),
                    key: None,
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "self_hosted on the instance host behind another port is another host",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://litellm:4001"),
                    key: None,
                },
                url: "http://litellm:4001/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on the instance host over https is another host",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("https://litellm:4000"),
                    key: None,
                },
                url: "https://litellm:4000/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on local Ollama is versioned",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://localhost:11434"),
                    key: None,
                },
                url: "http://localhost:11434/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted with a URL carrying credentials runs on the instance",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://user:pass@gateway.example:4000"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "openai with nothing saved runs on the instance",
                provider: PROVIDER_OPENAI,
                saved: NOTHING,
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "openai with whitespace saved runs on the instance",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: Some(" "),
                    key: Some(" "),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "openai with a key runs on OpenAI",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
                url: OPENAI_URL,
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "openai with a key and a base URL runs on the base URL",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: Some("https://proxy.example/openai/v1/"),
                    key: Some(SAVED_KEY),
                },
                url: "https://proxy.example/openai/v1",
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "openai with only a base URL sends it no key",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: Some("http://192.168.1.20:8080"),
                    key: None,
                },
                url: "http://192.168.1.20:8080/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "openai with a base URL carrying a query runs on the instance",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: Some("https://proxy.example/v1?key=leak"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "anthropic with nothing saved runs on the instance",
                provider: PROVIDER_ANTHROPIC,
                saved: NOTHING,
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "anthropic with a key runs on Anthropic's OpenAI-compatible endpoint",
                provider: PROVIDER_ANTHROPIC,
                saved: Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
                url: ANTHROPIC_URL,
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "anthropic's documented base URL loses its trailing slash",
                provider: PROVIDER_ANTHROPIC,
                saved: Saved {
                    url: Some("https://api.anthropic.com/v1/"),
                    key: Some(SAVED_KEY),
                },
                url: "https://api.anthropic.com/v1",
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "anthropic with only a base URL sends it no key",
                provider: PROVIDER_ANTHROPIC,
                saved: Saved {
                    url: Some("https://gateway.example"),
                    key: None,
                },
                url: "https://gateway.example/v1",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "anthropic with a base URL carrying a fragment runs on the instance",
                provider: PROVIDER_ANTHROPIC,
                saved: Saved {
                    url: Some("https://gateway.example/v1#part"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "bedrock runs on the instance whatever is saved",
                provider: PROVIDER_BEDROCK,
                saved: Saved {
                    url: Some("https://gateway.example"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "claude_code runs on the instance whatever is saved",
                provider: PROVIDER_CLAUDE_CODE,
                saved: Saved {
                    url: Some("https://gateway.example"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
            Case {
                name: "codex runs on the instance whatever is saved",
                provider: PROVIDER_CODEX,
                saved: Saved {
                    url: Some("https://gateway.example"),
                    key: Some(SAVED_KEY),
                },
                url: INSTANCE_HOST,
                key: INSTANCE_KEY,
                origin: Origin::Instance,
            },
        ]
    }

    #[test]
    fn each_provider_resolves_the_endpoint_its_settings_name() {
        let config = config();
        for case in cases() {
            let endpoint = Endpoint::resolve(&config, &saved(case.provider, case.saved));

            assert_eq!(endpoint.url(), case.url, "{}", case.name);
            assert_eq!(endpoint.key().expose(), case.key, "{}", case.name);
            assert_eq!(endpoint.origin(), case.origin, "{}", case.name);
        }
    }

    #[test]
    fn the_instance_key_never_leaves_the_instance_host() {
        let config = config();
        for case in cases() {
            let endpoint = Endpoint::resolve(&config, &saved(case.provider, case.saved));

            if endpoint.key().expose() == INSTANCE_KEY {
                assert_eq!(endpoint.url(), INSTANCE_HOST, "{}", case.name);
                assert_eq!(endpoint.origin(), Origin::Instance, "{}", case.name);
            }
            if endpoint.origin() == Origin::Settings {
                assert_ne!(endpoint.key().expose(), INSTANCE_KEY, "{}", case.name);
            }
        }
    }

    #[test]
    fn the_instance_host_is_used_as_configured() {
        let config = Config {
            litellm_host: "http://litellm:4000/".to_string(),
            ..config()
        };

        let endpoint = Endpoint::instance(&config);

        assert_eq!(endpoint.url(), "http://litellm:4000/");
        assert_eq!(endpoint.key().expose(), INSTANCE_KEY);
        assert_eq!(endpoint.origin(), Origin::Instance);
    }

    #[test]
    fn a_saved_host_is_never_matched_to_an_instance_without_one() {
        let config = Config {
            litellm_host: String::new(),
            ..config()
        };

        let endpoint = Endpoint::resolve(
            &config,
            &saved(
                PROVIDER_SELF_HOSTED,
                Saved {
                    url: Some("http://gateway.example"),
                    key: None,
                },
            ),
        );

        assert_eq!(endpoint.origin(), Origin::Settings);
        assert_eq!(endpoint.key().expose(), "");
    }

    #[test]
    fn validate_url_accepts_private_hosts_and_refuses_every_other_shape() {
        for accepted in [
            "http://localhost:11434",
            "http://127.0.0.1:4000/v1",
            "http://[::1]:4000",
            "http://litellm:4000",
            "https://api.openai.com/v1",
            "  https://api.anthropic.com/v1/  ",
        ] {
            assert!(validate_url(accepted).is_ok(), "{accepted}");
        }
        for (refused, error) in [
            ("", UrlError::Unparseable),
            ("api.openai.com/v1", UrlError::Unparseable),
            ("ftp://files.example", UrlError::Scheme),
            ("file:///etc/passwd", UrlError::Scheme),
            ("unix:/var/run/litellm.sock", UrlError::Scheme),
            ("http://user@gateway.example", UrlError::Userinfo),
            ("http://user:pass@gateway.example", UrlError::Userinfo),
            ("https://gateway.example/v1?api_key=1", UrlError::Query),
            ("https://gateway.example/v1?", UrlError::Query),
            ("https://gateway.example/v1#fragment", UrlError::Fragment),
        ] {
            assert_eq!(validate_url(refused).err(), Some(error), "{refused}");
        }
    }

    #[test]
    fn debug_never_prints_the_key() {
        let config = config();
        for endpoint in [
            Endpoint::instance(&config),
            Endpoint::resolve(
                &config,
                &saved(
                    PROVIDER_OPENAI,
                    Saved {
                        url: None,
                        key: Some(SAVED_KEY),
                    },
                ),
            ),
        ] {
            let printed = format!("{endpoint:?} {endpoint:#?}");

            assert!(!printed.contains(INSTANCE_KEY), "{printed}");
            assert!(!printed.contains(SAVED_KEY), "{printed}");
            assert!(printed.contains(REDACTED), "{printed}");
        }
    }

    #[test]
    fn llm_sends_the_model_to_this_endpoint_under_its_key() {
        let endpoint = Endpoint::resolve(
            &config(),
            &saved(
                PROVIDER_ANTHROPIC,
                Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
            ),
        );

        let llm = endpoint.llm("claude-sonnet-4-5", 0.2, 64, LlmBackend::Http);

        assert_eq!(llm.base_url, ANTHROPIC_URL);
        assert_eq!(llm.api_key, SAVED_KEY);
        assert_eq!(llm.default_model, "claude-sonnet-4-5");
        assert_eq!(llm.temperature, 0.2);
        assert_eq!(llm.max_tokens, 64);
        assert!(matches!(llm.backend, LlmBackend::Http));
    }

    #[tokio::test]
    async fn a_settings_endpoint_lists_no_models_and_reports_an_unknown_capacity_without_a_request()
    {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(404))
            .expect(0)
            .mount(&server)
            .await;
        let config = Config {
            ollama_host: server.uri(),
            ..config()
        };
        let mut settings = settings(PROVIDER_SELF_HOSTED);
        settings.litellm_host = Some(server.uri());
        settings.litellm_key = Some(SecretValue::new(SAVED_KEY));
        let endpoint = Endpoint::resolve(&config, &settings);
        assert_eq!(endpoint.origin(), Origin::Settings);

        let catalog = endpoint.catalog(&server.uri(), &LlmBackend::Http).await;
        let capacity = endpoint.capacity(&config).resolve("gpt-4o").await;

        assert!(catalog.models.is_empty());
        assert_eq!(catalog.agent, None);
        assert_eq!(capacity.source, Source::Unknown);
        assert_eq!(capacity.limit, None);
        assert_eq!(capacity.ollama, None);
        server.verify().await;
    }

    #[tokio::test]
    async fn the_instance_endpoint_asks_its_own_deployment_for_capacity() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(404))
            .expect(1..)
            .mount(&server)
            .await;
        let config = Config {
            litellm_host: server.uri(),
            ollama_host: server.uri(),
            ..config()
        };

        let capacity = Endpoint::instance(&config)
            .capacity(&config)
            .resolve("alias")
            .await;

        assert_eq!(capacity.source, Source::Unknown);
        server.verify().await;
    }

    #[test]
    fn auto_has_no_model_to_run_on_a_settings_endpoint() {
        let config = config();
        let settings = Endpoint::resolve(
            &config,
            &saved(
                PROVIDER_OPENAI,
                Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
            ),
        );
        let instance = Endpoint::instance(&config);

        assert_eq!(settings.model(stages::AUTO), Err(Error::ModelUnset));
        assert_eq!(settings.model("  "), Err(Error::ModelUnset));
        assert_eq!(settings.model("gpt-4o"), Ok("gpt-4o"));
        assert_eq!(instance.model(stages::AUTO), Ok(stages::AUTO));
        assert!(
            Error::ModelUnset
                .to_string()
                .contains("Set a model in AI Settings for this provider")
        );
    }

    #[test]
    fn scrub_removes_the_key_whole_masked_or_recognised() {
        let key = "sk-proj-AbCdEfGh1234567890wxyz";
        let endpoint = Endpoint {
            url: OPENAI_URL.to_string(),
            key: SecretValue::new(key),
            origin: Origin::Settings,
        };
        for (reported, echo) in [
            (format!("API error (401): bad key {key}"), key.to_string()),
            (
                "Incorrect API key provided: sk-proj-****************wxyz. You can find your API key at https://platform.openai.com/account/api-keys.".to_string(),
                "sk-proj-****************wxyz".to_string(),
            ),
            (
                r#"{"error":{"message":"invalid x-api-key: ****wxyz"}}"#.to_string(),
                "****wxyz".to_string(),
            ),
        ] {
            let scrubbed = endpoint.scrub(&reported);

            assert!(!scrubbed.contains(&echo), "{scrubbed}");
            assert!(!scrubbed.contains("wxyz"), "{scrubbed}");
            assert!(scrubbed.contains(REDACTED), "{scrubbed}");
        }
    }

    #[test]
    fn scrub_leaves_a_failure_without_the_key_alone() {
        let endpoint = Endpoint {
            url: OPENAI_URL.to_string(),
            key: SecretValue::new("plainkey-9999"),
            origin: Origin::Settings,
        };
        let failure =
            "API error (404): The model `gpt-9` does not exist ** or you do not have access.";

        assert_eq!(endpoint.scrub(failure), failure);
    }

    #[test]
    fn scrub_with_no_key_still_redacts_what_looks_like_a_credential() {
        let endpoint = Endpoint {
            url: "http://gateway.example/v1".to_string(),
            key: SecretValue::new(""),
            origin: Origin::Settings,
        };

        let scrubbed = endpoint.scrub("rejected sk-live0123456789abcdef");

        assert!(!scrubbed.contains("sk-live0123456789abcdef"), "{scrubbed}");
    }
}
