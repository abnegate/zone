//! Claude's own sign-in, as `claude setup-token` runs it, through `abnegate-agent-cli`'s
//! `login::claude`. Zone keeps how the tokens are sealed, how a failure is worded, and which
//! token endpoint and proxy the sign-in reaches.

mod clients;
mod explained;
mod sealed;
mod sealing_error;
mod unbuilt;

use abnegate_agent_cli::login::LoginError;
use abnegate_agent_cli::login::claude::Client;
use abnegate_agent_cli::login::claude::Endpoints;
use abnegate_agent_cli::login::claude::Refusal;
use hyper::Uri;
use hyper_util::client::proxy::matcher::Matcher;
use reqwest::StatusCode;
use reqwest::Url;

use crate::config::AgentConfig;
use clients::Clients;

pub use explained::Explained;
pub use sealed::Sealed;
pub use sealing_error::SealingError;
pub use unbuilt::Unbuilt;

/// The path of a bare origin, which names no path of its own.
pub const ROOT_PATH: &str = "/";

/// The endpoint a refused proxy is named as.
pub(super) const PROXY_ENDPOINT: &str = "proxy";

const PROXY_REFUSED: &str = "Claude's sign-in cannot go through the server's proxy: \
                             HTTPS_PROXY, HTTP_PROXY and ALL_PROXY must be http or https URLs \
                             with no username or password, or NO_PROXY must list Claude's token \
                             host";
const UNPROXIABLE: &str = "Claude's token URL cannot be matched against the server's proxy rules";
const UNREACHABLE: &str = "Could not reach Claude's sign-in service";

static CLIENTS: Clients = Clients::new();

/// Where a Claude sign-in goes: claude.com's own endpoints, with codes exchanged and tokens
/// renewed at the operator's token URL.
pub fn endpoints(agents: &AgentConfig) -> Result<Endpoints, LoginError> {
    Endpoints::default().with_token(&agents.claude_token_url)
}

/// The client for the operator's token endpoint, through the proxy the server's environment
/// names for it, as any other HTTP client of the server would choose it.
pub fn client(agents: &AgentConfig) -> Result<Client, Unbuilt> {
    client_through(agents, &Matcher::from_system())
}

/// [`client`], through the proxy `proxies` choose for the token endpoint. The client is shared
/// by every sign-in while the token endpoint and its proxy stay the same.
pub(super) fn client_through(agents: &AgentConfig, proxies: &Matcher) -> Result<Client, Unbuilt> {
    let endpoints = endpoints(agents)?;
    let proxy = proxy(proxies, endpoints.token())?;
    let token = endpoints.token().to_string();
    CLIENTS.get(&token, proxy.as_deref(), || {
        let client = Client::new(endpoints)?;
        match &proxy {
            Some(proxy) => Ok(client.with_proxy(proxy)?),
            None => Ok(client),
        }
    })
}

/// The proxy `proxies` choose for `token`: by its scheme, unless its host is one the proxies
/// bypass.
fn proxy(proxies: &Matcher, token: &Url) -> Result<Option<String>, Unbuilt> {
    let token: Uri = token
        .as_str()
        .parse()
        .map_err(|_| LoginError::Malformed(UNPROXIABLE))?;
    match proxies.intercept(&token) {
        None => Ok(None),
        Some(intercept) if intercept.basic_auth().is_some() || intercept.raw_auth().is_some() => {
            Err(Unbuilt::CredentialedProxy)
        }
        Some(intercept) => Ok(Some(intercept.uri().to_string())),
    }
}

