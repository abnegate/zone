//! Chat web search settings: the shared SearXNG client's config with zone's
//! defaults and the VPN gate.

use std::env;

use abnegate_search::WebSearchConfig;

/// SearXNG shares Gluetun's network namespace, so the host is `gluetun`, not
/// `searxng`.
pub const DEFAULT_QUERY_URL: &str = "http://gluetun:8080/search?q=<query>&format=json";

const ENABLED_VARIABLE: &str = "SEARCH_ENABLE_WEB_SEARCH";
const QUERY_URL_VARIABLE: &str = "SEARCH_SEARXNG_QUERY_URL";

/// Switched off, against SearXNG in Gluetun's namespace.
pub fn defaults() -> WebSearchConfig {
    WebSearchConfig::new(DEFAULT_QUERY_URL).with_enabled(false)
}

/// The `SEARCH_*` settings, on unless `SEARCH_ENABLE_WEB_SEARCH` turns them
/// off, and held off while public web must wait for the VPN
/// (`zone_core::vpn::allows_public`).
pub fn from_environment() -> WebSearchConfig {
    let mut config = WebSearchConfig::from_environment();
    if env::var(QUERY_URL_VARIABLE).is_err() {
        config.query_url = DEFAULT_QUERY_URL.to_string();
    }
    let switched_on = env::var(ENABLED_VARIABLE).is_err() || config.enabled;
    config.with_enabled(switched_on && zone_core::vpn::allows_public())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_search::SearchContext;
    use std::time::Duration;
    use zone_core::vpn::Hold;

    const RESULT_COUNT_VARIABLE: &str = "SEARCH_RESULT_COUNT";
    const TIMEOUT_VARIABLE: &str = "SEARCH_TIMEOUT_SECONDS";
    const VPN: &str = "ZONE_VPN";
    const REQUIRED: &str = "ZONE_VPN_REQUIRED";
    const WIREGUARD: &str = "VPN_WIREGUARD_PRIVATE_KEY";
    const OPENVPN: &str = "VPN_OPENVPN_USER";
    const NAMES: [&str; 4] = [
        ENABLED_VARIABLE,
        QUERY_URL_VARIABLE,
        RESULT_COUNT_VARIABLE,
        TIMEOUT_VARIABLE,
    ];

    const RECENCY: &str = "What is the latest news on OpenAI?";

    /// `hold` with the `SEARCH_*` variables cleared under its lock.
    fn isolated(mut hold: Hold) -> Hold {
        hold.variables().clear(&NAMES);
        hold
    }

    fn force_on() -> serde_json::Value {
        serde_json::json!({ "web_search": true })
    }

    #[test]
    fn defaults_are_switched_off_against_searxng_in_gluetun() {
        let config = defaults();
        assert!(!config.enabled);
        assert_eq!(
            config.query_url,
            "http://gluetun:8080/search?q=<query>&format=json"
        );
        assert_ne!(config.query_url, abnegate_search::DEFAULT_SEARXNG_QUERY_URL);
        assert_eq!(config.result_count, 5);
        assert_eq!(config.timeout, Duration::from_secs(15));
        assert_eq!(SearchContext::new(&config), SearchContext::Disabled);
    }

    #[test]
    fn from_environment_is_on_against_gluetun_when_nothing_is_set() {
        let _held = isolated(Hold::off());
        let config = from_environment();
        assert!(
            config.enabled,
            "zone searches unless told not to, unlike the crate: {config:?}"
        );
        assert_eq!(config.query_url, DEFAULT_QUERY_URL);
    }

    #[test]
    fn from_environment_keeps_a_configured_query_url() {
        let mut held = isolated(Hold::off());
        held.variables()
            .set(QUERY_URL_VARIABLE, "http://searxng.test/search?q=<query>");
        assert_eq!(
            from_environment().query_url,
            "http://searxng.test/search?q=<query>"
        );
        held.variables().set(QUERY_URL_VARIABLE, "");
        let blank = from_environment();
        assert_eq!(blank.query_url, "");
        assert_eq!(SearchContext::new(&blank), SearchContext::Disabled);
    }

    #[test]
    fn from_environment_stays_off_when_the_vpn_is_required_and_not_on() {
        let _held = isolated(Hold::required_off());
        let config = from_environment();
        assert!(!config.enabled);
        assert!(!config.requested_for(RECENCY, None));
        assert!(!config.requested_for("anything", Some(&force_on())));
        assert_eq!(SearchContext::new(&config), SearchContext::Disabled);
    }

    #[test]
    fn from_environment_allows_search_when_the_vpn_is_not_required() {
        let _held = isolated(Hold::off());
        let config = from_environment();
        assert!(config.enabled);
        assert!(config.requested_for(RECENCY, None));
        assert_eq!(SearchContext::new(&config), SearchContext::NotRequested);
    }

    #[test]
    fn from_environment_stays_off_when_vpn_credentials_are_present() {
        let mut held = isolated(Hold::configured_off());
        assert!(!from_environment().enabled);
        held.variables().set(WIREGUARD, "");
        held.variables().set(OPENVPN, "user");
        assert!(!from_environment().enabled);
    }

    #[test]
    fn from_environment_allows_search_when_required_is_off_even_with_credentials() {
        let mut held = isolated(Hold::configured_off());
        held.variables().set(REQUIRED, "0");
        assert!(from_environment().enabled);
    }

    #[test]
    fn from_environment_turns_on_when_the_vpn_is_on() {
        let _held = isolated(Hold::on());
        let config = from_environment();
        assert!(config.enabled);
        assert!(config.requested_for(RECENCY, None));
        assert_eq!(SearchContext::new(&config), SearchContext::NotRequested);
    }

    #[test]
    fn from_environment_stays_off_when_search_is_disabled_even_with_vpn() {
        let mut held = isolated(Hold::on());
        held.variables().set(ENABLED_VARIABLE, "false");
        let config = from_environment();
        assert!(!config.enabled);
        assert!(!config.requested_for(RECENCY, Some(&force_on())));
        assert_eq!(SearchContext::new(&config), SearchContext::Disabled);
    }

    #[test]
    fn from_environment_treats_empty_and_zero_vpn_as_off_when_required() {
        let mut held = isolated(Hold::required_off());
        held.variables().set(ENABLED_VARIABLE, "true");
        held.variables().set(VPN, "");
        assert!(!from_environment().enabled);
        held.variables().set(VPN, "0");
        assert!(!from_environment().enabled);
    }
}
