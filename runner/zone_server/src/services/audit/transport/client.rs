use std::error::Error;
use std::io;
use std::sync::{Arc, OnceLock};
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

/// The one client every audit request shares, built on first use. A client that fails to build
/// fails the delivery instead of the process, and the next delivery tries again.
pub fn client() -> Result<Client, Failure> {
    static SHARED: OnceLock<Client> = OnceLock::new();
    if let Some(client) = SHARED.get() {
        return Ok(client.clone());
    }
    let built = builder().build().map_err(|error| {
        tracing::error!(%error, "Could not build the audit delivery HTTP client");
        Failure::of(FailureCode::Configuration)
    })?;
    Ok(SHARED.get_or_init(|| built).clone())
}

pub fn target(url: &str) -> Result<Url, Failure> {
    TargetPolicy::ExceptMetadata.validate(url).map_err(|error| {
        tracing::warn!(%error, "Refused an audit delivery target");
        match error {
            abnegate_http::Error::MetadataTarget => Failure::of(FailureCode::Target),
            _ => Failure::of(FailureCode::Configuration),
        }
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

/// The fixed failure an organization's admins see; the cause, which can name the address and
/// port Zone tried, goes only to the server log.
pub fn failure(error: &reqwest::Error) -> Failure {
    let code = classify(error);
    let host = error.url().and_then(Url::host_str).unwrap_or_default();
    tracing::warn!(%code, host, cause = %root(error), "An audit delivery request failed");
    Failure::of(code)
}

fn classify(error: &reqwest::Error) -> FailureCode {
    if cause::<abnegate_http::Error>(error).is_some() {
        FailureCode::Target
    } else if error.is_timeout() {
        FailureCode::Timeout
    } else if handshake(error) {
        FailureCode::Tls
    } else {
        FailureCode::Connect
    }
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
    fn refusals_carry_the_fixed_sentence_for_their_code() {
        for (url, code) in [
            ("http://169.254.169.254/latest", FailureCode::Target),
            ("not a url", FailureCode::Configuration),
        ] {
            assert_eq!(target(url), Err(Failure::of(code)), "{url}");
        }
    }

    #[test]
    fn every_caller_shares_one_client_without_panicking() {
        assert!(client().is_ok());
        assert!(client().is_ok());
    }

    #[tokio::test]
    async fn a_refused_connection_names_no_address_or_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = client()
            .unwrap()
            .get(format!("http://{address}/hook"))
            .send()
            .await
            .unwrap_err();
        let failure = failure(&error);
        assert_eq!(failure, Failure::of(FailureCode::Connect));
        assert!(!failure.message.contains(&address.port().to_string()));
        assert!(!failure.message.contains("127.0.0.1"));
    }

    #[test]
    fn the_user_agent_names_zone_and_its_version() {
        assert_eq!(USER_AGENT, format!("Zone/{}", env!("CARGO_PKG_VERSION")));
    }
}
