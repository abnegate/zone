//! Settings read from `SEARCH_*` environment variables.

use std::env;

fn env_truthy(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(s) => matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

/// Default SearXNG query URL. SearXNG shares Gluetun's network namespace, so
/// the hostname is `gluetun`, not `searxng`.
pub const DEFAULT_SEARXNG_QUERY_URL: &str = "http://gluetun:8080/search?q=<query>&format=json";

/// Zone chat web search settings loaded from `SEARCH_*` env vars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSearchConfig {
    /// Master switch. When false, chat never calls SearXNG.
    pub enabled: bool,
    /// Query URL template. `<query>` or `{query}` is replaced with the
    /// URL-encoded search string.
    pub query_url: String,
    /// Max results injected into the prompt (1–20)
    pub result_count: usize,
    /// HTTP timeout for a single SearXNG request
    pub timeout_secs: u64,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            query_url: DEFAULT_SEARXNG_QUERY_URL.to_string(),
            result_count: 5,
            timeout_secs: 15,
        }
    }
}

impl WebSearchConfig {
    /// Load from `SEARCH_*` and `ZONE_VPN`. Missing values use the Compose
    /// defaults (`SEARCH_ENABLE_WEB_SEARCH=true`, the Gluetun SearXNG URL).
    /// Lookups stay off unless the VPN tunnel is on.
    pub fn from_env() -> Self {
        let result_count = env::var("SEARCH_RESULT_COUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5)
            .clamp(1, 20);
        let timeout_secs = env::var("SEARCH_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15)
            .clamp(1, 60);
        Self {
            enabled: env_truthy("SEARCH_ENABLE_WEB_SEARCH", true) && env_truthy("ZONE_VPN", false),
            query_url: env::var("SEARCH_SEARXNG_QUERY_URL")
                .unwrap_or_else(|_| DEFAULT_SEARXNG_QUERY_URL.to_string()),
            result_count,
            timeout_secs,
        }
    }

    /// Whether this chat message should trigger a SearXNG lookup.
    ///
    /// When the server switch is on, search runs only when the message looks
    /// like it needs current web information. A boolean `metadata.web_search`
    /// value can force a lookup on or off for a single message.
    pub fn requested_for(&self, content: &str, metadata: Option<&serde_json::Value>) -> bool {
        if !self.enabled || self.query_url.trim().is_empty() {
            return false;
        }
        match metadata.and_then(|m| m.get("web_search")) {
            Some(v) if v.is_boolean() => v.as_bool() == Some(true),
            _ => crate::client::needs_web_search(content),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SearchContext;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    static ENVIRONMENT: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
    }

    struct Isolated(Vec<(&'static str, Option<OsString>)>);

    impl Isolated {
        fn new(names: &[&'static str]) -> Self {
            let saved = names
                .iter()
                .map(|name| (*name, env::var_os(name)))
                .collect();
            for name in names {
                // SAFETY: every environment-mutating test in this module holds
                // ENVIRONMENT for the guard's lifetime.
                unsafe { env::remove_var(name) };
            }
            Self(saved)
        }

        fn set(name: &str, value: &str) {
            // SAFETY: the caller still holds ENVIRONMENT.
            unsafe { env::set_var(name, value) };
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                // SAFETY: the caller still holds ENVIRONMENT while the saved
                // process environment is restored.
                unsafe {
                    match value {
                        Some(value) => env::set_var(name, value),
                        None => env::remove_var(name),
                    }
                }
            }
        }
    }

    const NAMES: &[&str] = &[
        "SEARCH_ENABLE_WEB_SEARCH",
        "SEARCH_SEARXNG_QUERY_URL",
        "SEARCH_RESULT_COUNT",
        "SEARCH_TIMEOUT_SECS",
        "ZONE_VPN",
    ];

    fn recency() -> &'static str {
        "What is the latest news on OpenAI?"
    }

    fn force_on() -> serde_json::Value {
        serde_json::json!({ "web_search": true })
    }

    #[test]
    fn from_env_stays_off_when_the_vpn_is_not_on() {
        let _lock = lock();
        let _environment = Isolated::new(NAMES);
        let config = WebSearchConfig::from_env();
        assert!(!config.enabled);
        assert!(!config.requested_for(recency(), None));
        assert!(!config.requested_for("anything", Some(&force_on())));
        assert_eq!(SearchContext::new(&config), SearchContext::Disabled);
    }

    #[test]
    fn from_env_turns_on_when_the_vpn_is_on() {
        let _lock = lock();
        let _environment = Isolated::new(NAMES);
        Isolated::set("ZONE_VPN", "1");
        let config = WebSearchConfig::from_env();
        assert!(config.enabled);
        assert!(config.requested_for(recency(), None));
        assert_eq!(SearchContext::new(&config), SearchContext::NotRequested);
    }

    #[test]
    fn from_env_stays_off_when_search_is_disabled_even_with_vpn() {
        let _lock = lock();
        let _environment = Isolated::new(NAMES);
        Isolated::set("ZONE_VPN", "1");
        Isolated::set("SEARCH_ENABLE_WEB_SEARCH", "false");
        let config = WebSearchConfig::from_env();
        assert!(!config.enabled);
        assert!(!config.requested_for(recency(), Some(&force_on())));
        assert_eq!(SearchContext::new(&config), SearchContext::Disabled);
    }

    #[test]
    fn from_env_treats_empty_and_zero_vpn_as_off() {
        let _lock = lock();
        let _environment = Isolated::new(NAMES);
        Isolated::set("SEARCH_ENABLE_WEB_SEARCH", "true");
        Isolated::set("ZONE_VPN", "");
        assert!(!WebSearchConfig::from_env().enabled);
        Isolated::set("ZONE_VPN", "0");
        assert!(!WebSearchConfig::from_env().enabled);
    }
}
