use chrono::{DateTime, TimeDelta, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

use crate::sso::audience::Audience;
use crate::sso::claims::Claims;

pub const KEY_ID: &str = "test-1";
pub const SUBJECT: &str = "00u-test-subject";
pub const EMAIL: &str = "jane@acme.com";
pub const NAME: &str = "Jane Doe";
pub const MODULUS: &str = "nupMGbVq5q_Uj_SpnnHFLLlN891yK06s0OsVemQHGy5pLogTgcvM_X35MXXrm4vGTwpGmsOKKS_vGNe0VmVb37Z-Z9-TQXFiGI53cwNQtkWlIDmIvfbjhgOPssaT0_JjM9TLOdwJ6Mt8KZFevK_o-blmcbIJwIemRWNLuHrqQCi_MeDFGvz3Pmn6KFPlNQgzG5eUZ1u3hdvDzzxeLdWVi_lYPBEBVB2rRy4Zp7-DxQ2gDbZmb-pyW_dbVAlvqTOFBwIRQL9Nrm7YHqUQndqO97_vw0TQgIduqExRqhQfX_qfvptO00-ce4ikMhQOE6VkBltOO4hub1YOo_H7rNCg9Q";
pub const EXPONENT: &str = "AQAB";
pub const PRIVATE_KEY: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCe6kwZtWrmr9SP\n\
9KmeccUsuU3z3XIrTqzQ6xV6ZAcbLmkuiBOBy8z9ffkxdeubi8ZPCkaaw4opL+8Y\n\
17RWZVvftn5n35NBcWIYjndzA1C2RaUgOYi99uOGA4+yxpPT8mMz1Ms53Anoy3wp\n\
kV68r+j5uWZxsgnAh6ZFY0u4eupAKL8x4MUa/Pc+afooU+U1CDMbl5RnW7eF28PP\n\
PF4t1ZWL+Vg8EQFUHatHLhmnv4PFDaANtmZv6nJb91tUCW+pM4UHAhFAv02ubtge\n\
pRCd2o73v+/DRNCAh26oTFGqFB9f+p++m07TT5x7iKQyFA4TpWQGW047iG5vVg6j\n\
8fus0KD1AgMBAAECggEAA3ibq+4WBS7vBivybocOpwDvLnX2UfLYfIG+CNvZeeME\n\
IiOPxYSFmD3hrQ5l1JJzhPN1AllmEas93j2L710sahEK5srv5PyMh3vJKSZ341tX\n\
FYKcNYYIhztZrq5+uSDwwMeTntJrFv4m2qOVvapb/ORrU+1vIBj2bUqTc1af+kHA\n\
x35FHSy474I3Mc82H95i8AV8AvbBQa74PoVvBanehf5Uk0dHl4uC2ffL7p9b16gH\n\
38Q2qbIGcZDDd104picUHlGD8vhVbKc5LSJebuPaGBQoNpeB95vFjwO6YPMFwT5B\n\
Gw5aH4VKMrIJFV84Dddb18OAutPaeV4y6V/usbAmGwKBgQDMIVMGUOw3Yd3F9Iq1\n\
kGfvMrAJ+SmlldLobRxNGPotwwHsP7OoCjdcOwR93wYPlYteoKClxRlVfxlVLHwZ\n\
P7p17R/S3Ws3iZv2d9RJ+YQNZyXDWLkWq4ATghHED4N9U4KPLC6Qkg/1aBtzoefB\n\
/hP//OAvw5UnoNsyCb19pmMvFwKBgQDHS70d8EaO/6lRQhv/buJ34JpHYI58zTYG\n\
e9hvqr0mLX87xU9VYr0yFIqN2XuVciWdSxdJC0OKj5L/bOfqD79Kwq21UJb9MuzX\n\
wHhUrcPdlket2Niz/9EM28hjGWAh9NyBt8765vQOcbgpEZgln9JdIUsaeaJj09Bi\n\
LYCS1nBX0wKBgH1WONQ3+9JFlRZnAIRbFnYYeV6mTpwNUmLEHAx6uaI/in603mrf\n\
Wv6WfjuLJP2MpPaFSh5+1v9X+wYBOnGGX8bowhGHQr3nEIxVJ1QEkMBqBRXc9Ein\n\
JQzVYHMx01qwXAns5up9xK66LaDiWJsUltMnQcASJxMeAXjH4kG0WFIhAoGAGY2h\n\
gvwy39TyDcQvONIr4Xj/7xT3UBMz0SMB/ldjB0kgHjbPVEBjx88QcN2o1eGb5w4g\n\
vzHdiB+elneNSNqaCWjqwuvBYFeuPH+mLh1VlszVBfpe1r1/PWnUyLgRNmlh+JPr\n\
ywnf0xZLqaBi5LgYTe6d5Zgg9THMbzosWSd7ohcCgYA7+S5EbOD/y2f56akLRRIN\n\
PvLQ1iuH/+MAepVK4UvA1nIM5tTtuXsw6HR3W73hiBgTKusVoM2XoDo0Ng7TsUCy\n\
wYAmoCgPEfOfb5aGwk4bW5sfHzPKUf/xMe3Qes9Dy7Tb3AwvFb0lOqnM25hDEld2\n\
xGi/0UWB6xR4UNN1Y19hLg==\n\
-----END PRIVATE KEY-----\n\
";

const LIFETIME: TimeDelta = TimeDelta::minutes(5);
const KEY_TYPE: &str = "RSA";
const KEY_USE: &str = "sig";

pub fn key_set() -> Value {
    key_set_with(KEY_ID)
}

pub fn key_set_with(key_id: &str) -> Value {
    json!({
        "keys": [{
            "kty": KEY_TYPE,
            "use": KEY_USE,
            "alg": "RS256",
            "kid": key_id,
            "n": MODULUS,
            "e": EXPONENT,
        }]
    })
}

pub fn discovery(issuer: &str) -> Value {
    let base = issuer.trim_end_matches('/');
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    })
}

pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn claims(issuer: &str, client_id: &str, nonce: &str, now: DateTime<Utc>) -> Claims {
    Claims {
        issuer: issuer.to_owned(),
        subject: SUBJECT.to_owned(),
        audience: Audience::One(client_id.to_owned()),
        authorized_party: None,
        expires_at: (now + LIFETIME).timestamp(),
        issued_at: now.timestamp(),
        nonce: Some(nonce.to_owned()),
        email: Some(EMAIL.to_owned()),
        email_verified: Some(true),
        name: Some(NAME.to_owned()),
        hosted_domain: None,
        email_domain_owner_verified: None,
    }
}

pub fn id_token(claims: &Claims) -> String {
    id_token_with(KEY_ID, claims)
}

pub fn id_token_with(key_id: &str, claims: &Claims) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_owned());
    let key = EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes())
        .expect("the committed test key is a valid RSA private key");
    jsonwebtoken::encode(&header, claims, &key).expect("claims always serialize")
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::jwk::JwkSet;
    use jsonwebtoken::{DecodingKey, Validation};

    use super::*;

    #[test]
    fn the_published_key_verifies_the_signed_token() {
        let now = Utc::now();
        let claims = claims("https://idp.test", "zone", &nonce(), now);
        let token = id_token(&claims);
        let set: JwkSet = serde_json::from_value(key_set()).unwrap();
        let key = DecodingKey::from_jwk(set.find(KEY_ID).unwrap()).unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&["zone"]);
        let decoded = jsonwebtoken::decode::<Claims>(&token, &key, &validation).unwrap();
        assert_eq!(decoded.claims, claims);
        assert_eq!(decoded.header.kid.as_deref(), Some(KEY_ID));
    }

    #[test]
    fn the_key_is_rsa_2048() {
        assert_eq!(
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, MODULUS)
                .unwrap()
                .len(),
            256
        );
    }

    #[test]
    fn signs_with_the_requested_key_id() {
        let token = id_token_with(
            "rotated",
            &claims("https://idp.test", "zone", &nonce(), Utc::now()),
        );
        assert_eq!(
            jsonwebtoken::decode_header(&token).unwrap().kid.as_deref(),
            Some("rotated")
        );
    }

    #[test]
    fn discovery_points_at_the_issuer() {
        let document = discovery("http://127.0.0.1:9000/");
        assert_eq!(document["issuer"], "http://127.0.0.1:9000/");
        assert_eq!(document["token_endpoint"], "http://127.0.0.1:9000/token");
        assert_eq!(document["jwks_uri"], "http://127.0.0.1:9000/jwks");
    }
}
