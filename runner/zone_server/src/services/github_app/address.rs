use reqwest::Url;

const SCHEMES: [&str; 2] = ["http", "https"];

/// The address parsed when it is an absolute http(s) URL. Any host is
/// accepted, private and loopback ones included, since a GitHub Enterprise
/// server may live on either.
pub fn addressable(address: &str) -> Option<Url> {
    Url::parse(address)
        .ok()
        .filter(|url| SCHEMES.contains(&url.scheme()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_web_address_on_any_host_is_addressable() {
        for address in [
            "https://github.com/apps/zone-acme",
            "http://192.168.1.5:8080/apps/zone",
            "http://10.0.0.5:8080/acme/widgets",
            "http://127.0.0.1:8080/settings/installations/1",
        ] {
            assert_eq!(
                addressable(address).map(String::from).as_deref(),
                Some(address),
                "{address}"
            );
        }
    }

    #[test]
    fn an_address_that_is_not_absolute_http_is_not_addressable() {
        for address in [
            "",
            "acme/widgets",
            "/apps/zone-acme",
            "ftp://github.com/apps/x",
            "javascript:alert(1)",
        ] {
            assert_eq!(addressable(address), None, "{address}");
        }
    }
}
