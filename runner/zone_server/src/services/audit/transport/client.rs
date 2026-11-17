use std::error::Error;
use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use abnegate_http::metadata::MetadataResolver;
use abnegate_http::{RetryAfter, TargetPolicy};
use reqwest::header::RETRY_AFTER;
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder, Response, StatusCode, Url};
use zone_enterprise::audit::FailureCode;

use super::Failure;

pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const USER_AGENT: &str = concat!("Zone/", env!("CARGO_PKG_VERSION"));
const THROTTLED: [StatusCode; 2] = [
    StatusCode::TOO_MANY_REQUESTS,
    StatusCode::SERVICE_UNAVAILABLE,
];

pub fn builder() -> ClientBuilder {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .dns_resolver(Arc::new(MetadataResolver::new()))
        .user_agent(USER_AGENT)
        .timeout(TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
}

pub fn client() -> Client {
    builder().build().expect("the audit delivery client builds")
}

pub fn target(url: &str) -> Result<Url, Failure> {
    TargetPolicy::ExceptMetadata
        .validate(url)
        .map_err(|error| match error {
            abnegate_http::Error::MetadataTarget => {
                Failure::new(FailureCode::Target, error.to_string())
            }
            _ => Failure::new(FailureCode::Configuration, error.to_string()),
        })
}

pub fn accepted(response: &Response) -> Result<(), Failure> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let mut failure = Failure::new(FailureCode::Status, format!("HTTP {}", status.as_u16()));
    if THROTTLED.contains(&status) {
        failure.retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| RetryAfter::from_header(value, SystemTime::now()));
    }
    Err(failure)
}

pub fn failure(error: &reqwest::Error) -> Failure {
    if let Some(refused) = cause::<abnegate_http::Error>(error) {
        return Failure::new(FailureCode::Target, refused.to_string());
    }
    if error.is_timeout() {
        return Failure::new(
            FailureCode::Timeout,
            "The destination did not respond in time",
        );
    }
    let root = root(error);
    if handshake(error) {
        return Failure::new(FailureCode::Tls, format!("TLS handshake failed: {root}"));
    }
    Failure::new(
        FailureCode::Connect,
        format!("Could not reach the destination: {root}"),
    )
}

fn handshake(error: &reqwest::Error) -> bool {
    causes(error)
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|cause| cause.kind() == io::ErrorKind::InvalidData)
}

fn cause<T: Error + 'static>(error: &reqwest::Error) -> Option<&T> {
    causes(error).find_map(|cause| cause.downcast_ref::<T>())
}

fn root(error: &reqwest::Error) -> String {
    causes(error)
        .last()
        .map_or_else(|| error.to_string(), ToString::to_string)
}

fn causes(error: &reqwest::Error) -> impl Iterator<Item = &(dyn Error + 'static)> {
    std::iter::successors(error.source(), |current| inner(*current))
}

fn inner<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a (dyn Error + 'static)> {
    match error.downcast_ref::<io::Error>() {
        Some(wrapper) => wrapper
            .get_ref()
            .map(|wrapped| wrapped as &(dyn Error + 'static)),
        None => error.source(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metadata_url_is_a_target_failure() {
        for refused in [
            "http://169.254.169.254/latest",
            "http://metadata.google.internal/",
            "http://[fd00:ec2::254]/",
        ] {
            assert_eq!(
                target(refused).map_err(|failure| failure.code),
                Err(FailureCode::Target),
                "{refused}"
            );
        }
    }

    #[test]
    fn loopback_and_lan_urls_are_allowed() {
        for allowed in [
            "http://127.0.0.1:8080/hook",
            "http://localhost/hook",
            "https://10.0.0.5/hook",
            "http://receiver/hook",
        ] {
            assert!(target(allowed).is_ok(), "{allowed}");
        }
    }

    #[test]
    fn a_malformed_url_is_a_configuration_failure() {
        assert_eq!(
            target("not a url").map_err(|failure| failure.code),
            Err(FailureCode::Configuration)
        );
    }

    #[test]
    fn the_user_agent_names_zone_and_its_version() {
        assert_eq!(USER_AGENT, format!("Zone/{}", env!("CARGO_PKG_VERSION")));
    }
}
