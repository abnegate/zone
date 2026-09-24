//! The Zone console a loopback sign-in returns to, taken from the `Origin` of the request that
//! started it.

use reqwest::Url;
use uuid::Uuid;

const SCHEMES: [&str; 2] = ["http", "https"];
const LOCALHOST: &str = "localhost";
const LOCALHOST_SUFFIX: &str = ".localhost";
const LOOPBACK_ADDRESSES: [&str; 2] = ["127.0.0.1", "[::1]"];
const ROOT_PATH: &str = "/";

/// Where the browser finishes a sign-in, with the receipt the callback gave it.
pub const RETURN_PATH: &str = "/agent-sign-in";
pub const RECEIPT: &str = "receipt";
pub const ORGANIZATION: &str = "organization";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Console {
    origin: String,
}

impl Console {
    /// The console at `origin`, an `http` or `https` origin whose host is a loopback name:
    /// `localhost`, a subdomain of it, `127.0.0.1` or `[::1]`.
    pub fn at(origin: &str) -> Option<Self> {
        let url = Url::parse(origin).ok()?;
        let host = url.host_str()?;
        let loopback = host == LOCALHOST
            || host
                .strip_suffix(LOCALHOST_SUFFIX)
                .is_some_and(|labels| labels.split('.').all(|label| !label.is_empty()))
            || LOOPBACK_ADDRESSES.contains(&host);
        let bare = url.username().is_empty()
            && url.password().is_none()
            && url.path() == ROOT_PATH
            && url.query().is_none()
            && url.fragment().is_none()
            && !origin.trim_end().ends_with(ROOT_PATH);
        (SCHEMES.contains(&url.scheme()) && loopback && bare).then(|| Self {
            origin: url.origin().ascii_serialization(),
        })
    }

    /// Where the callback sends the browser once it has parked the code under `receipt`.
    pub fn receipt(&self, receipt: &str, organization: Uuid) -> String {
        let mut url = Url::parse(&self.origin).expect("a console origin is a URL");
        url.set_path(RETURN_PATH);
        url.query_pairs_mut()
            .append_pair(RECEIPT, receipt)
            .append_pair(ORGANIZATION, &organization.to_string());
        url.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_console_on_this_machine_is_where_a_sign_in_returns() {
        for (origin, kept) in [
            ("http://localhost:3000", "http://localhost:3000"),
            ("http://manager.localhost", "http://manager.localhost"),
            (
                "HTTP://Manager.LocalHost:8080",
                "http://manager.localhost:8080",
            ),
            (
                "https://zone.manager.localhost",
                "https://zone.manager.localhost",
            ),
            ("http://127.0.0.1:5173", "http://127.0.0.1:5173"),
            ("http://[::1]:3000", "http://[::1]:3000"),
        ] {
            assert_eq!(
                Console::at(origin).map(|console| console.origin),
                Some(kept.to_string()),
                "{origin}"
            );
        }
    }

    #[test]
    fn a_console_anywhere_else_is_never_where_a_sign_in_returns() {
        for origin in [
            "https://zone.example.com",
            "http://localhost.attacker.example",
            "http://attacker.example/localhost",
            "http://.localhost",
            "http://manager..localhost",
            "http://10.0.0.5:3000",
            "http://127.0.0.2:3000",
            "http://0.0.0.0:3000",
            "ftp://localhost",
            "http://someone@localhost:3000",
            "http://localhost:3000/",
            "http://localhost:3000/settings",
            "http://localhost:3000?next=elsewhere",
            "null",
            "",
        ] {
            assert_eq!(Console::at(origin), None, "{origin:?}");
        }
    }

    #[test]
    fn the_browser_returns_to_the_consoles_own_page_with_the_receipt() {
        let organization =
            Uuid::parse_str("7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c").expect("a valid UUID");

        let url = Console::at("http://manager.localhost")
            .expect("a loopback console")
            .receipt("fake-receipt_1", organization);

        assert_eq!(
            url,
            "http://manager.localhost/agent-sign-in?receipt=fake-receipt_1\
             &organization=7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c"
        );
    }
}
