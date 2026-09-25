//! The Zone console a loopback sign-in returns to: the `Origin` of the request that started it,
//! when the operator listed that console.

use reqwest::Url;
use uuid::Uuid;

use super::claude::ROOT_PATH;

const SCHEMES: [&str; 2] = ["http", "https"];
const LOCALHOST: &str = "localhost";
const LOCALHOST_SUFFIX: &str = ".localhost";
const LOOPBACK_ADDRESSES: [&str; 2] = ["127.0.0.1", "[::1]"];

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
    /// `localhost`, a subdomain of it, `127.0.0.1` or `[::1]`, when `consoles` lists exactly that
    /// origin.
    pub fn at(origin: &str, consoles: &[String]) -> Option<Self> {
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
        let normalised = url.origin().ascii_serialization();
        (SCHEMES.contains(&url.scheme()) && loopback && bare && consoles.contains(&normalised))
            .then_some(Self { origin: normalised })
    }

    /// Where the callback sends the browser once it has parked the code under `receipt`, which
    /// travels in the fragment: a browser sends that to no server, and in no `Referer`.
    pub fn receipt(&self, receipt: &str, organization: Uuid) -> String {
        let mut url = Url::parse(&self.origin).expect("a console origin is a URL");
        url.set_path(RETURN_PATH);
        url.set_fragment(Some(&format!(
            "{RECEIPT}={}&{ORGANIZATION}={organization}",
            urlencoding::encode(receipt)
        )));
        url.into()
    }
}

/// What the console reads as `name` from `url`, an address the callback sent a browser to: only
/// its fragment, so a query, which servers log, fails the test.
#[cfg(test)]
pub(crate) fn handed(url: &str, name: &str) -> String {
    let url = Url::parse(url).expect("an absolute URL");
    assert_eq!(
        url.query(),
        None,
        "{url} carries a query a server would log"
    );
    url.fragment()
        .into_iter()
        .flat_map(|fragment| fragment.split('&'))
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        .map(|value| {
            urlencoding::decode(value)
                .expect("a UTF-8 value")
                .into_owned()
        })
        .unwrap_or_else(|| panic!("{url} hands the console no {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(origins: &[&str]) -> Vec<String> {
        origins.iter().map(|origin| origin.to_string()).collect()
    }

    #[test]
    fn a_listed_console_on_this_machine_is_where_a_sign_in_returns() {
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
                Console::at(origin, &listed(&[kept])).map(|console| console.origin),
                Some(kept.to_string()),
                "{origin}"
            );
        }
    }

    #[test]
    fn a_console_nobody_listed_is_never_where_a_sign_in_returns() {
        let consoles = listed(&["http://manager.localhost", "http://localhost:3001"]);

        for origin in [
            "http://localhost:9999",
            "http://localhost:3000",
            "http://evil.localhost",
            "http://manager.localhost:8080",
            "https://manager.localhost",
            "http://127.0.0.1:53123",
            "http://127.0.0.1:3001",
            "http://[::1]:3001",
        ] {
            assert_eq!(Console::at(origin, &consoles), None, "{origin}");
        }
        assert_eq!(
            Console::at("http://manager.localhost", &[]),
            None,
            "a server that lists no console sent a sign-in's browser to one"
        );
    }

    #[test]
    fn a_console_anywhere_else_is_never_where_a_sign_in_returns_even_when_listed() {
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
            let consoles = Url::parse(origin)
                .map(|url| url.origin().ascii_serialization())
                .into_iter()
                .collect::<Vec<_>>();

            assert_eq!(Console::at(origin, &consoles), None, "{origin:?}");
        }
    }

    #[test]
    fn the_browser_returns_to_the_consoles_own_page_with_the_receipt_in_its_fragment() {
        let organization =
            Uuid::parse_str("7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c").expect("a valid UUID");

        let url = Console::at(
            "http://manager.localhost",
            &listed(&["http://manager.localhost"]),
        )
        .expect("a listed loopback console")
        .receipt("fake-receipt_1", organization);

        assert_eq!(
            url,
            "http://manager.localhost/agent-sign-in#receipt=fake-receipt_1\
             &organization=7b0e7c9a-2f7a-4a55-9d0e-1c7d8f6a5b4c",
            "a receipt in the query reaches every access log between the browser and the console"
        );
        assert_eq!(handed(&url, RECEIPT), "fake-receipt_1");
        assert_eq!(handed(&url, ORGANIZATION), organization.to_string());
    }
}
