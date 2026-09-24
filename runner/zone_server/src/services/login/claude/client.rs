//! Exchanging a pasted code for tokens, and renewing them, at Claude's token endpoint.

use std::sync::LazyLock;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::header::ACCEPT;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use zone_core::secret::{REDACTED, SecretValue, redact};

use super::{CLIENT_ID, Code, Error, REDIRECT_URL, Scope, Tokens};

const TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT: &str = "Zone-agent-login";
const JSON: &str = "application/json";
const AUTHORIZATION_CODE: &str = "authorization_code";
const REFRESH_TOKEN: &str = "refresh_token";
const UNREADABLE: &str = "Claude's token endpoint sent a response Zone could not read";
const UNRENEWABLE: &str = "This Claude sign-in has no refresh token";
const UNDESCRIBED: &str = "no reason given";

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(USER_AGENT)
        .build()
        .expect("the Claude token client builds")
});

pub struct Client {
    http: reqwest::Client,
    endpoint: Url,
}

#[derive(Serialize)]
struct Exchange<'a> {
    grant_type: &'static str,
    code: &'a str,
    redirect_uri: &'static str,
    client_id: &'static str,
    code_verifier: &'a str,
    state: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in: Option<u64>,
}

#[derive(Serialize)]
struct Refresh<'a> {
    grant_type: &'static str,
    refresh_token: &'a str,
    client_id: &'static str,
    scope: &'a str,
}

