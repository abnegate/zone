use reqwest::Url;

pub const CALLBACK_PATH: &str = "/sso/callback";

/// Where identity providers send people back: the console's callback page, which hands the code
/// to the server.
pub fn redirect_uri(app_base_url: &str) -> String {
    format!("{}{CALLBACK_PATH}", callback_origin(app_base_url))
}

/// The scheme, host and port of the console. A sign-in has to start on this origin, because the
/// callback page checks the state the browser kept when it began.
pub fn callback_origin(app_base_url: &str) -> String {
    Url::parse(app_base_url)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|_| app_base_url.trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_uri_is_the_console_callback_on_the_app_base_url_origin() {
        for base in ["https://zone.example.com", "https://zone.example.com/"] {
            assert_eq!(
                redirect_uri(base),
                "https://zone.example.com/sso/callback",
                "{base}"
            );
        }
        assert_eq!(
            redirect_uri("https://example.com/zone"),
            "https://example.com/sso/callback"
        );
    }

    #[test]
    fn the_redirect_uri_is_the_callback_path_on_the_callback_origin() {
        for base in [
            "https://zone.example.com",
            "https://zone.example.com/",
            "https://example.com/zone",
            "https://zone.example.com:8443/app/",
            "http://localhost:3000",
        ] {
            assert_eq!(
                redirect_uri(base),
                format!("{}{CALLBACK_PATH}", callback_origin(base)),
                "{base}"
            );
        }
    }

    #[test]
    fn the_callback_origin_is_the_scheme_host_and_port_of_the_console() {
        let cases = [
            ("https://zone.example.com", "https://zone.example.com"),
            ("https://zone.example.com/", "https://zone.example.com"),
            ("https://example.com/zone", "https://example.com"),
            (
                "https://zone.example.com:8443/app/",
                "https://zone.example.com:8443",
            ),
            ("https://zone.example.com:443", "https://zone.example.com"),
            ("http://localhost:3000", "http://localhost:3000"),
        ];
        for (base, origin) in cases {
            assert_eq!(callback_origin(base), origin, "{base}");
        }
    }
}
