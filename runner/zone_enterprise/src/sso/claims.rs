use serde::{Deserialize, Deserializer, Serialize};

use crate::sso::audience::Audience;

const TRUE: &str = "true";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    #[serde(rename = "iss")]
    pub issuer: String,
    #[serde(rename = "sub")]
    pub subject: String,
    #[serde(rename = "aud")]
    pub audience: Audience,
    #[serde(rename = "azp", default, skip_serializing_if = "Option::is_none")]
    pub authorized_party: Option<String>,
    #[serde(rename = "exp")]
    pub expires_at: i64,
    #[serde(rename = "iat")]
    pub issued_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient_boolean",
        skip_serializing_if = "Option::is_none"
    )]
    pub email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "hd", default, skip_serializing_if = "Option::is_none")]
    pub hosted_domain: Option<String>,
    #[serde(
        rename = "xms_edov",
        default,
        deserialize_with = "lenient_boolean",
        skip_serializing_if = "Option::is_none"
    )]
    pub email_domain_owner_verified: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LenientBoolean {
    Boolean(bool),
    Text(String),
}

pub(crate) fn lenient_boolean<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<bool>, D::Error> {
    Ok(
        Option::<LenientBoolean>::deserialize(deserializer)?.map(|value| match value {
            LenientBoolean::Boolean(value) => value,
            LenientBoolean::Text(value) => value == TRUE,
        }),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> Claims {
        serde_json::from_value(value).unwrap()
    }

    fn minimal() -> serde_json::Value {
        json!({"iss": "https://idp", "sub": "s1", "aud": "zone", "exp": 2, "iat": 1})
    }

    #[test]
    fn reads_the_standard_claims_under_their_wire_names() {
        let claims = parse(json!({
            "iss": "https://idp", "sub": "s1", "aud": ["zone", "api"], "azp": "zone",
            "exp": 2, "iat": 1, "nonce": "n", "email": "a@acme.com", "email_verified": true,
            "name": "Ada", "hd": "acme.com", "xms_edov": true
        }));
        assert_eq!(claims.issuer, "https://idp");
        assert_eq!(claims.subject, "s1");
        assert_eq!(
            claims.audience,
            Audience::Many(vec!["zone".into(), "api".into()])
        );
        assert_eq!(claims.authorized_party.as_deref(), Some("zone"));
        assert_eq!((claims.issued_at, claims.expires_at), (1, 2));
        assert_eq!(claims.nonce.as_deref(), Some("n"));
        assert_eq!(claims.email.as_deref(), Some("a@acme.com"));
        assert_eq!(claims.email_verified, Some(true));
        assert_eq!(claims.name.as_deref(), Some("Ada"));
        assert_eq!(claims.hosted_domain.as_deref(), Some("acme.com"));
        assert_eq!(claims.email_domain_owner_verified, Some(true));
    }

    #[test]
    fn optional_claims_default_to_none() {
        let claims = parse(minimal());
        assert_eq!(claims.authorized_party, None);
        assert_eq!(claims.nonce, None);
        assert_eq!(claims.email, None);
        assert_eq!(claims.email_verified, None);
        assert_eq!(claims.hosted_domain, None);
        assert_eq!(claims.email_domain_owner_verified, None);
    }

    #[test]
    fn verification_flags_accept_a_boolean_or_a_string() {
        for (value, expected) in [
            (json!(true), Some(true)),
            (json!(false), Some(false)),
            (json!("true"), Some(true)),
            (json!("false"), Some(false)),
            (json!("yes"), Some(false)),
            (json!(null), None),
        ] {
            let mut claims = minimal();
            claims["email_verified"] = value.clone();
            claims["xms_edov"] = value.clone();
            let claims = parse(claims);
            assert_eq!(claims.email_verified, expected, "{value}");
            assert_eq!(claims.email_domain_owner_verified, expected, "{value}");
        }
    }

    #[test]
    fn round_trips_through_its_wire_names() {
        let claims = parse(minimal());
        assert_eq!(serde_json::to_value(&claims).unwrap(), minimal());
    }
}
