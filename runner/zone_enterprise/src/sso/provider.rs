use crate::sso::client_id::ClientId;
use crate::sso::client_secret::ClientSecret;
use crate::sso::display_name::DisplayName;
use crate::sso::domains::Domains;
use crate::sso::error::Error;
use crate::sso::fields::Fields;
use crate::sso::issuer::Issuer;
use crate::sso::policy::Policy;
use crate::sso::protocol::Protocol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub protocol: Protocol,
    pub display_name: DisplayName,
    pub issuer: Issuer,
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub policy: Policy,
}

impl Provider {
    pub fn parse(fields: &Fields<'_>) -> Result<Self, Error> {
        Ok(Self {
            protocol: fields.protocol,
            display_name: DisplayName::parse(fields.display_name)?,
            issuer: Issuer::parse(fields.issuer)?,
            client_id: ClientId::parse(fields.client_id)?,
            client_secret: ClientSecret::parse(fields.client_secret)?,
            policy: Policy {
                domains: Domains::parse(fields.allowed_domains)?,
                require_verified_email: fields.require_verified_email,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields<'a>(domains: &'a [String]) -> Fields<'a> {
        Fields {
            protocol: Protocol::Oidc,
            display_name: " Okta ",
            issuer: "https://acme.okta.com",
            client_id: "0oa1",
            client_secret: "s3cret",
            allowed_domains: domains,
            require_verified_email: true,
        }
    }

    #[test]
    fn parses_stored_fields() {
        let domains = vec!["ACME.com".to_owned()];
        let provider = Provider::parse(&fields(&domains)).unwrap();
        assert_eq!(provider.protocol, Protocol::Oidc);
        assert_eq!(provider.display_name.as_str(), "Okta");
        assert_eq!(provider.issuer.as_str(), "https://acme.okta.com");
        assert_eq!(provider.client_id.as_str(), "0oa1");
        assert_eq!(provider.client_secret.expose(), "s3cret");
        assert_eq!(provider.policy.domains.as_slice(), ["acme.com"]);
        assert!(provider.policy.require_verified_email);
    }

    #[test]
    fn reports_the_first_invalid_field() {
        let domains = vec!["*.acme.com".to_owned()];
        let cases = [
            (
                Fields {
                    display_name: "",
                    ..fields(&[])
                },
                Error::DisplayNameInvalid,
            ),
            (
                Fields {
                    issuer: "http://acme.okta.com",
                    ..fields(&[])
                },
                Error::IssuerInvalid,
            ),
            (
                Fields {
                    client_id: " ",
                    ..fields(&[])
                },
                Error::ClientIdInvalid,
            ),
            (
                Fields {
                    client_secret: "",
                    ..fields(&[])
                },
                Error::ClientSecretInvalid,
            ),
            (fields(&domains), Error::DomainInvalid("*.acme.com".into())),
        ];
        for (fields, error) in cases {
            assert_eq!(Provider::parse(&fields), Err(error.clone()), "{error:?}");
        }
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let provider = Provider::parse(&fields(&[])).unwrap();
        assert!(!format!("{provider:?}").contains("s3cret"));
    }
}
