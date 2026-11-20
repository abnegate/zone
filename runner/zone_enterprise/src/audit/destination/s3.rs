use std::fmt;
use std::net::Ipv4Addr;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::audit::destination::{REDACTED, url};
use crate::audit::error::Error;

const BUCKET_LENGTH: RangeInclusive<usize> = 3..=63;
const BUCKET_RESERVED_PREFIXES: [&str; 3] = ["xn--", "sthree-", "amzn-s3-demo-"];
const BUCKET_RESERVED_SUFFIXES: [&str; 5] =
    ["-s3alias", "--ol-s3", ".mrap", "--x-s3", "--table-s3"];
const DOT: char = '.';
const HYPHEN: char = '-';
const SLASH: char = '/';
const CURRENT_SEGMENT: &str = ".";
const PARENT_SEGMENT: &str = "..";
const ADJACENT_DOTS: &str = "..";
const OBJECT_KEY_MAXIMUM: usize = 1024;
const LONGEST_OBJECT_NAME: &str = "00000000-0000-0000-0000-000000000000/0000/00/00/000000Z-00000000-0000-0000-0000-000000000000.ndjson";
const PREFIX_MAXIMUM: usize = OBJECT_KEY_MAXIMUM - LONGEST_OBJECT_NAME.len();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Addressing {
    Path,
    #[default]
    VirtualHost,
}

impl Addressing {
    pub const ALL: [Self; 2] = [Self::Path, Self::VirtualHost];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::VirtualHost => "virtual_host",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Settings {
    pub bucket: String,
    pub region: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub addressing: Addressing,
}

impl S3Settings {
    pub fn validated(self) -> Result<Self, Error> {
        let bucket = self.bucket.trim();
        if !valid_bucket(bucket) {
            return Err(Error::BucketInvalid(bucket.to_owned()));
        }
        let region = self.region.trim();
        if !valid_region(region) {
            return Err(Error::RegionInvalid(region.to_owned()));
        }
        let endpoint = match self.endpoint.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(endpoint) => Some(url(endpoint)?),
        };
        let validated = Self {
            bucket: bucket.to_owned(),
            region: region.to_owned(),
            endpoint,
            prefix: prefix(&self.prefix)?,
            addressing: self.addressing,
        };
        validated.request_base()?;
        Ok(validated)
    }

    pub fn endpoint_url(&self) -> Url {
        match &self.endpoint {
            Some(endpoint) => Url::parse(endpoint),
            None => Url::parse(&format!("https://s3.{}.amazonaws.com", self.region)),
        }
        .expect("validated S3 settings name a parseable endpoint")
    }

    /// Where requests go, with the bucket in the host or the path: an object's URL is this with
    /// its key appended. Its host is the one a request reaches, so endpoint rules apply to it.
    pub fn request_base(&self) -> Result<Url, Error> {
        let mut url = self.endpoint_url();
        let base = url.path().trim_end_matches(SLASH).to_owned();
        match self.addressing {
            Addressing::Path => url.set_path(&format!("{base}/{}", self.bucket)),
            Addressing::VirtualHost => {
                let Some(Host::Domain(host)) = url.host() else {
                    return Err(Error::VirtualHostUnaddressable);
                };
                let virtual_host = format!("{}.{host}", self.bucket);
                url.set_host(Some(&virtual_host))
                    .map_err(|_| Error::VirtualHostUnaddressable)?;
                url.set_path(&base);
            }
        }
        Ok(url)
    }
}

fn valid_bucket(bucket: &str) -> bool {
    BUCKET_LENGTH.contains(&bucket.len())
        && bucket.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == DOT
                || character == HYPHEN
        })
        && bucket.starts_with(|character: char| character.is_ascii_alphanumeric())
        && bucket.ends_with(|character: char| character.is_ascii_alphanumeric())
        && !bucket.contains(ADJACENT_DOTS)
        && bucket.parse::<Ipv4Addr>().is_err()
        && !BUCKET_RESERVED_PREFIXES
            .iter()
            .any(|reserved| bucket.starts_with(reserved))
        && !BUCKET_RESERVED_SUFFIXES
            .iter()
            .any(|reserved| bucket.ends_with(reserved))
}

fn valid_region(region: &str) -> bool {
    !region.is_empty()
        && region.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == HYPHEN
        })
}

fn prefix(raw: &str) -> Result<String, Error> {
    let stripped = raw.trim().trim_start_matches(SLASH);
    if stripped.is_empty() {
        return Ok(String::new());
    }
    let body = stripped.strip_suffix(SLASH).unwrap_or(stripped);
    let valid = body.len() < PREFIX_MAXIMUM
        && !body.chars().any(char::is_control)
        && body.split(SLASH).all(|segment| {
            !segment.is_empty() && segment != CURRENT_SEGMENT && segment != PARENT_SEGMENT
        });
    if !valid {
        return Err(Error::PrefixInvalid(raw.trim().to_owned()));
    }
    Ok(format!("{body}{SLASH}"))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Secrets {
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl fmt::Debug for S3Secrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Secrets")
            .field("access_key_id", &REDACTED)
            .field("secret_access_key", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests;