/// Why a Claude sign-in step failed, in Zone's words: a refusal says what to do next, and its
/// status stands in for a refusal Claude did not name.
pub fn described(error: &LoginError) -> String {
    match error {
        LoginError::Rejected {
            refusal: Some(refusal),
            ..
        } => refusal.explained().to_string(),
        LoginError::Rejected { status, .. } if unavailable(*status) => {
            Refusal::Unavailable.explained().to_string()
        }
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

fn unavailable(status: u16) -> bool {
    StatusCode::from_u16(status).is_ok_and(|status| status.is_server_error())
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::login::claude::Code;
    use abnegate_agent_cli::login::claude::Redirect;
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
    const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
    const LAN_TOKEN_URL: &str = "http://10.0.0.5/v1/oauth/token";
    const HTTP_PROXY: &str = "http://plain.internal:3128";
    const HTTPS_PROXY: &str = "http://secure.internal:3128";
    const ALL_PROXY: &str = "http://every.internal:3128";
    const SOCKS_PROXY: &str = "socks5://socks.internal:1080";
    const PROXY_PASSWORD: &str = "hunter2-proxy-password";
    const USER_AGENT_PREFIX: &str = "abnegate-agent-cli/";
    const ACCESS: &str = "fake-access-token";
    const LIFETIME: u64 = 28_800;
    const FOUND: u16 = 302;
    const BAD_REQUEST: u16 = 400;
    const SERVER_ERRORS: [u16; 3] = [500, 502, 503];

    fn agents(token_url: &str) -> AgentConfig {
        AgentConfig {
            claude_token_url: token_url.to_string(),
            ..AgentConfig::default()
        }
    }

    fn proxies(http: &str, https: &str, all: &str, no: &str) -> Matcher {
        Matcher::builder()
            .http(http)
            .https(https)
            .all(all)
            .no(no)
            .build()
    }

    fn chosen(token_url: &str, proxies: &Matcher) -> Option<String> {
        client_through(&agents(token_url), proxies)
            .expect("a client")
            .proxy()
            .map(|proxy| proxy.origin().ascii_serialization())
    }

    async fn exchanged(server: &MockServer) -> Result<Tokens, LoginError> {
        let client = client_through(
            &agents(&format!("{}{TOKEN_PATH}", server.uri())),
            &Matcher::builder().build(),
        )
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
    fn the_token_urls_scheme_chooses_the_proxy() {
        for (token_url, http, https, all, expected) in [
            (
                TOKEN_URL,
                HTTP_PROXY,
                HTTPS_PROXY,
                ALL_PROXY,
                Some(HTTP_PROXY),
            ),
            (
                CLAUDE_TOKEN_URL,
                HTTP_PROXY,
                HTTPS_PROXY,
                ALL_PROXY,
                Some(HTTPS_PROXY),
            ),
            (TOKEN_URL, "", HTTPS_PROXY, "", None),
            (CLAUDE_TOKEN_URL, HTTP_PROXY, "", "", None),
            (TOKEN_URL, "", HTTPS_PROXY, ALL_PROXY, Some(ALL_PROXY)),
            (CLAUDE_TOKEN_URL, HTTP_PROXY, "", ALL_PROXY, Some(ALL_PROXY)),
            (CLAUDE_TOKEN_URL, "", "", "", None),
        ] {
            let proxies = proxies(http, https, all, "");

            assert_eq!(
                chosen(token_url, &proxies).as_deref(),
                expected,
                "{token_url} http={http} https={https} all={all}"
            );
        }
    }

    #[test]
    fn a_token_host_the_server_lists_in_no_proxy_bypasses_the_proxy() {
        for (token_url, no, bypassed) in [
            (CLAUDE_TOKEN_URL, "*", true),
            (CLAUDE_TOKEN_URL, "claude.com", true),
            (CLAUDE_TOKEN_URL, ".claude.com", true),
            (CLAUDE_TOKEN_URL, "platform.claude.com", true),
            (CLAUDE_TOKEN_URL, "example.com, claude.com", true),
            (CLAUDE_TOKEN_URL, "notclaude.com", false),
            (CLAUDE_TOKEN_URL, "api.claude.com", false),
            (CLAUDE_TOKEN_URL, "", false),
            (LAN_TOKEN_URL, "10.0.0.5", true),
            (LAN_TOKEN_URL, "10.0.0.0/8", true),
            (LAN_TOKEN_URL, "10.0.0.6", false),
        ] {
            let proxies = proxies(HTTP_PROXY, HTTPS_PROXY, "", no);

            assert_eq!(
                chosen(token_url, &proxies).is_none(),
                bypassed,
                "{token_url} NO_PROXY={no}"
            );
        }
    }

    #[test]
    fn a_proxy_with_credentials_is_refused_in_zones_words() {
        for credentialed in [
            format!("http://zone:{PROXY_PASSWORD}@secure.internal:3128"),
            format!("http://{PROXY_PASSWORD}@secure.internal:3128"),
        ] {
            let proxies = proxies("", &credentialed, "", "");

            let error = client_through(&agents(CLAUDE_TOKEN_URL), &proxies)
                .expect_err("a credentialed proxy");

            assert!(
                matches!(error, Unbuilt::CredentialedProxy),
                "{credentialed}: {error:?}"
            );
            assert_eq!(error.to_string(), PROXY_REFUSED);
            assert!(!format!("{error:?}").contains(PROXY_PASSWORD), "{error:?}");
        }
    }

    #[test]
    fn a_credentialed_proxy_the_token_host_bypasses_is_never_used() {
        let credentialed = format!("http://zone:{PROXY_PASSWORD}@secure.internal:3128");
        let proxies = proxies("", &credentialed, "", "claude.com");

        assert_eq!(chosen(CLAUDE_TOKEN_URL, &proxies), None);
    }

    #[test]
    fn a_proxy_the_sign_in_client_refuses_is_named_in_zones_words() {
        let proxies = proxies("", SOCKS_PROXY, "", "");

        let error = client_through(&agents(CLAUDE_TOKEN_URL), &proxies).expect_err("a socks proxy");

        assert!(
            matches!(
                &error,
                Unbuilt::Login(LoginError::Endpoint {
                    name: PROXY_ENDPOINT,
                    ..
                })
            ),
            "{error:?}"
        );
        assert!(error.to_string().starts_with(PROXY_REFUSED), "{error}");
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
    }

    #[tokio::test]
    async fn an_unavailable_service_is_worded_as_unavailable_without_starting_again() {
        for status in SERVER_ERRORS {
            let outage = answering(status, json!({ "message": "upstream down" })).await;

            let refused = exchanged(&outage).await.expect_err("an outage");

            let shown = described(&refused);
            assert_eq!(shown, Refusal::Unavailable.explained(), "HTTP {status}");
            assert!(!shown.contains("Start again"), "HTTP {status}: {shown}");
        }
    }

    #[test]
    fn an_unreachable_token_endpoint_is_named_as_zone_named_it() {
        assert_eq!(
            described(&LoginError::Transport("connection refused".to_string())),
            "Could not reach Claude's sign-in service: connection refused"
        );
    }
}
