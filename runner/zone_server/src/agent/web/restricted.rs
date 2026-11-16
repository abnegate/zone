//! A fetch held to an agent policy's domains.
//!
//! The public client follows redirects itself, past any check a caller could
//! add, so a fetch a policy restricts runs on a client of its own: the same
//! public-address checks on the URL, on every name it resolves and on every
//! redirect hop, and the policy's domains on every host before anything is
//! sent to it.

use std::error::Error;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use abnegate_http::validate_public_url;
use reqwest::Client;
use reqwest::Proxy;
use reqwest::Url;
use reqwest::dns::Addrs;
use reqwest::dns::Name;
use reqwest::dns::Resolve;
use reqwest::dns::Resolving;
use reqwest::redirect::Policy;
use zone_enterprise::policy::Effective;

/// As many as the public client follows.
const MAXIMUM_REDIRECTS: usize = 3;

#[derive(Debug, Clone, thiserror::Error)]
#[error("Requests to {host} are outside the workspace's allowed domains.")]
pub struct Refused {
    pub host: String,
}

pub fn admit(policy: &Effective, url: &Url) -> Result<(), Refused> {
    let host = url.host_str().unwrap_or_default();
    policy.domain(host).map_err(|_| Refused {
        host: host.to_string(),
    })
}

pub fn client(
    policy: Arc<Effective>,
    proxy: Option<Proxy>,
    timeout: Duration,
    agent: &str,
) -> reqwest::Result<Client> {
    let builder = Client::builder()
        .timeout(timeout)
        .no_proxy()
        .user_agent(agent)
        .dns_resolver(Arc::new(Public))
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() > MAXIMUM_REDIRECTS {
                return attempt.error(abnegate_http::Error::TooManyRedirects);
            }
            let url = match validate_public_url(attempt.url().as_str()) {
                Ok(url) => url,
                Err(error) => return attempt.error(error),
            };
            match admit(&policy, &url) {
                Ok(()) => attempt.follow(),
                Err(refused) => attempt.error(refused),
            }
        }));
    match proxy {
        Some(proxy) => builder.proxy(proxy),
        None => builder,
    }
    .build()
}

pub fn refused(error: &reqwest::Error) -> Option<&Refused> {
    let mut source: Option<&(dyn Error + 'static)> = error.source();
    while let Some(cause) = source {
        if let Some(refused) = cause.downcast_ref::<Refused>() {
            return Some(refused);
        }
        source = cause.source();
    }
    None
}

/// Resolves a name to the addresses a public fetch may connect to, refusing
/// a name that answers with none: a public name can point inside the
/// deployment, which no check of the URL's text sees.
struct Public;

impl Resolve for Public {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let public: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|address| fetchable(address.ip()))
                .collect();
            if public.is_empty() {
                return Err(abnegate_http::Error::unfetchable_resolution(host).into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

fn fetchable(address: IpAddr) -> bool {
    validate_public_url(&format!("http://{}/", SocketAddr::new(address, 0))).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_public_address_is_fetchable() {
        for (address, expected) in [
            ("93.184.215.14", true),
            ("2606:4700::6810:84e5", true),
            ("127.0.0.1", false),
            ("10.0.0.5", false),
            ("169.254.169.254", false),
            ("::1", false),
            ("fd00::1", false),
        ] {
            let parsed: IpAddr = address.parse().expect("an address");
            assert_eq!(fetchable(parsed), expected, "{address}");
        }
    }

    #[tokio::test]
    async fn a_name_answering_only_inside_the_deployment_is_refused() {
        let resolved = Public.resolve("localhost".parse().expect("a name")).await;
        assert!(resolved.is_err());
    }
}
