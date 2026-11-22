use reqwest::Url;
use zone_enterprise::sso::saml::ServiceProvider;

pub const CALLBACK_PATH: &str = "/sso/callback";
pub const SAML_METADATA_PATH: &str = "/api/auth/sso/saml/metadata";
pub const SAML_ACS_PATH: &str = "/api/auth/sso/saml/acs";

pub fn redirect_uri(app_base_url: &str) -> String {
    format!("{}{CALLBACK_PATH}", callback_origin(app_base_url))
}

/// The service provider's entity ID, which is also where its metadata is served.
pub fn saml_entity_id(app_base_url: &str) -> String {
    format!("{}{SAML_METADATA_PATH}", callback_origin(app_base_url))
}

pub fn saml_acs_url(app_base_url: &str) -> String {
    format!("{}{SAML_ACS_PATH}", callback_origin(app_base_url))
}

pub fn service_provider(app_base_url: &str) -> ServiceProvider {
    ServiceProvider {
        entity_id: saml_entity_id(app_base_url),
        acs_url: saml_acs_url(app_base_url),
    }
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

    #[test]
    fn the_saml_urls_are_their_paths_on_the_callback_origin() {
        for base in [
            "https://zone.example.com",
            "https://zone.example.com/",
            "https://example.com/zone",
            "https://zone.example.com:8443/app/",
            "http://localhost:3000",
        ] {
            let origin = callback_origin(base);
            assert_eq!(
                saml_entity_id(base),
                format!("{origin}{SAML_METADATA_PATH}"),
                "{base}"
            );
            assert_eq!(
                saml_acs_url(base),
                format!("{origin}{SAML_ACS_PATH}"),
                "{base}"
            );
        }
    }

    #[test]
    fn the_service_provider_is_the_saml_entity_id_and_acs_url() {
        for base in ["https://zone.example.com/", "https://example.com/zone"] {
            assert_eq!(
                service_provider(base),
                ServiceProvider {
                    entity_id: saml_entity_id(base),
                    acs_url: saml_acs_url(base),
                },
                "{base}"
            );
        }
        let service = service_provider("https://zone.example.com:8443/app/");
        assert_eq!(
            service.entity_id,
            "https://zone.example.com:8443/api/auth/sso/saml/metadata"
        );
        assert_eq!(
            service.acs_url,
            "https://zone.example.com:8443/api/auth/sso/saml/acs"
        );
    }

    #[test]
    fn the_saml_urls_are_the_contract_paths() {
        assert_eq!(
            saml_entity_id("https://example.com/zone"),
            "https://example.com/api/auth/sso/saml/metadata"
        );
        assert_eq!(
            saml_acs_url("https://zone.example.com:8443/"),
            "https://zone.example.com:8443/api/auth/sso/saml/acs"
        );
    }
}
