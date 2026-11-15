use reqwest::Url;

const WEB_SCHEMES: [&str; 2] = ["https", "http"];

pub fn absolute(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    (WEB_SCHEMES.contains(&parsed.scheme()) && parsed.has_host()).then(|| url.to_string())
}

pub fn linked(url: &str) -> bool {
    absolute(url).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_addresses_with_a_host_are_links() {
        for url in [
            "https://github.com/owner/repository/pull/7",
            "http://github.example.com/owner/repository",
        ] {
            assert_eq!(absolute(url).as_deref(), Some(url));
            assert!(linked(url));
        }
        for url in [
            "",
            "/owner/repository/pull/7",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://",
        ] {
            assert_eq!(absolute(url), None, "{url}");
            assert!(!linked(url), "{url}");
        }
    }
}
