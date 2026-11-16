use crate::sso::claims::Claims;
use crate::sso::domains::Domains;
use crate::sso::error::Error;
use crate::sso::identity::Identity;
use crate::sso::issuer::Issuer;

const AT: char = '@';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub domains: Domains,
    pub require_verified_email: bool,
}

impl Policy {
    pub fn admit(&self, issuer: &Issuer, claims: &Claims) -> Result<Identity, Error> {
        let email = email(claims).ok_or(Error::EmailMissing)?;
        if self.require_verified_email && !verified(claims) {
            return Err(Error::EmailUnverified);
        }
        if !self.domains.allows(&email) || !self.hosted_domain_allowed(issuer, claims) {
            return Err(Error::DomainNotAllowed);
        }
        Ok(Identity {
            subject: claims.subject.clone(),
            email,
            name: claims
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
        })
    }

    fn hosted_domain_allowed(&self, issuer: &Issuer, claims: &Claims) -> bool {
        if self.domains.is_empty() {
            return true;
        }
        match claims.hosted_domain.as_deref() {
            Some(domain) => self.domains.contains(domain),
            None => !issuer.is_google(),
        }
    }
}

fn email(claims: &Claims) -> Option<String> {
    let email = claims.email.as_deref()?.trim().to_lowercase();
    let (local, domain) = email.rsplit_once(AT)?;
    (!local.is_empty() && !domain.is_empty()).then_some(email)
}

