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
use zone_core::llm::{Dialect, LlmBackend, LlmConfig, Trust, metadata};
use zone_core::secret::{REDACTED, SecretValue, conceal, redact};

use crate::config::Config;
use crate::db::ai_settings::{EffectiveAiSettings, PROVIDER_ANTHROPIC};
use crate::services::hosts::Hosts;
use crate::services::route;
use crate::services::stages::{self, Catalog};
use crate::state::AppState;

pub const OPENAI_URL: &str = "https://api.openai.com/v1";
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";

const VERSION_PATH: &str = "/v1";
const SCHEMES: [&str; 2] = ["http", "https"];

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
    #[error("The URL must not point at a link-local or cloud metadata address.")]
    Metadata,
    #[error("The URL's host is not one this instance allows endpoints on.")]
    Unlisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "No model is set for this provider. Set a model in AI Settings for this provider; Zone cannot choose one on an endpoint it does not run."
    )]
    ModelUnset,
}

/// Check an endpoint URL an organization or workspace saves.
///
/// Private, LAN and loopback hosts pass: a self-hosted Zone points at them.
/// Link-local and cloud metadata addresses never do, and when the instance
/// lists `hosts`, only those pass.
pub fn validate_url(raw: &str, hosts: &Hosts) -> Result<Url, UrlError> {
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
    if metadata::is_url(&url) {
        return Err(UrlError::Metadata);
    }
    if !hosts.permits(&url) {
        return Err(UrlError::Unlisted);
    }
    Ok(url)
}

#[derive(Clone)]
pub struct Endpoint {
    url: String,
    key: SecretValue,
    origin: Origin,
    dialect: Dialect,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("url", &self.url)
            .field("key", &REDACTED)
            .field("origin", &self.origin)
            .field("dialect", &self.dialect)
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
            dialect: Dialect::Compatible,
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
                config,
                settings.openai_base_url.as_deref(),
                settings.openai_api_key.as_ref(),
                OPENAI_URL,
                Dialect::OpenAI,
            ),
            PROVIDER_ANTHROPIC => Self::provider(
                config,
                settings.anthropic_base_url.as_deref(),
                settings.anthropic_api_key.as_ref(),
                ANTHROPIC_URL,
                Dialect::Anthropic,
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
        match route::saved(state, workspace).await {
            Some((_, settings)) => Self::resolve(state.config(), &settings),
            None => Self::instance(state.config()),
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

    pub fn dialect(&self) -> Dialect {
        self.dialect
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
            dialect: self.dialect,
            trust: match self.origin {
                Origin::Instance => Trust::Operator,
                Origin::Settings => Trust::Tenant,
            },
        }
    }

    /// Where a model's context capacity is learned. Only the instance's own
    /// LiteLLM deployment is asked: a third party publishes no deployment
    /// metadata, so its models are given the configured context, and it must
    /// never be sent an Ollama `num_ctx`.
    pub fn capacity(&self, config: &Config) -> Resolver {
        match self.origin {
            Origin::Instance => Resolver::with_context(
                &self.url,
                self.key.expose(),
                &config.ollama_host,
                Some(config.chat.context),
            ),
            Origin::Settings => Resolver::undisclosed(Some(config.chat.context)),
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
        redact(&conceal(text, self.key.expose())).into_owned()
    }

    fn self_hosted(
        config: &Config,
        host: Option<&str>,
        key: Option<&SecretValue>,
    ) -> Result<Option<Self>, UrlError> {
        let Some(url) = saved_url(host, &config.endpoint_hosts)? else {
            return Ok(None);
        };
        let key = saved_key(key);
        if same_origin(&url.parsed, &config.litellm_host) {
            return Ok(Some(Self {
                url: config.litellm_host.clone(),
                key: key.unwrap_or_else(|| SecretValue::new(config.litellm_key.clone())),
                origin: Origin::Instance,
                dialect: Dialect::Compatible,
            }));
        }
        Ok(Some(Self::settings(url, key, Dialect::Compatible)))
    }

    fn provider(
        config: &Config,
        base: Option<&str>,
        key: Option<&SecretValue>,
        default: &str,
        dialect: Dialect,
    ) -> Result<Option<Self>, UrlError> {
        let url = saved_url(base, &config.endpoint_hosts)?;
        match (saved_key(key), url) {
            (Some(key), Some(url)) => Ok(Some(Self::settings(url, Some(key), dialect))),
            (Some(key), None) => Ok(Some(Self {
                url: default.to_string(),
                key,
                origin: Origin::Settings,
                dialect,
            })),
            (None, Some(url)) => Ok(Some(Self::settings(url, None, dialect))),
            (None, None) => Ok(None),
        }
    }

    fn settings(url: SavedUrl<'_>, key: Option<SecretValue>, dialect: Dialect) -> Self {
        Self {
            url: normalized(url),
            key: key.unwrap_or_else(|| SecretValue::new(String::new())),
            origin: Origin::Settings,
            dialect,
        }
    }
}

struct SavedUrl<'a> {
    parsed: Url,
    raw: &'a str,
}

