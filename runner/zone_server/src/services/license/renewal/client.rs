mod renewed;
mod request;

use std::sync::Arc;
use std::time::Duration;

use abnegate_http::metadata::MetadataResolver;
use reqwest::redirect::Policy;
use reqwest::{Response, StatusCode, Url};

use super::{Answer, Proxy, Settings};
use renewed::Renewed;
use request::Request;

pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const BODY_LIMIT: usize = 64 * 1024;

const USER_AGENT: &str = "Zone";

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    url: Url,
}

impl Client {
    pub fn new(settings: &Settings, proxy: Proxy) -> Result<Self, reqwest::Error> {
        Self::with_timeouts(settings, proxy, TIMEOUT, CONNECT_TIMEOUT)
    }

    fn with_timeouts(
        settings: &Settings,
        proxy: Proxy,
        timeout: Duration,
        connect: Duration,
    ) -> Result<Self, reqwest::Error> {
        let builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .dns_resolver(Arc::new(MetadataResolver::new()))
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .connect_timeout(connect);
        let builder = match proxy {
            Proxy::System => builder,
            Proxy::Direct => builder.no_proxy(),
        };
        Ok(Self {
            http: builder.build()?,
            url: settings.url.clone(),
        })
    }

    pub async fn renew(&self, key: &str) -> Answer {
        let response = match self
            .http
            .post(self.url.clone())
            .json(&Request { license: key })
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(cause = %error.without_url(), "Could not reach the license server");
                return Answer::Unreachable;
            }
        };
        let status = response.status();
        match status {
            StatusCode::OK => read(response).await,
            StatusCode::NO_CONTENT => Answer::Current,
            StatusCode::GONE => Answer::Ended,
            StatusCode::NOT_FOUND | StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => {
                unreachable(status)
            }
            _ if status.is_client_error() => {
                tracing::warn!(
                    status = status.as_u16(),
                    "The license server refused the renewal request"
                );
                Answer::Refused
            }
            _ => unreachable(status),
        }
    }
}

async fn read(response: Response) -> Answer {
    let body = match abnegate_http::read_capped(response, BODY_LIMIT).await {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, "Could not read the license server's answer");
            return Answer::Unreachable;
        }
    };
    match serde_json::from_slice::<Renewed>(&body) {
        Ok(renewed) => Answer::Renewed(renewed.license),
        Err(error) => {
            tracing::warn!(%error, "The license server's answer is not a renewed key");
            Answer::Malformed
        }
    }
}

fn unreachable(status: StatusCode) -> Answer {
    tracing::warn!(
        status = status.as_u16(),
        "The license server could not answer the renewal request"
    );
    Answer::Unreachable
}

#[cfg(test)]
mod tests;