fn verified(claims: &Claims) -> bool {
    claims
        .email_verified
        .or(claims.email_domain_owner_verified)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sso::audience::Audience;
    use crate::sso::issuer::GOOGLE;

    const OKTA: &str = "https://acme.okta.com";

    fn issuer(value: &str) -> Issuer {
        Issuer::parse(value).unwrap()
    }

    fn claims() -> Claims {
        Claims {
            issuer: OKTA.into(),
            subject: "00u1".into(),
            audience: Audience::One("zone".into()),
            authorized_party: None,
            expires_at: 2,
            issued_at: 1,
            nonce: Some("nonce".into()),
            email: Some(" Jane@Acme.com ".into()),
            email_verified: Some(true),
            name: Some(" Jane Doe ".into()),
            hosted_domain: None,
            email_domain_owner_verified: None,
        }
    }

    fn policy(domains: &[&str], require_verified_email: bool) -> Policy {
        Policy {
            domains: Domains::parse(domains).unwrap(),
            require_verified_email,
        }
    }

    #[test]
    fn admits_a_verified_email_as_a_lowercased_identity() {
        let identity = policy(&[], true).admit(&issuer(OKTA), &claims()).unwrap();
        assert_eq!(
            identity,
            Identity {
                subject: "00u1".into(),
                email: "jane@acme.com".into(),
                name: Some("Jane Doe".into()),
            }
        );
    }

    #[test]
    fn a_blank_name_is_no_name() {
        let claims = Claims {
            name: Some("  ".into()),
            ..claims()
        };
        let identity = policy(&[], true).admit(&issuer(OKTA), &claims).unwrap();
        assert_eq!(identity.name, None);
    }

    #[test]
    fn refuses_a_missing_or_unusable_email() {
        for email in [
            None,
            Some(""),
            Some("  "),
            Some("jane"),
            Some("@acme.com"),
            Some("jane@"),
        ] {
            let claims = Claims {
                email: email.map(str::to_owned),
                ..claims()
            };
            assert_eq!(
                policy(&[], false).admit(&issuer(OKTA), &claims),
                Err(Error::EmailMissing),
                "{email:?}"
            );
        }
    }

    #[test]
    fn refuses_an_unverified_email_when_verification_is_required() {
        for verified in [None, Some(false)] {
            let claims = Claims {
                email_verified: verified,
                ..claims()
            };
            assert_eq!(
                policy(&[], true).admit(&issuer(OKTA), &claims),
                Err(Error::EmailUnverified),
                "{verified:?}"
            );
        }
    }

    #[test]
    fn trusts_an_unverified_email_when_verification_is_off() {
        let claims = Claims {
            email_verified: None,
            ..claims()
        };
        assert!(policy(&[], false).admit(&issuer(OKTA), &claims).is_ok());
    }

    #[test]
    fn accepts_the_string_true_as_verified() {
        let claims: Claims = serde_json::from_value(serde_json::json!({
            "iss": OKTA, "sub": "00u1", "aud": "zone", "exp": 2, "iat": 1,
            "email": "jane@acme.com", "email_verified": "true"
        }))
        .unwrap();
        assert!(policy(&[], true).admit(&issuer(OKTA), &claims).is_ok());
    }

    #[test]
    fn domain_owner_verification_counts_when_email_verified_is_absent() {
        let claims = Claims {
            email_verified: None,
            email_domain_owner_verified: Some(true),
            ..claims()
        };
        assert!(policy(&[], true).admit(&issuer(OKTA), &claims).is_ok());
    }

    #[test]
    fn an_explicit_unverified_email_overrides_domain_owner_verification() {
        let claims = Claims {
            email_verified: Some(false),
            email_domain_owner_verified: Some(true),
            ..claims()
        };
        assert_eq!(
            policy(&[], true).admit(&issuer(OKTA), &claims),
            Err(Error::EmailUnverified)
        );
    }

    #[test]
    fn allows_listed_domains_ignoring_case() {
        assert!(
            policy(&["ACME.com"], true)
                .admit(&issuer(OKTA), &claims())
                .is_ok()
        );
    }

    #[test]
    fn refuses_an_email_outside_the_allowed_domains() {
        assert_eq!(
            policy(&["example.com"], true).admit(&issuer(OKTA), &claims()),
            Err(Error::DomainNotAllowed)
        );
    }

    #[test]
    fn google_with_domains_set_and_no_hosted_domain_is_refused() {
        let claims = Claims {
            issuer: GOOGLE.into(),
            ..claims()
        };
        assert_eq!(
            policy(&["acme.com"], true).admit(&issuer(GOOGLE), &claims),
            Err(Error::DomainNotAllowed)
        );
    }

    #[test]
    fn a_hosted_domain_outside_the_allowlist_is_refused() {
        for provider in [GOOGLE, OKTA] {
            let claims = Claims {
                hosted_domain: Some("evil.test".into()),
                ..claims()
            };
            assert_eq!(
                policy(&["acme.com"], true).admit(&issuer(provider), &claims),
                Err(Error::DomainNotAllowed),
                "{provider}"
            );
        }
    }

    #[test]
    fn a_matching_hosted_domain_is_admitted() {
        let claims = Claims {
            hosted_domain: Some("Acme.com".into()),
            ..claims()
        };
        assert!(
            policy(&["acme.com"], true)
                .admit(&issuer(GOOGLE), &claims)
                .is_ok()
        );
    }

    #[test]
    fn without_domains_the_hosted_domain_is_ignored() {
        for hosted_domain in [None, Some("evil.test".to_owned())] {
            let claims = Claims {
                hosted_domain: hosted_domain.clone(),
                ..claims()
            };
            assert!(
                policy(&[], true).admit(&issuer(GOOGLE), &claims).is_ok(),
                "{hosted_domain:?}"
            );
        }
    }

    #[test]
    fn a_non_google_issuer_without_a_hosted_domain_checks_only_the_email() {
        assert!(
            policy(&["acme.com"], true)
                .admit(&issuer(OKTA), &claims())
                .is_ok()
        );
        let elsewhere = Claims {
            email: Some("jane@evil.test".into()),
            ..claims()
        };
        assert_eq!(
            policy(&["acme.com"], true).admit(&issuer(OKTA), &elsewhere),
            Err(Error::DomainNotAllowed)
        );
    }
}
