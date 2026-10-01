//! The hosts an instance lets organizations and workspaces save endpoints on,
//! read from `ZONE_ENDPOINT_HOSTS`.

use reqwest::Url;

const SEPARATOR: char = ',';
const SUFFIX: char = '.';
const WILDCARD: &str = "*.";

/// Every host when empty. Otherwise an entry is a host, which matches itself
/// only, or a suffix such as `.example.com` or `*.example.com`, which matches
/// `example.com` and every name under it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hosts {
    entries: Vec<String>,
}

impl Hosts {
    pub fn parse(raw: &str) -> Self {
        Self {
            entries: raw
                .split(SEPARATOR)
                .map(normalized)
                .map(|entry| match entry.strip_prefix(WILDCARD) {
                    Some(domain) => format!("{SUFFIX}{domain}"),
                    None => entry,
                })
                .filter(|entry| !entry.is_empty())
                .collect(),
        }
    }

    pub fn from_env() -> Self {
        std::env::var("ZONE_ENDPOINT_HOSTS")
            .map(|raw| Self::parse(&raw))
            .unwrap_or_default()
    }

    pub fn is_open(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn permits(&self, url: &Url) -> bool {
        if self.is_open() {
            return true;
        }
        let Some(host) = url.host_str().map(normalized) else {
            return false;
        };
        self.entries
            .iter()
            .any(|entry| match entry.strip_prefix(SUFFIX) {
                Some(domain) => host == domain || host.ends_with(entry.as_str()),
                None => host == *entry,
            })
    }
}

fn normalized(host: &str) -> String {
    let host = host.trim();
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .trim_end_matches(SUFFIX)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("a URL")
    }

    #[test]
    fn no_entries_permit_every_host() {
        for raw in ["", " , ,", "."] {
            let hosts = Hosts::parse(raw);

            assert!(hosts.is_open(), "{raw:?}");
            assert!(hosts.permits(&url("http://anything.example")), "{raw:?}");
        }
    }

    #[test]
    fn a_host_matches_only_itself() {
        let hosts = Hosts::parse(" API.openai.com. , 192.168.1.20, [fd12::1]");

        for permitted in [
            "https://api.openai.com/v1",
            "https://API.OPENAI.COM./v1",
            "http://192.168.1.20:8080",
            "http://[fd12::1]:4000",
        ] {
            assert!(hosts.permits(&url(permitted)), "{permitted}");
        }
        for refused in [
            "https://evil.api.openai.com/v1",
            "https://api.openai.com.evil.example/v1",
            "https://openai.com",
            "http://192.168.1.21",
            "http://[fd12::2]",
        ] {
            assert!(!hosts.permits(&url(refused)), "{refused}");
        }
    }

    #[test]
    fn a_suffix_matches_its_domain_and_every_name_under_it() {
        for raw in [".corp.example", "*.corp.example"] {
            let hosts = Hosts::parse(raw);

            for permitted in [
                "https://corp.example",
                "https://llm.corp.example/v1",
                "http://a.b.corp.example:4000",
            ] {
                assert!(hosts.permits(&url(permitted)), "{raw} {permitted}");
            }
            for refused in ["https://evilcorp.example", "https://corp.example.evil"] {
                assert!(!hosts.permits(&url(refused)), "{raw} {refused}");
            }
        }
    }
}
