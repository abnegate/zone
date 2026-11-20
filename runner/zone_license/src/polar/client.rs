use std::time::Duration;

use reqwest::header::{ACCEPT, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Response, StatusCode};
use uuid::Uuid;

use super::access_token::AccessToken;
use super::api_error::ApiError;
use super::order_subscription::OrderSubscription;
use super::proxy::Proxy;

pub const API: &str = "https://api.polar.sh";
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const BODY_LIMIT: usize = 1024 * 1024;

const SUBSCRIPTIONS_PATH: &str = "v1/subscriptions";
const USER_AGENT: &str = concat!("zone-license/", env!("CARGO_PKG_VERSION"));
const JSON: HeaderValue = HeaderValue::from_static("application/json");

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    token: AccessToken,
    base: String,
}

impl Client {
    pub fn new(token: AccessToken, base: &str, proxy: Proxy) -> Result<Self, reqwest::Error> {
        Self::build(token, base, proxy, TIMEOUT)
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(
        token: AccessToken,
        base: &str,
        proxy: Proxy,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        Self::build(token, base, proxy, timeout)
    }

    fn build(
        token: AccessToken,
        base: &str,
        proxy: Proxy,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .connect_timeout(CONNECT_TIMEOUT.min(timeout));
        let http = match proxy {
            Proxy::System => builder,
            Proxy::Direct => builder.no_proxy(),
        }
        .build()?;
        let base = http
            .get(base)
            .build()?
            .url()
            .as_str()
            .trim_end_matches('/')
            .to_owned();
        Ok(Self { http, token, base })
    }

    pub async fn subscription(&self, id: Uuid) -> Result<Option<OrderSubscription>, ApiError> {
        let response = self
            .http
            .get(format!("{}/{SUBSCRIPTIONS_PATH}/{id}", self.base))
            .bearer_auth(self.token.expose())
            .header(ACCEPT, JSON)
            .send()
            .await
            .map_err(unavailable)?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status.is_success() {
            let body = body(response).await?;
            return serde_json::from_slice(&body).map(Some).map_err(malformed);
        }
        if is_unavailable(status) {
            return Err(ApiError::Unavailable);
        }
        Err(ApiError::Refused {
            status: status.as_u16(),
        })
    }
}

fn is_unavailable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

async fn body(mut response: Response) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Err(oversize());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(unavailable)? {
        if body.len() + chunk.len() > BODY_LIMIT {
            return Err(oversize());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn oversize() -> ApiError {
    tracing::warn!(
        limit = BODY_LIMIT,
        "Polar answered with a body over the limit"
    );
    ApiError::Malformed
}

fn malformed(error: serde_json::Error) -> ApiError {
    tracing::warn!(
        category = ?error.classify(),
        line = error.line(),
        column = error.column(),
        "Polar answered with a subscription that could not be parsed"
    );
    ApiError::Malformed
}

fn unavailable(error: reqwest::Error) -> ApiError {
    tracing::warn!(cause = %error.without_url(), "The Polar API could not be reached");
    ApiError::Unavailable
}

#[cfg(test)]
mod tests;
