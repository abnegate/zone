use std::fmt;
use std::time::Duration;

use reqwest::header::{HeaderValue, RANGE};
use reqwest::redirect::Policy;
use reqwest::{StatusCode, Url};
use rusty_s3::{Bucket, Credentials, S3Action};

use super::object::Object;
use super::presence::Presence;
use super::settings::BundleSettings;
use super::storage_error::StorageError;
use crate::polar::Proxy;

pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const BODY_LIMIT: usize = 8 * 1024;
pub const DOWNLOAD_LIFETIME: Duration = Duration::from_secs(900);
pub const PROBE_LIFETIME: Duration = Duration::from_secs(60);
pub const NO_SUCH_KEY: &str = "NoSuchKey";
pub const ACCESS_DENIED: &str = "AccessDenied";

const USER_AGENT: &str = concat!("zone-license/", env!("CARGO_PKG_VERSION"));
const FIRST_BYTE: HeaderValue = HeaderValue::from_static("bytes=0-0");
const CODE_OPEN: &str = "<Code>";
const CODE_CLOSE: &str = "</Code>";

#[derive(Clone)]
pub struct Presigner {
    http: reqwest::Client,
    bucket: Bucket,
    prefix: String,
    credentials: Credentials,
}

impl Presigner {
    pub fn new(settings: BundleSettings, proxy: Proxy) -> Result<Self, reqwest::Error> {
        Self::build(settings, proxy, TIMEOUT)
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(
        settings: BundleSettings,
        proxy: Proxy,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        Self::build(settings, proxy, timeout)
    }

    fn build(
        settings: BundleSettings,
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
        Ok(Self {
            http,
            bucket: settings.bucket,
            prefix: settings.prefix,
            credentials: settings.credentials,
        })
    }

    pub fn presign(&self, object: &Object, lifetime: Duration) -> Url {
        let key = object.key(&self.prefix);
        self.bucket
            .get_object(Some(&self.credentials), &key)
            .sign(lifetime)
    }

    pub async fn probe(&self, object: &Object) -> Result<Presence, StorageError> {
        let response = self
            .http
            .get(self.presign(object, PROBE_LIFETIME))
            .header(RANGE, FIRST_BYTE)
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        if status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT {
            return Ok(Presence::Present);
        }
        let body = excerpt(response).await.map_err(unreachable)?;
        match alphanumeric_code(&body) {
            Some(code) if code == NO_SUCH_KEY || code == ACCESS_DENIED => {
                Ok(Presence::Absent { code: Some(code) })
            }
            Some(code) => Err(StorageError::Refused { code }),
            None if status == StatusCode::NOT_FOUND => Ok(Presence::Absent { code: None }),
            None => Err(StorageError::Unexpected {
                status: status.as_u16(),
            }),
        }
    }
}

async fn excerpt(mut response: reqwest::Response) -> Result<Vec<u8>, reqwest::Error> {
    let mut body = Vec::new();
    while body.len() < BODY_LIMIT {
        let Some(chunk) = response.chunk().await? else {
            break;
        };
        let room = BODY_LIMIT - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    Ok(body)
}

fn alphanumeric_code(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let start = text.find(CODE_OPEN)? + CODE_OPEN.len();
    let length = text[start..].find(CODE_CLOSE)?;
    let code = text[start..start + length].trim();
    (!code.is_empty() && code.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| code.to_owned())
}

fn unreachable(error: reqwest::Error) -> StorageError {
    tracing::warn!(cause = %error.without_url(), "The bundle storage could not be reached");
    StorageError::Unreachable
}

impl fmt::Debug for Presigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Presigner")
            .field("bucket", &self.bucket.name())
            .field("url", &self.bucket.base_url().as_str())
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}
