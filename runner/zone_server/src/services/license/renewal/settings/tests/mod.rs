use super::*;

const CUSTOM_URL: &str = "http://signer.internal:8080/licenses/renew";
const METADATA_URL: &str = "http://169.254.169.254/latest/meta-data";

fn lookup(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let values: Vec<(String, String)> = values
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    move |variable| {
        values
            .iter()
            .find(|(name, _)| name == variable)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn the_default_url_validates() {
    let url = validated(DEFAULT_URL).expect("the default URL is a valid target");
    assert_eq!(url.as_str(), DEFAULT_URL);
}

#[test]
fn renewal_is_on_by_default_against_the_default_url() {
    let settings = Settings::from_lookup(lookup(&[])).unwrap();
    assert!(settings.enabled, "online servers renew unless told not to");
    assert_eq!(settings.url.as_str(), DEFAULT_URL);
    assert_eq!(settings, Settings::default());
}

#[test]
fn false_turns_renewal_off() {
    for off in ["false", "0", "no", "off", " FALSE "] {
        let settings = Settings::from_lookup(lookup(&[(ENABLED_VARIABLE, off)])).unwrap();
        assert!(!settings.enabled, "{off:?} must turn renewal off");
    }
}

#[test]
fn an_unreadable_flag_keeps_renewal_on() {
    let settings = Settings::from_lookup(lookup(&[(ENABLED_VARIABLE, "maybe")])).unwrap();
    assert!(settings.enabled);
}

#[test]
fn blank_values_read_as_unset() {
    let settings =
        Settings::from_lookup(lookup(&[(ENABLED_VARIABLE, " "), (URL_VARIABLE, "  \t")])).unwrap();
    assert_eq!(settings, Settings::default());
}

#[test]
fn a_custom_url_is_used() {
    let settings =
        Settings::from_lookup(lookup(&[(URL_VARIABLE, &format!(" {CUSTOM_URL} "))])).unwrap();
    assert_eq!(settings.url.as_str(), CUSTOM_URL);
    assert!(settings.enabled);
}

#[test]
fn a_url_that_is_not_http_is_refused() {
    for refused in [
        "not a url",
        "ftp://signer.example/renew",
        "file:///etc/passwd",
    ] {
        assert_eq!(
            Settings::from_lookup(lookup(&[(URL_VARIABLE, refused)])),
            Err(SettingsError::Url),
            "{refused}"
        );
    }
}

#[test]
fn a_metadata_address_is_refused() {
    for refused in [METADATA_URL, "http://metadata.google.internal/renew"] {
        assert_eq!(
            Settings::from_lookup(lookup(&[(URL_VARIABLE, refused)])),
            Err(SettingsError::Metadata),
            "{refused}"
        );
    }
}

#[test]
fn a_url_with_credentials_is_refused() {
    for refused in [
        "https://user:secret@licenses.example.com/renew",
        "http://user@localhost:8080/renew",
    ] {
        assert_eq!(
            Settings::from_lookup(lookup(&[(URL_VARIABLE, refused)])),
            Err(SettingsError::Credentials),
            "{refused}"
        );
    }
}

#[test]
fn plain_http_to_a_public_host_is_refused() {
    for refused in [
        "http://licenses.bitecode.ltd/licenses/renew",
        "http://8.8.8.8/licenses/renew",
        "http://[2606:4700::6810:84e5]/licenses/renew",
    ] {
        assert_eq!(
            Settings::from_lookup(lookup(&[(URL_VARIABLE, refused)])),
            Err(SettingsError::Plaintext),
            "{refused}"
        );
    }
}

#[test]
fn plain_http_to_a_loopback_or_private_host_is_accepted() {
    for accepted in [
        "http://127.0.0.1:8080/licenses/renew",
        "http://localhost:8080/licenses/renew",
        "http://[::1]:8080/licenses/renew",
        "http://10.0.0.5/licenses/renew",
        "http://172.16.4.2/licenses/renew",
        "http://192.168.1.20/licenses/renew",
        "http://[fd12:3456::1]/licenses/renew",
        "http://signer:8080/licenses/renew",
        CUSTOM_URL,
    ] {
        let settings = Settings::from_lookup(lookup(&[(URL_VARIABLE, accepted)]))
            .unwrap_or_else(|error| panic!("{accepted}: {error}"));
        assert_eq!(settings.url.as_str(), accepted);
    }
}

#[test]
fn https_to_a_public_host_is_accepted() {
    for accepted in [
        "https://signer.example.com/licenses/renew",
        "https://8.8.8.8/licenses/renew",
    ] {
        let settings = Settings::from_lookup(lookup(&[(URL_VARIABLE, accepted)]))
            .unwrap_or_else(|error| panic!("{accepted}: {error}"));
        assert_eq!(settings.url.as_str(), accepted);
    }
}

#[test]
fn a_bad_url_is_refused_even_with_renewal_off() {
    assert_eq!(
        Settings::from_lookup(lookup(&[
            (ENABLED_VARIABLE, "false"),
            (URL_VARIABLE, "not a url")
        ])),
        Err(SettingsError::Url)
    );
}

#[test]
fn messages_are_fixed_and_name_no_url() {
    let cases = [
        (
            SettingsError::Url,
            "ZONE_LICENSE_RENEWAL_URL must be an http or https URL",
        ),
        (
            SettingsError::Credentials,
            "ZONE_LICENSE_RENEWAL_URL must not carry a user name or password",
        ),
        (
            SettingsError::Metadata,
            "ZONE_LICENSE_RENEWAL_URL must not name a cloud metadata address",
        ),
        (
            SettingsError::Plaintext,
            "ZONE_LICENSE_RENEWAL_URL must use https unless it names a loopback or private network host",
        ),
    ];
    for (error, expected) in cases {
        let message = error.message();
        assert_eq!(message, expected);
        assert!(message.starts_with(URL_VARIABLE), "{message}");
        assert!(!message.contains("://"), "{message}");
        assert!(!message.contains("169.254"), "{message}");
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn debug_shows_the_url_as_written() {
    assert_eq!(
        format!("{:?}", Settings::default()),
        format!("Settings {{ enabled: true, url: {DEFAULT_URL:?} }}")
    );
}
