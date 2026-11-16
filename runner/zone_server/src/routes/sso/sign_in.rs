pub const CALLBACK_PATH: &str = "/sso/callback";

/// Where identity providers send people back: the console's callback page, which hands the code
/// to the server.
pub fn redirect_uri(app_base_url: &str) -> String {
    format!("{}{CALLBACK_PATH}", app_base_url.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_uri_is_the_console_callback_under_the_app_base_url() {
        for base in ["https://zone.example.com", "https://zone.example.com/"] {
            assert_eq!(
                redirect_uri(base),
                "https://zone.example.com/sso/callback",
                "{base}"
            );
        }
        assert_eq!(
            redirect_uri("https://example.com/zone"),
            "https://example.com/zone/sso/callback"
        );
    }
}
