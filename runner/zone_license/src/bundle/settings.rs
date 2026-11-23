use std::fmt;

use reqwest::Url;
use rusty_s3::{Bucket, Credentials};

pub const DEFAULT_PREFIX: &str = "bundles";

const REDACTED: &str = "[redacted]";

#[derive(Clone)]
pub struct BundleSettings {
    pub bucket: Bucket,
    pub prefix: String,
    pub credentials: Credentials,
}

impl BundleSettings {
    pub fn default_endpoint(region: &str) -> String {
        format!("https://s3.{region}.amazonaws.com")
    }

    pub fn is_endpoint(text: &str) -> bool {
        text.parse::<Url>()
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    }

    pub fn is_region(text: &str) -> bool {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }

    pub fn parse_prefix(text: &str) -> Option<String> {
        let prefix = text.trim_matches('/');
        prefix.split('/').all(is_segment).then(|| prefix.to_owned())
    }
}

fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl fmt::Debug for BundleSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BundleSettings")
            .field("bucket", &self.bucket.name())
            .field("region", &self.bucket.region())
            .field("url", &self.bucket.base_url().as_str())
            .field("prefix", &self.prefix)
            .field("access_key_id", &self.credentials.key())
            .field("secret_access_key", &REDACTED)
            .finish()
    }
}
