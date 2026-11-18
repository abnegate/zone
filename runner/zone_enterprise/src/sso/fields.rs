use std::fmt;

use crate::sso::REDACTED;
use crate::sso::protocol::Protocol;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fields<'a> {
    pub protocol: Protocol,
    pub display_name: &'a str,
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub allowed_domains: &'a [String],
    pub require_verified_email: bool,
}

impl fmt::Debug for Fields<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Fields")
            .field("protocol", &self.protocol)
            .field("display_name", &self.display_name)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &REDACTED)
            .field("allowed_domains", &self.allowed_domains)
            .field("require_verified_email", &self.require_verified_email)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_client_secret() {
        let shown = format!(
            "{:?}",
            Fields {
                protocol: Protocol::Oidc,
                display_name: "Okta",
                issuer: "https://acme.okta.com",
                client_id: "0oa1",
                client_secret: "s3cret",
                allowed_domains: &[],
                require_verified_email: true,
            }
        );
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
        assert!(shown.contains("0oa1"), "{shown}");
    }
}
