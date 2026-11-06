//! Claude's own sign-in, as `claude setup-token` runs it, through `abnegate-agent-cli`'s
//! `login::claude`. Zone keeps how the tokens are sealed, how a failure is worded, and which
//! token endpoint and proxy the sign-in reaches.

mod explained;
mod sealed;
mod sealing_error;

use abnegate_agent_cli::login::LoginError;
use abnegate_agent_cli::login::claude::Client;
use abnegate_agent_cli::login::claude::Endpoints;

use crate::config::AgentConfig;

pub use explained::Explained;
pub use sealed::Sealed;
pub use sealing_error::SealingError;

/// The path of a bare origin, which names no path of its own.
pub const ROOT_PATH: &str = "/";

/// The variables naming the proxy Claude's token endpoint is reached through, first set first.
pub const PROXY_VARIABLES: [&str; 4] = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

const PROXY_ENDPOINT: &str = "proxy";
const PROXY_REFUSED: &str = "Claude's sign-in cannot go through the server's HTTPS proxy: it must \
                             be an http or https URL with no username or password";
const UNREACHABLE: &str = "Could not reach Claude's sign-in service";

/// Where a Claude sign-in goes: claude.com's own endpoints, with codes exchanged and tokens
/// renewed at the operator's token URL.
pub fn endpoints(agents: &AgentConfig) -> Result<Endpoints, LoginError> {
    Endpoints::default().with_token(&agents.claude_token_url)
}

/// A client for the operator's token endpoint, through the server's HTTPS proxy when it has one.
pub fn client(agents: &AgentConfig) -> Result<Client, LoginError> {
    client_through(agents, proxy(|name| std::env::var(name).ok()))
}

fn client_through(agents: &AgentConfig, proxy: Option<String>) -> Result<Client, LoginError> {
    let client = Client::new(endpoints(agents)?)?;
    match proxy {
        Some(proxy) => client.with_proxy(&proxy),
        None => Ok(client),
    }
}

/// The first of [`PROXY_VARIABLES`] that `variable` names a value for.
fn proxy(variable: impl Fn(&str) -> Option<String>) -> Option<String> {
    PROXY_VARIABLES
        .iter()
        .filter_map(|name| variable(name))
        .find(|value| !value.trim().is_empty())
}