#[derive(Deserialize)]
struct Granted {
    access_token: SecretValue,
    refresh_token: Option<SecretValue>,
    expires_in: u64,
    scope: Option<String>,
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Refusal {
    error: Option<Reason>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Reason {
    Code(String),
    Detail { message: String },
}

impl Client {
    pub fn new(endpoint: Url) -> Self {
        Self {
            http: HTTP.clone(),
            endpoint,
        }
    }

    pub async fn exchange(
        &self,
        code: &Code,
        verifier: &SecretValue,
        scope: Scope,
    ) -> Result<Tokens, Error> {
        let request = Exchange {
            grant_type: AUTHORIZATION_CODE,
            code: code.value.expose(),
            redirect_uri: REDIRECT_URL,
            client_id: CLIENT_ID,
            code_verifier: verifier.expose(),
            state: &code.state,
            expires_in: scope.lifetime(),
        };
        let now = Utc::now();
        let mut tokens = self
            .grant(&request, &[code.value.expose(), verifier.expose()])
            .await?
            .tokens(now)?;
        if tokens.scope.is_empty() {
            tokens.scope = scope.parameter();
        }
        Ok(tokens)
    }

    pub async fn refresh(&self, tokens: &Tokens) -> Result<Tokens, Error> {
        let refresh = tokens
            .refresh
            .as_ref()
            .ok_or(Error::Malformed(UNRENEWABLE))?;
        let request = Refresh {
            grant_type: REFRESH_TOKEN,
            refresh_token: refresh.expose(),
            client_id: CLIENT_ID,
            scope: &tokens.scope,
        };
        let now = Utc::now();
        let mut renewed = self
            .grant(&request, &[refresh.expose()])
            .await?
            .tokens(now)?;
        renewed.refresh = renewed.refresh.or_else(|| tokens.refresh.clone());
        if renewed.scope.is_empty() {
            renewed.scope.clone_from(&tokens.scope);
        }
        renewed.subscription = renewed.subscription.or_else(|| tokens.subscription.clone());
        Ok(renewed)
    }

    async fn grant(&self, request: &impl Serialize, sent: &[&str]) -> Result<Granted, Error> {
        let response = self
            .http
            .post(self.endpoint.clone())
            .header(ACCEPT, JSON)
            .json(request)
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        let body = SecretValue::new(response.text().await.map_err(transport)?);
        if !status.is_success() {
            return Err(rejection(status, body.expose(), sent));
        }
        serde_json::from_str(body.expose()).map_err(|_| Error::Malformed(UNREADABLE))
    }
}

impl Granted {
    fn tokens(self, now: DateTime<Utc>) -> Result<Tokens, Error> {
        let expires_at = i64::try_from(self.expires_in)
            .ok()
            .and_then(TimeDelta::try_seconds)
            .and_then(|lifetime| now.checked_add_signed(lifetime))
            .ok_or(Error::Malformed(UNREADABLE))?;
        if self.access_token.is_empty() {
            return Err(Error::Malformed(UNREADABLE));
        }
        Ok(Tokens {
            access: self.access_token,
            refresh: self.refresh_token.filter(|token| !token.is_empty()),
            expires_at,
            issued_at: Some(now),
            scope: self.scope.unwrap_or_default(),
            subscription: self.subscription_type.filter(|kind| !kind.is_empty()),
        })
    }
}

impl Refusal {
    fn description(self) -> Option<String> {
        let reason = self.error.map(|reason| match reason {
            Reason::Code(code) => code,
            Reason::Detail { message } => message,
        });
        [self.error_description, reason]
            .into_iter()
            .flatten()
            .find(|text| !text.trim().is_empty())
    }
}

fn rejection(status: StatusCode, body: &str, sent: &[&str]) -> Error {
    let described = serde_json::from_str::<Refusal>(body)
        .ok()
        .and_then(Refusal::description)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or(UNDESCRIBED).to_string());
    let scrubbed = sent
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(described, |text, secret| text.replace(secret, REDACTED));
    Error::Rejected {
        status: status.as_u16(),
        message: redact(&scrubbed).into_owned(),
    }
}

fn transport(error: reqwest::Error) -> Error {
    let error = error.without_url();
    let causes: Vec<String> = std::iter::successors(
        Some(&error as &(dyn std::error::Error + 'static)),
        |cause| cause.source(),
    )
    .map(ToString::to_string)
    .collect();
    Error::Transport(redact(&causes.join(": ")).into_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::services::login::claude::{LIFETIME, token_endpoint};

    const TOKEN_PATH: &str = "/v1/oauth/token";
    const CODE: &str = "fake-authorization-code";
    const STATE: &str = "fake-state";
    const VERIFIER: &str = "fake-code-verifier";
    const ACCESS: &str = "fake-access-token";
    const REFRESH: &str = "fake-refresh-token";
    const GRANTED: i64 = 28_800;

    async fn answering(status: u16, body: Value) -> (MockServer, Client) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let endpoint = token_endpoint(&format!("{}{TOKEN_PATH}", server.uri()))
            .expect("a loopback token endpoint");
        (server, Client::new(endpoint))
    }

    async fn sent(server: &MockServer) -> Value {
        let requests = server
            .received_requests()
            .await
            .expect("the server records requests");
        assert_eq!(requests.len(), 1, "one request per call");
        requests[0].body_json().expect("a JSON body")
    }

    fn code() -> Code {
        Code {
            value: SecretValue::new(CODE),
            state: STATE.to_string(),
        }
    }

    fn verifier() -> SecretValue {
        SecretValue::new(VERIFIER)
    }

    fn earlier() -> Tokens {
        Tokens {
            access: SecretValue::new(ACCESS),
            refresh: Some(SecretValue::new(REFRESH)),
            expires_at: Utc::now(),
            issued_at: None,
            scope: "user:profile user:inference".to_string(),
            subscription: Some("max".to_string()),
        }
    }

    #[tokio::test]
    async fn an_inference_exchange_asks_for_a_year_long_token() {
        let (server, client) = answering(
            200,
            json!({
                "access_token": ACCESS,
                "refresh_token": REFRESH,
                "expires_in": LIFETIME,
                "scope": "user:inference",
                "token_type": "Bearer",
            }),
        )
        .await;
        let before = Utc::now();

        let tokens = client
            .exchange(&code(), &verifier(), Scope::Inference)
            .await
            .expect("exchange");

        assert_eq!(
            sent(&server).await,
            json!({
                "grant_type": "authorization_code",
                "code": CODE,
                "redirect_uri": REDIRECT_URL,
                "client_id": CLIENT_ID,
                "code_verifier": VERIFIER,
                "state": STATE,
                "expires_in": 31_536_000,
            })
        );
        assert_eq!(tokens.access.expose(), ACCESS);
        assert_eq!(
            tokens.refresh.as_ref().map(SecretValue::expose),
            Some(REFRESH)
        );
        assert_eq!(tokens.scope, "user:inference");
        let lifetime = TimeDelta::seconds(31_536_000);
        assert!(
            tokens.expires_at >= before + lifetime && tokens.expires_at <= Utc::now() + lifetime,
            "{}",
            tokens.expires_at
        );
        assert_eq!(tokens.lifetime(), Some(lifetime));
    }

    #[tokio::test]
    async fn a_full_exchange_leaves_the_lifetime_to_claude() {
        let (server, client) =
            answering(200, json!({"access_token": ACCESS, "expires_in": GRANTED})).await;

        let tokens = client
            .exchange(&code(), &verifier(), Scope::Full)
            .await
            .expect("exchange");

        let body = sent(&server).await;
        assert!(
            body.get("expires_in").is_none(),
            "a full-access exchange must not ask for a lifetime: {body}"
        );
        assert_eq!(
            body,
            json!({
                "grant_type": "authorization_code",
                "code": CODE,
                "redirect_uri": REDIRECT_URL,
                "client_id": CLIENT_ID,
                "code_verifier": VERIFIER,
                "state": STATE,
            })
        );
        assert_eq!(
            tokens.scope,
            Scope::Full.parameter(),
            "a grant that names no scope keeps the scope asked for"
        );
        assert!(tokens.refresh.is_none());
        assert_eq!(tokens.lifetime(), Some(TimeDelta::seconds(GRANTED)));
    }

    #[tokio::test]
    async fn a_refresh_sends_the_scope_and_keeps_a_refresh_token_it_was_not_sent_again() {
        let (server, client) = answering(
            200,
            json!({"access_token": "fake-renewed-access-token", "expires_in": GRANTED}),
        )
        .await;
        let earlier = earlier();

        let renewed = client.refresh(&earlier).await.expect("refresh");

        assert_eq!(
            sent(&server).await,
            json!({
                "grant_type": "refresh_token",
                "refresh_token": REFRESH,
                "client_id": CLIENT_ID,
                "scope": "user:profile user:inference",
            })
        );
        assert_eq!(renewed.access.expose(), "fake-renewed-access-token");
        assert_eq!(
            renewed.refresh, earlier.refresh,
            "Claude sent no new refresh token, so the old one stays"
        );
        assert_eq!(renewed.scope, earlier.scope);
        assert_eq!(renewed.subscription, earlier.subscription);
        assert_eq!(
            renewed.lifetime(),
            Some(TimeDelta::seconds(GRANTED)),
            "a renewal keeps the lifetime Claude granted it"
        );
    }

    #[tokio::test]
    async fn a_refresh_that_rotates_the_refresh_token_keeps_the_new_one() {
        let (_server, client) = answering(
            200,
            json!({
                "access_token": "fake-renewed-access-token",
                "refresh_token": "fake-rotated-refresh-token",
                "expires_in": GRANTED,
            }),
        )
        .await;

        let renewed = client.refresh(&earlier()).await.expect("refresh");

        assert_eq!(
            renewed.refresh.as_ref().map(SecretValue::expose),
            Some("fake-rotated-refresh-token")
        );
    }

    #[tokio::test]
    async fn a_refresh_without_a_refresh_token_is_refused_before_any_request() {
        let server = MockServer::start().await;
        let client = Client::new(
            token_endpoint(&format!("{}{TOKEN_PATH}", server.uri())).expect("endpoint"),
        );
        let tokens = Tokens {
            refresh: None,
            ..earlier()
        };

        let error = client.refresh(&tokens).await.expect_err("nothing to renew");

        assert!(matches!(error, Error::Malformed(_)), "{error:?}");
        assert_eq!(
            server
                .received_requests()
                .await
                .map(|requests| requests.len()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn a_rejection_carries_claudes_description_but_not_the_code() {
        let (_server, client) = answering(
            400,
            json!({
                "error": "invalid_grant",
                "error_description": format!("Invalid code {CODE} for this client"),
            }),
        )
        .await;

        let error = client
            .exchange(&code(), &verifier(), Scope::Inference)
            .await
            .expect_err("rejected");

        let Error::Rejected { status, message } = &error else {
            panic!("expected a rejection, got {error:?}");
        };
        assert_eq!(*status, 400);
        assert!(
            message.starts_with("Invalid code ") && message.ends_with(" for this client"),
            "{message}"
        );
        assert!(!error.to_string().contains(CODE), "{error}");
    }

    #[tokio::test]
    async fn an_api_style_rejection_carries_its_message_redacted() {
        let (_server, client) = answering(
            401,
            json!({
                "type": "error",
                "error": {
                    "type": "authentication_error",
                    "message": "This sign-in was revoked for sk-ant-oat01-fake0123456789abcdefghij",
                },
            }),
        )
        .await;

        let error = client.refresh(&earlier()).await.expect_err("rejected");

        assert!(
            matches!(
                &error,
                Error::Rejected { status: 401, message }
                    if message == "This sign-in was revoked for [REDACTED]"
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_unreadable_grant_is_refused_without_echoing_it() {
        let (_server, client) =
            answering(200, json!({"access_token": ACCESS, "token_type": "Bearer"})).await;

        let error = client
            .exchange(&code(), &verifier(), Scope::Inference)
            .await
            .expect_err("a grant without an expiry");

        assert!(matches!(error, Error::Malformed(_)), "{error:?}");
        assert!(!error.to_string().contains(ACCESS), "{error}");
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_is_a_transport_failure_that_names_its_cause() {
        let client =
            Client::new(token_endpoint("http://127.0.0.1:1/v1/oauth/token").expect("endpoint"));

        let error = client
            .exchange(&code(), &verifier(), Scope::Inference)
            .await
            .expect_err("nothing listens on port 1");

        let Error::Transport(message) = &error else {
            panic!("expected a transport failure, got {error:?}");
        };
        assert!(message.contains("Connection refused"), "{message}");
        assert!(!message.contains(CODE), "{message}");
    }
}
