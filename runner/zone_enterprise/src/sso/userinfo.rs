use serde::Deserialize;

use crate::sso::claims::{Claims, lenient_boolean};
use crate::sso::error::Error;
use crate::sso::rejection::Rejection;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Userinfo {
    #[serde(rename = "sub")]
    pub(crate) subject: String,
    #[serde(default)]
    pub(crate) email: Option<String>,
    #[serde(default, deserialize_with = "lenient_boolean")]
    pub(crate) email_verified: Option<bool>,
    #[serde(default)]
    pub(crate) name: Option<String>,
}

impl Userinfo {
    /// Fills only what the ID token left out. A verification flag describes the address it came
    /// with, so it is borrowed only when userinfo's address is the one the sign-in will use.
    pub(crate) fn complete(self, claims: Claims) -> Result<Claims, Error> {
        if self.subject != claims.subject {
            return Err(Error::IdTokenInvalid(Rejection::Subject));
        }
        let email = claims.email.clone().or_else(|| self.email.clone());
        let describes = email
            .as_deref()
            .zip(self.email.as_deref())
            .is_some_and(|(used, described)| used.trim().eq_ignore_ascii_case(described.trim()));
        Ok(Claims {
            email_verified: claims
                .email_verified
                .or(self.email_verified.filter(|_| describes)),
            name: claims.name.clone().or(self.name),
            email,
            ..claims
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::sso::testing;

    fn claims() -> Claims {
        Claims {
            email: None,
            email_verified: None,
            name: None,
            ..testing::claims("https://acme.okta.com", "zone", "nonce", Utc::now())
        }
    }

    fn userinfo(value: serde_json::Value) -> Userinfo {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn reads_the_standard_claims_with_a_lenient_verification_flag() {
        assert_eq!(
            userinfo(json!({
                "sub": "s1", "email": "a@acme.com", "email_verified": "true", "name": "Ada",
                "locale": "en"
            })),
            Userinfo {
                subject: "s1".into(),
                email: Some("a@acme.com".into()),
                email_verified: Some(true),
                name: Some("Ada".into()),
            }
        );
        assert_eq!(
            userinfo(json!({"sub": "s1"})),
            Userinfo {
                subject: "s1".into(),
                email: None,
                email_verified: None,
                name: None,
            }
        );
    }

    #[test]
    fn fills_what_the_id_token_left_out() {
        let completed = userinfo(json!({
            "sub": testing::SUBJECT, "email": testing::EMAIL, "email_verified": true,
            "name": testing::NAME
        }))
        .complete(claims())
        .unwrap();
        assert_eq!(completed.email.as_deref(), Some(testing::EMAIL));
        assert_eq!(completed.email_verified, Some(true));
        assert_eq!(completed.name.as_deref(), Some(testing::NAME));
        assert_eq!(completed.subject, testing::SUBJECT);
    }

    #[test]
    fn never_overrides_the_id_token() {
        let token = Claims {
            email: Some("jane@acme.com".into()),
            email_verified: Some(false),
            name: Some("Jane".into()),
            ..claims()
        };
        let completed = userinfo(json!({
            "sub": testing::SUBJECT, "email": "jane@acme.com", "email_verified": true,
            "name": "Someone Else"
        }))
        .complete(token.clone())
        .unwrap();
        assert_eq!(completed, token);
    }

    #[test]
    fn borrows_the_verification_flag_only_for_the_same_address() {
        let token = Claims {
            email: Some("Jane@Acme.com".into()),
            ..claims()
        };
        let same = userinfo(json!({
            "sub": testing::SUBJECT, "email": "jane@acme.com", "email_verified": true
        }))
        .complete(token.clone())
        .unwrap();
        assert_eq!(same.email.as_deref(), Some("Jane@Acme.com"));
        assert_eq!(same.email_verified, Some(true));

        let other = userinfo(json!({
            "sub": testing::SUBJECT, "email": "mallory@acme.com", "email_verified": true
        }))
        .complete(token.clone())
        .unwrap();
        assert_eq!(other.email.as_deref(), Some("Jane@Acme.com"));
        assert_eq!(other.email_verified, None);

        let silent = userinfo(json!({"sub": testing::SUBJECT, "email_verified": true}))
            .complete(token)
            .unwrap();
        assert_eq!(silent.email_verified, None);
    }

    #[test]
    fn refuses_a_different_subject() {
        assert_eq!(
            userinfo(json!({"sub": "someone-else", "email": testing::EMAIL})).complete(claims()),
            Err(Error::IdTokenInvalid(Rejection::Subject))
        );
    }
}