/// Why a Claude sign-in step failed, in Zone's words: a refusal says what to do next, and its
/// status stands in for a refusal Claude did not name.
pub fn described(error: &LoginError) -> String {
    match error {
        LoginError::Rejected {
            refusal: Some(refusal),
            ..
        } => refusal.explained().to_string(),
        LoginError::Rejected { status, .. } => {
            format!("Claude refused the sign-in (HTTP {status}). Start again.")
        }
        LoginError::Transport(reason) => format!("{UNREACHABLE}: {reason}"),
        LoginError::Endpoint {
            name: PROXY_ENDPOINT,
            reason,
            ..
        } => format!("{PROXY_REFUSED} ({reason})"),
        error => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use abnegate_agent_cli::login::claude::Code;
    use abnegate_agent_cli::login::claude::Redirect;
    use abnegate_agent_cli::login::claude::Refusal;
    use abnegate_agent_cli::login::claude::Scope;
    use abnegate_agent_cli::login::claude::Tokens;
    use abnegate_secret::SecretValue;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const TOKEN_PATH: &str = "/v1/oauth/token";
    const MOVED_PATH: &str = "/moved/oauth/token";
    const TOKEN_URL: &str = "http://127.0.0.1:4010/v1/oauth/token";
    const PROXY: &str = "http://proxy.internal:3128";
    const OTHER_PROXY: &str = "http://other.internal:3128";
    const PROXY_PASSWORD: &str = "hunter2-proxy-password";
    const USER_AGENT_PREFIX: &str = "abnegate-agent-cli/";
    const ACCESS: &str = "fake-access-token";
    const LIFETIME: u64 = 28_800;
    const FOUND: u16 = 302;
    const BAD_REQUEST: u16 = 400;
    const UNAVAILABLE: u16 = 503;

    fn agents(token_url: &str) -> AgentConfig {
        AgentConfig {
            claude_token_url: token_url.to_string(),
            ..AgentConfig::default()
        }
    }

    fn environment(variables: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let variables: HashMap<String, String> = variables
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name| variables.get(name).cloned()
    }

    async fn exchanged(server: &MockServer) -> Result<Tokens, LoginError> {
        let client = client_through(&agents(&format!("{}{TOKEN_PATH}", server.uri())), None)
            .expect("a client for a loopback token endpoint");
        client
            .exchange(
                &Code::new("fake-code", "fake-state").expect("a code"),
                &SecretValue::new("fake-verifier"),
                Scope::Inference,
                Redirect::Paste,
            )
            .await
    }

    async fn answering(status: u16, body: Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn the_endpoints_exchange_codes_at_the_operators_token_url() {
        let endpoints = endpoints(&agents(TOKEN_URL)).expect("a loopback token URL");

        assert_eq!(endpoints.token().as_str(), TOKEN_URL);
        assert_eq!(endpoints.authorize(), Endpoints::default().authorize());
        assert_eq!(endpoints.redirect(), Endpoints::default().redirect());
    }

    #[test]
    fn the_login_client_takes_the_servers_https_proxy() {
        for (variables, expected) in [
            (
                vec![("HTTPS_PROXY", PROXY), ("https_proxy", OTHER_PROXY)],
                Some(PROXY),
            ),
            (
                vec![("https_proxy", PROXY), ("ALL_PROXY", OTHER_PROXY)],
                Some(PROXY),
            ),
            (
                vec![("ALL_PROXY", PROXY), ("all_proxy", OTHER_PROXY)],
                Some(PROXY),
            ),
            (vec![("all_proxy", PROXY)], Some(PROXY)),
            (
                vec![("HTTPS_PROXY", " "), ("ALL_PROXY", PROXY)],
                Some(PROXY),
            ),
            (vec![("HTTP_PROXY", PROXY), ("http_proxy", PROXY)], None),
            (vec![("NO_PROXY", "*")], None),
            (vec![], None),
        ] {
            let chosen = proxy(environment(&variables));
            let client = client_through(&agents(TOKEN_URL), chosen).expect("a client");

            assert_eq!(
                client
                    .proxy()
                    .map(|proxy| proxy.origin().ascii_serialization()),
                expected.map(str::to_string),
                "{variables:?}"
            );
        }
    }

    #[test]
    fn a_proxy_with_credentials_fails_the_sign_in_in_zones_words() {
        let credentialed = format!("http://zone:{PROXY_PASSWORD}@proxy.internal:3128");
        let chosen = proxy(environment(&[("HTTPS_PROXY", &credentialed)]));

        let error = client_through(&agents(TOKEN_URL), chosen).expect_err("a credentialed proxy");

        let shown = described(&error);
        assert!(shown.starts_with(PROXY_REFUSED), "{shown}");
        assert!(!shown.contains(PROXY_PASSWORD), "{shown}");
        assert!(!format!("{error:?}").contains(PROXY_PASSWORD), "{error:?}");
    }

    #[tokio::test]
    async fn the_token_exchange_names_abnegate_agent_cli_as_its_user_agent() {
        let server = answering(
            200,
            json!({ "access_token": ACCESS, "expires_in": LIFETIME }),
        )
        .await;

        exchanged(&server).await.expect("an exchange");

        let requests = server.received_requests().await.expect("recorded requests");
        let agents: Vec<&str> = requests
            .iter()
            .filter_map(|request| request.headers.get("user-agent"))
            .filter_map(|agent| agent.to_str().ok())
            .collect();
        assert_eq!(agents.len(), 1, "{agents:?}");
        assert!(agents[0].starts_with(USER_AGENT_PREFIX), "{agents:?}");
    }

    #[tokio::test]
    async fn a_redirect_from_the_token_endpoint_is_not_followed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(
                ResponseTemplate::new(FOUND)
                    .insert_header("location", format!("{}{MOVED_PATH}", server.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(path(MOVED_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "access_token": ACCESS, "expires_in": LIFETIME })),
            )
            .expect(0)
            .mount(&server)
            .await;

        let error = exchanged(&server).await.expect_err("a redirect");

        assert!(
            matches!(
                error,
                LoginError::Rejected {
                    status: FOUND,
                    refusal: None,
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(
            described(&error),
            "Claude refused the sign-in (HTTP 302). Start again."
        );
    }

    #[tokio::test]
    async fn a_refused_exchange_is_worded_by_its_refusal() {
        for (error, description, refusal) in [
            ("invalid_grant", None, Refusal::Declined),
            ("invalid_scope", None, Refusal::Scope),
            ("access_denied", Some("account_on_hold"), Refusal::OnHold),
            ("temporarily_unavailable", None, Refusal::Unavailable),
        ] {
            let server = answering(
                BAD_REQUEST,
                json!({
                    "error": error,
                    "error_description": description.unwrap_or("Invalid <b>authorization</b> code"),
                }),
            )
            .await;

            let refused = exchanged(&server).await.expect_err("a refusal");

            assert_eq!(described(&refused), refusal.explained(), "{error}");
        }

        let outage = answering(UNAVAILABLE, json!({ "message": "upstream down" })).await;
        let refused = exchanged(&outage).await.expect_err("an outage");
        assert_eq!(
            described(&refused),
            "Claude refused the sign-in (HTTP 503). Start again.",
            "a refusal Claude did not name is worded by its status"
        );
    }

    #[test]
    fn an_unreachable_token_endpoint_is_named_as_zone_named_it() {
        assert_eq!(
            described(&LoginError::Transport("connection refused".to_string())),
            "Could not reach Claude's sign-in service: connection refused"
        );
    }
}
