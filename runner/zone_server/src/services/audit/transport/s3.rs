use std::fmt;

use abnegate_secret::REDACTED;
use async_trait::async_trait;
use chrono::Utc;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, Url};
use zone_enterprise::audit::destination::{Addressing, S3Secrets, S3Settings};
use zone_enterprise::audit::sigv4::{self, Credentials, DATE_HEADER, PAYLOAD_HASH_HEADER, Request};
use zone_enterprise::audit::{Event, FailureCode, Format, ndjson, object_key};

use super::{Batch, Failure, Transport, client};

pub const HOST_HEADER: &str = "host";

const SLASH: char = '/';

pub struct S3 {
    settings: S3Settings,
    secrets: S3Secrets,
    client: Client,
}

impl S3 {
    pub fn new(settings: &S3Settings, secrets: &S3Secrets, client: Client) -> Self {
        Self {
            settings: settings.clone(),
            secrets: secrets.clone(),
            client,
        }
    }

    pub fn key(&self, batch: &Batch) -> Option<String> {
        batch.events.first().map(|first| {
            object_key(
                &self.settings.prefix,
                batch.scope.organization_id(),
                first.created_at,
                first.id,
            )
        })
    }

    pub fn location(&self, key: &str) -> Result<Url, Failure> {
        let mut url = client::target(self.settings.endpoint_url().as_str())?;
        let base = url.path().trim_end_matches(SLASH).to_owned();
        let object = sigv4::encode_path(key);
        let path = match self.settings.addressing {
            Addressing::Path => format!("{base}/{}/{object}", self.settings.bucket),
            Addressing::VirtualHost => {
                let host = url.host_str().unwrap_or_default();
                let virtual_host = format!("{}.{host}", self.settings.bucket);
                url.set_host(Some(&virtual_host)).map_err(|_| {
                    Failure::new(
                        FailureCode::Configuration,
                        format!("{virtual_host} is not a valid virtual-host bucket address"),
                    )
                })?;
                format!("{base}/{object}")
            }
        };
        url.set_path(&path);
        client::target(url.as_str())
    }
}

impl fmt::Debug for S3 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3")
            .field("settings", &self.settings)
            .field("secrets", &REDACTED)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Transport for S3 {
    async fn deliver(&self, batch: &Batch) -> Result<(), Failure> {
        let Some(key) = self.key(batch) else {
            return Ok(());
        };
        let url = self.location(&key)?;
        let body = body(&batch.events)?;
        let content = Format::Ndjson.content_type();
        let at = Utc::now();
        let date = sigv4::amz_date(at);
        let payload_hash = sigv4::payload_hash(&body);
        let host = authority(&url);
        let headers = [
            (HOST_HEADER, host.as_str()),
            (PAYLOAD_HASH_HEADER, payload_hash.as_str()),
            (DATE_HEADER, date.as_str()),
            (CONTENT_TYPE.as_str(), content),
        ];
        let request = Request {
            method: Method::PUT.as_str(),
            path: url.path(),
            query: &[],
            headers: &headers,
            payload_hash: &payload_hash,
        };
        let credentials = Credentials {
            access_key_id: &self.secrets.access_key_id,
            secret_access_key: &self.secrets.secret_access_key,
        };
        let authorization = sigv4::authorization(
            &request,
            &credentials,
            &self.settings.region,
            sigv4::SERVICE,
            at,
        );
        let response = self
            .client
            .put(url.clone())
            .header(PAYLOAD_HASH_HEADER, &payload_hash)
            .header(DATE_HEADER, &date)
            .header(CONTENT_TYPE, content)
            .header(AUTHORIZATION, authorization)
            .body(body)
            .send()
            .await
            .map_err(|error| client::failure(&error))?;
        client::accepted(&response)
    }
}

pub fn authority(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

fn body(events: &[Event]) -> Result<Vec<u8>, Failure> {
    let mut body = Vec::new();
    for event in events {
        let line = ndjson::line(event)
            .map_err(|error| Failure::new(FailureCode::Configuration, error.to_string()))?;
        body.extend(line);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(endpoint: &str, addressing: Addressing) -> S3Settings {
        S3Settings {
            bucket: "audit-bucket".to_owned(),
            region: "eu-west-1".to_owned(),
            endpoint: Some(endpoint.to_owned()),
            prefix: "zone/".to_owned(),
            addressing,
        }
        .validated()
        .unwrap()
    }

    fn secrets() -> S3Secrets {
        S3Secrets {
            access_key_id: "AKIDEXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMIEXAMPLEKEY".to_owned(),
        }
    }

    fn transport(endpoint: &str, addressing: Addressing) -> S3 {
        S3::new(
            &settings(endpoint, addressing),
            &secrets(),
            client::client(),
        )
    }

    #[test]
    fn path_style_puts_the_bucket_first_in_the_path() {
        let url = transport("http://minio:9000", Addressing::Path)
            .location("zone/server/2026/11/17/093005Z-id.ndjson")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "http://minio:9000/audit-bucket/zone/server/2026/11/17/093005Z-id.ndjson"
        );
        assert_eq!(authority(&url), "minio:9000");
    }

    #[test]
    fn virtual_host_puts_the_bucket_in_the_host() {
        let url = transport("https://s3.example.com", Addressing::VirtualHost)
            .location("zone/server/key.ndjson")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://audit-bucket.s3.example.com/zone/server/key.ndjson"
        );
        assert_eq!(authority(&url), "audit-bucket.s3.example.com");
    }

    #[test]
    fn an_endpoint_path_is_kept_before_the_object() {
        let url = transport("http://gateway:8080/storage/", Addressing::Path)
            .location("key.ndjson")
            .unwrap();
        assert_eq!(url.path(), "/storage/audit-bucket/key.ndjson");
    }

    #[test]
    fn the_default_endpoint_is_the_regional_amazon_endpoint() {
        let mut settings = settings("https://unused.example", Addressing::VirtualHost);
        settings.endpoint = None;
        let url = S3::new(&settings, &secrets(), client::client())
            .location("key.ndjson")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://audit-bucket.s3.eu-west-1.amazonaws.com/key.ndjson"
        );
    }

    #[test]
    fn a_metadata_endpoint_is_a_target_failure() {
        let failure = transport("http://169.254.169.254", Addressing::Path)
            .location("key.ndjson")
            .unwrap_err();
        assert_eq!(failure.code, FailureCode::Target);
    }

    #[test]
    fn the_debug_output_redacts_the_secrets() {
        let debug = format!("{:?}", transport("http://minio:9000", Addressing::Path));
        assert!(!debug.contains("AKIDEXAMPLE"));
        assert!(!debug.contains("wJalrXUtnFEMIEXAMPLEKEY"));
        assert!(debug.contains(REDACTED));
    }
}