fn saved_url<'a>(raw: Option<&'a str>, hosts: &Hosts) -> Result<Option<SavedUrl<'a>>, UrlError> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };
    let parsed = validate_url(raw, hosts)?;
    Ok(Some(SavedUrl { parsed, raw }))
}

fn saved_key(key: Option<&SecretValue>) -> Option<SecretValue> {
    key.map(|key| key.expose().trim())
        .filter(|key| !key.is_empty())
        .map(SecretValue::new)
}

/// `url` without trailing slashes. A URL saved as a bare host is versioned;
/// one saved with a trailing `/` and no path names the host's root, which
/// `Url` cannot tell apart from a bare host once parsed.
fn normalized(SavedUrl { mut parsed, raw }: SavedUrl<'_>) -> String {
    let path = parsed.path().trim_end_matches('/').to_string();
    if !path.is_empty() {
        parsed.set_path(&path);
    } else if !raw.ends_with('/') {
        parsed.set_path(VERSION_PATH);
    }
    parsed.as_str().trim_end_matches('/').to_string()
}

/// Whether `url` names the server `instance` does: scheme, host and port,
/// with each scheme's default port filled in.
fn same_origin(url: &Url, instance: &str) -> bool {
    Url::parse(instance.trim())
        .is_ok_and(|instance| instance.origin().is_tuple() && instance.origin() == url.origin())
}

#[cfg(test)]
pub(crate) mod testing {
    use super::{Endpoint, Origin};
    use crate::db::ai_settings::EffectiveAiSettings;
    use zone_core::llm::Dialect;
    use zone_core::secret::SecretValue;

