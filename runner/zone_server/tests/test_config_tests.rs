mod common;

use std::net::IpAddr;
use zone_server::config::Config;

fn assert_usage_on_loopback(config: Config) {
    for url in [config.agents.claude_api_url, config.agents.codex_api_url] {
        let parsed = reqwest::Url::parse(&url).expect("a usage URL");
        let loopback = parsed
            .host_str()
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        assert!(loopback, "{url} would reach a real agent's API from a test");
    }
}

#[test]
fn a_shared_test_config_reads_agent_usage_only_from_loopback() {
    assert_usage_on_loopback(common::test_config());
}

#[test]
fn a_test_config_on_another_ollama_host_reads_agent_usage_only_from_loopback() {
    assert_usage_on_loopback(common::test_config_with_ollama_host(
        "http://192.0.2.1:11434",
    ));
}
