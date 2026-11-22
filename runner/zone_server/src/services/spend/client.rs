//! The instance LiteLLM's key management and spend log routes, under its master key.

use std::sync::LazyLock;
use std::time::Duration;

use abnegate_secret::SecretValue;
use chrono::{DateTime, Utc};
use reqwest::{RequestBuilder, Response, StatusCode, Url};
use serde_json::{Value, json};

use super::error::Error;
use super::key::Key;
use super::page::Page;
use super::subject::Subject;

/// The most rows `/spend/logs/v2` returns on a page.
pub const PAGE_SIZE: usize = 1000;
pub const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

const MANAGEMENT_TIMEOUT: Duration = Duration::from_secs(2);
const LOGS_TIMEOUT: Duration = Duration::from_secs(30);

const GENERATE_PATH: &str = "/key/generate";
const INFO_PATH: &str = "/key/info";
const DELETE_PATH: &str = "/key/delete";
const LOGS_PATH: &str = "/spend/logs/v2";

const SORT_FIELD: &str = "startTime";
const SORT_ORDER: &str = "asc";

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(MANAGEMENT_TIMEOUT)
        .build()
        .expect("the LiteLLM management client builds")
});

#[derive(Clone)]
pub struct Client {
    host: String,
    master: SecretValue,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("host", &self.host)
            .field("master", &self.master)
            .finish()
    }
}

impl Client {
    pub fn new(host: impl Into<String>, master: SecretValue) -> Self {
        Self {
            host: host.into(),
            master,
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Registers `key` for `subject`. LiteLLM updates a key it already has, so registering one
    /// again is harmless. The key is sent with no alias, which LiteLLM requires to be unique,
    /// and no user, whose LiteLLM row could narrow the models it may call.
    pub async fn register(&self, key: &Key, subject: &Subject) -> Result<(), Error> {
        let body = json!({
            "key": key.secret.expose(),
            "metadata": {
                "zone_organization": subject.organization,
                "zone_workspace": subject.workspace,
                "zone_user": subject.user,
            },
        });
        let response = self
            .send(
                HTTP.post(self.url(GENERATE_PATH)?).json(&body),
                MANAGEMENT_TIMEOUT,
            )
            .await?;
        let registered: Value = succeeded(response)?.json().await?;
        if registered.get("token").and_then(Value::as_str) != Some(key.token.as_str()) {
            return Err(Error::Token);
        }
        Ok(())
    }

    pub async fn exists(&self, token: &str) -> Result<bool, Error> {
        let mut url = self.url(INFO_PATH)?;
        url.query_pairs_mut().append_pair("key", token);
        let response = self.send(HTTP.get(url), MANAGEMENT_TIMEOUT).await?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(false),
            _ => succeeded(response).map(|_| true),
        }
    }

    /// Deletes `tokens`' keys. Keys LiteLLM no longer has are already revoked.
    pub async fn revoke(&self, tokens: &[String]) -> Result<(), Error> {
        if tokens.is_empty() {
            return Ok(());
        }
        let response = self
            .send(
                HTTP.post(self.url(DELETE_PATH)?)
                    .json(&json!({ "keys": tokens })),
                MANAGEMENT_TIMEOUT,
            )
            .await?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(()),
            _ => succeeded(response).map(|_| ()),
        }
    }

    /// Page `page`, from 1, of the requests that started from `start` through `end`, both
    /// inclusive to the second, oldest first.
    pub async fn logs(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        page: u32,
    ) -> Result<Page, Error> {
        let mut url = self.url(LOGS_PATH)?;
        url.query_pairs_mut()
            .append_pair("start_date", &start.format(DATE_FORMAT).to_string())
            .append_pair("end_date", &end.format(DATE_FORMAT).to_string())
            .append_pair("page", &page.to_string())
            .append_pair("page_size", &PAGE_SIZE.to_string())
            .append_pair("sort_by", SORT_FIELD)
            .append_pair("sort_order", SORT_ORDER);
        let response = self.send(HTTP.get(url), LOGS_TIMEOUT).await?;
        Ok(succeeded(response)?.json().await?)
    }

    fn url(&self, path: &str) -> Result<Url, Error> {
        Url::parse(&format!("{}{path}", self.host.trim_end_matches('/'))).map_err(|_| Error::Host)
    }

    async fn send(&self, request: RequestBuilder, timeout: Duration) -> Result<Response, Error> {
        Ok(request
            .bearer_auth(self.master.expose())
            .timeout(timeout)
            .send()
            .await?)
    }
}

fn succeeded(response: Response) -> Result<Response, Error> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(Error::Status(status))
}