    /// The instance's endpoint at `url` under `key`.
    pub fn endpoint(url: &str, key: &str) -> Endpoint {
        Endpoint {
            url: url.to_string(),
            key: SecretValue::new(key),
            origin: Origin::Instance,
            dialect: Dialect::Compatible,
        }
    }

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
                name: "self_hosted on a host saved with a trailing slash sends to its root",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some(" http://gateway.example:4000/ "),
                    key: Some(SAVED_KEY),
                },
                url: "http://gateway.example:4000",
                key: SAVED_KEY,
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on a host saved with repeated trailing slashes sends to its root",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://gateway.example:4000//"),
                    key: None,
                },
                url: "http://gateway.example:4000",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "self_hosted on a host saved with a path uses the path as saved",
                provider: PROVIDER_SELF_HOSTED,
                saved: Saved {
                    url: Some("http://gateway.example:4000/api/openai"),
                    key: None,
                },
                url: "http://gateway.example:4000/api/openai",
                key: "",
                origin: Origin::Settings,
            },
            Case {
                name: "openai on a base URL saved with a trailing slash sends to its root",
                provider: PROVIDER_OPENAI,
                saved: Saved {
                    url: Some("https://proxy.example/"),
                    key: Some(SAVED_KEY),
                },
                url: "https://proxy.example",
                key: SAVED_KEY,
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
            "http://192.168.1.10:4000",
            "http://litellm:4000",
            "https://api.openai.com/v1",
            "  https://api.anthropic.com/v1/  ",
        ] {
            assert!(
                validate_url(accepted, &Hosts::default()).is_ok(),
                "{accepted}"
            );
        }
        for (refused, error) in [
            ("", UrlError::Unparseable),
            ("api.openai.com/v1", UrlError::Unparseable),
            ("not a url", UrlError::Unparseable),
            ("litellm:4000", UrlError::Scheme),
            ("ftp://files.example", UrlError::Scheme),
            ("file:///etc/passwd", UrlError::Scheme),
            ("unix:/var/run/litellm.sock", UrlError::Scheme),
            ("http://user@gateway.example", UrlError::Userinfo),
            ("http://user:pass@gateway.example", UrlError::Userinfo),
            ("https://gateway.example/v1?api_key=1", UrlError::Query),
            ("https://gateway.example/v1?", UrlError::Query),
            ("https://gateway.example/v1#fragment", UrlError::Fragment),
            ("http://169.254.169.254/latest", UrlError::Metadata),
            ("http://169.254.170.2", UrlError::Metadata),
            ("http://[fe80::1]:4000", UrlError::Metadata),
            ("http://[fd00:ec2::254]", UrlError::Metadata),
            ("http://[::ffff:169.254.169.254]", UrlError::Metadata),
            ("http://2852039166/", UrlError::Metadata),
            ("http://metadata.google.internal", UrlError::Metadata),
            ("http://Metadata.Google.Internal./v1", UrlError::Metadata),
        ] {
            assert_eq!(
                validate_url(refused, &Hosts::default()).err(),
                Some(error),
                "{refused}"
            );
        }
    }

    #[test]
    fn validate_url_accepts_only_the_hosts_the_instance_lists() {
        let hosts = Hosts::parse("api.openai.com, .corp.example, 192.168.1.10");

        for accepted in [
            "https://api.openai.com/v1",
            "https://llm.corp.example/v1",
            "http://192.168.1.10:4000",
        ] {
            assert!(validate_url(accepted, &hosts).is_ok(), "{accepted}");
        }
        for refused in [
            "https://api.anthropic.com/v1",
            "https://corp.example.evil/v1",
            "http://192.168.1.11:4000",
            "http://localhost:11434",
        ] {
            assert_eq!(
                validate_url(refused, &hosts).err(),
                Some(UrlError::Unlisted),
                "{refused}"
            );
        }
        assert_eq!(
            validate_url("http://169.254.169.254", &Hosts::parse("169.254.169.254")).err(),
            Some(UrlError::Metadata),
            "a listed metadata address is still refused"
        );
    }

    #[test]
    fn a_saved_url_the_instance_does_not_list_runs_on_the_instance() {
        let config = Config {
            endpoint_hosts: Hosts::parse(".corp.example"),
            ..config()
        };
        let unlisted = Endpoint::resolve(
            &config,
            &saved(
                PROVIDER_OPENAI,
                Saved {
                    url: Some("https://gateway.example/v1"),
                    key: Some(SAVED_KEY),
                },
            ),
        );
        let listed = Endpoint::resolve(
            &config,
            &saved(
                PROVIDER_OPENAI,
                Saved {
                    url: Some("https://llm.corp.example/v1"),
                    key: Some(SAVED_KEY),
                },
            ),
        );
        let hosted = Endpoint::resolve(
            &config,
            &saved(
                PROVIDER_ANTHROPIC,
                Saved {
                    url: None,
                    key: Some(SAVED_KEY),
                },
            ),
        );

        assert_eq!(unlisted.origin(), Origin::Instance);
        assert_eq!(unlisted.key().expose(), INSTANCE_KEY);
        assert_eq!(listed.url(), "https://llm.corp.example/v1");
        assert_eq!(listed.origin(), Origin::Settings);
        assert_eq!(hosted.url(), ANTHROPIC_URL);
        assert_eq!(hosted.origin(), Origin::Settings);
    }

    #[test]
    fn a_saved_metadata_url_runs_on_the_instance() {
        let config = config();
        for url in [
            "http://169.254.169.254",
            "http://metadata.google.internal",
            "http://[fe80::1]:11434",
        ] {
            for provider in [PROVIDER_SELF_HOSTED, PROVIDER_OPENAI, PROVIDER_ANTHROPIC] {
                let endpoint = Endpoint::resolve(
                    &config,
                    &saved(
                        provider,
                        Saved {
                            url: Some(url),
                            key: Some(SAVED_KEY),
                        },
                    ),
                );

                assert_eq!(endpoint.url(), INSTANCE_HOST, "{provider} {url}");
                assert_eq!(endpoint.origin(), Origin::Instance, "{provider} {url}");
            }
        }
    }

    #[test]
    fn only_a_settings_endpoint_is_trusted_as_a_tenant() {
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

        assert_eq!(
            settings.llm("gpt-4o", 0.2, 64, LlmBackend::Http).trust,
            Trust::Tenant
        );
        assert_eq!(
            Endpoint::instance(&config)
                .llm("alias", 0.2, 64, LlmBackend::Http)
                .trust,
            Trust::Operator
        );
    }

    #[tokio::test]
    async fn a_settings_endpoint_refusal_reports_only_its_status_and_error_message() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": { "message": format!("no route for key {SAVED_KEY}") },
                "reflected": "instance-id: i-0123456789abcdef0",
            })))
            .mount(&server)
            .await;
        let mut settings = settings(PROVIDER_SELF_HOSTED);
        settings.litellm_host = Some(server.uri());
        settings.litellm_key = Some(SecretValue::new(SAVED_KEY));
        let endpoint = Endpoint::resolve(&config(), &settings);

        let failure =
            zone_core::llm::LlmClient::new(endpoint.llm("model", 0.2, 64, LlmBackend::Http))
                .chat(&[zone_core::llm::Message::user("hi")], None)
                .await
                .expect_err("the endpoint refused the turn")
                .to_string();

        assert!(failure.contains("404"), "{failure}");
        assert!(failure.contains("no route for key"), "{failure}");
        assert!(!failure.contains("instance-id"), "{failure}");
        assert!(!failure.contains(SAVED_KEY), "{failure}");
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
        assert_eq!(llm.dialect, Dialect::Anthropic);
    }

    #[test]
    fn each_endpoint_speaks_the_dialect_of_the_api_behind_it() {
        let config = config();
        let key = Saved {
            url: None,
            key: Some(SAVED_KEY),
        };
        let gateway = Saved {
            url: Some("https://gateway.example"),
            key: Some(SAVED_KEY),
        };
        for (provider, values, dialect) in [
            (PROVIDER_OPENAI, key, Dialect::OpenAI),
            (PROVIDER_OPENAI, gateway, Dialect::OpenAI),
            (PROVIDER_OPENAI, NOTHING, Dialect::Compatible),
            (PROVIDER_ANTHROPIC, key, Dialect::Anthropic),
            (PROVIDER_ANTHROPIC, gateway, Dialect::Anthropic),
            (PROVIDER_ANTHROPIC, NOTHING, Dialect::Compatible),
            (PROVIDER_SELF_HOSTED, gateway, Dialect::Compatible),
            (PROVIDER_SELF_HOSTED, NOTHING, Dialect::Compatible),
            (PROVIDER_BEDROCK, gateway, Dialect::Compatible),
        ] {
            let endpoint = Endpoint::resolve(&config, &saved(provider, values));

            assert_eq!(endpoint.dialect(), dialect, "{provider} {:?}", values.url);
            assert_eq!(
                endpoint.llm("model", 0.2, 64, LlmBackend::Http).dialect,
                dialect,
                "{provider} {:?}",
                values.url
            );
        }
    }

    async fn chat_turn(provider: &str, model: &str, temperature: f32) -> serde_json::Value {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": model,
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hi"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let mut settings = settings(provider);
        settings.openai_base_url = Some(server.uri());
        settings.openai_api_key = Some(SecretValue::new(SAVED_KEY));
        settings.anthropic_base_url = Some(server.uri());
        settings.anthropic_api_key = Some(SecretValue::new(SAVED_KEY));
        let endpoint = Endpoint::resolve(&config(), &settings);
        let stops = crate::services::completion_tokens::merge_stops(&["User:".to_string()]);

        let answered =
            zone_core::llm::LlmClient::new(endpoint.llm(model, temperature, 512, LlmBackend::Http))
                .with_stop(stops)
                .chat(&[zone_core::llm::Message::user("hi")], None)
                .await;

        assert!(answered.is_ok(), "{provider} {model}: {answered:?}");
        let requests = server.received_requests().await.unwrap_or_default();
        serde_json::from_slice(&requests[0].body).expect("a JSON body")
    }

    #[tokio::test]
    async fn a_chat_turn_on_openai_is_sent_a_request_openai_accepts() {
        let body = chat_turn(PROVIDER_OPENAI, "gpt-4o", 0.7).await;
        let stops = body["stop"].as_array().cloned().unwrap_or_default();
        assert!(stops.len() <= 4, "{body}");
        assert!(
            stops
                .iter()
                .all(|stop| stop.as_str().is_some_and(|stop| !stop.starts_with("<|"))),
            "{body}"
        );
        assert_eq!(body["max_tokens"], 512, "{body}");

        let body = chat_turn(PROVIDER_OPENAI, "o3-mini", 0.7).await;
        assert!(body.get("stop").is_none(), "{body}");
        assert!(body.get("temperature").is_none(), "{body}");
        assert!(body.get("max_tokens").is_none(), "{body}");
        assert_eq!(body["max_completion_tokens"], 512, "{body}");
    }

    #[tokio::test]
    async fn a_chat_turn_on_anthropic_is_sent_a_temperature_anthropic_accepts() {
        let body = chat_turn(PROVIDER_ANTHROPIC, "claude-sonnet-4-5", 1.5).await;

        assert_eq!(body["temperature"], 1.0, "{body}");
        assert_eq!(body["max_tokens"], 512, "{body}");
    }

    #[tokio::test]
    async fn a_settings_endpoint_lists_no_models_and_assumes_the_configured_context() {
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
        assert_eq!(capacity.source, Source::Configured);
        assert_eq!(capacity.limit, Some(config.chat.context));
        assert_eq!(capacity.ollama, None, "no num_ctx may reach a third party");
        assert!(
            crate::services::chat::session::policy(&config.chat, &capacity)
                .threshold()
                .is_some(),
            "a saved endpoint's history is never compacted"
        );
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
            dialect: Dialect::Compatible,
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
            (
                r#"{"error":"{\"message\":\"invalid x-api-key: ****wxyz\"}"}"#.to_string(),
                "****wxyz".to_string(),
            ),
            (
                "litellm.AuthenticationError: api_key=sk-proj-AbCd****".to_string(),
                "sk-proj-AbCd".to_string(),
            ),
            (
                "rejected key:sk-proj-AbCd****".to_string(),
                "sk-proj-AbCd".to_string(),
            ),
        ] {
            let scrubbed = endpoint.scrub(&reported);

            assert!(!scrubbed.contains(&echo), "{scrubbed}");
            assert!(!scrubbed.contains("wxyz"), "{scrubbed}");
            assert!(!scrubbed.contains("sk-proj-AbCd"), "{scrubbed}");
            assert!(scrubbed.contains(REDACTED), "{scrubbed}");
        }
    }

    #[test]
    fn scrub_leaves_a_failure_without_the_key_alone() {
        let endpoint = Endpoint {
            url: OPENAI_URL.to_string(),
            key: SecretValue::new("plainkey-9999"),
            origin: Origin::Settings,
            dialect: Dialect::Compatible,
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
            dialect: Dialect::Compatible,
        };

        let scrubbed = endpoint.scrub("rejected sk-live0123456789abcdef");

        assert!(!scrubbed.contains("sk-live0123456789abcdef"), "{scrubbed}");
    }
}
