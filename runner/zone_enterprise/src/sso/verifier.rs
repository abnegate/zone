use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::sso::algorithm;
use crate::sso::claims::Claims;
use crate::sso::client_id::ClientId;
use crate::sso::error::Error;
use crate::sso::issuer::Issuer;
use crate::sso::rejection::Rejection;

pub(crate) const LEEWAY_SECONDS: i64 = 60;

const SEGMENT_SEPARATOR: char = '.';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) algorithm: Algorithm,
    pub(crate) key_id: Option<String>,
}

#[derive(Deserialize)]
struct EncodedHeader {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

pub(crate) struct Verifier<'a> {
    pub(crate) issuer: &'a Issuer,
    pub(crate) client_id: &'a ClientId,
    pub(crate) nonce: &'a str,
    pub(crate) now: DateTime<Utc>,
    pub(crate) advertised: Option<&'a [String]>,
}

impl Verifier<'_> {
    pub(crate) fn header(&self, token: &str) -> Result<Header, Error> {
        let segment = token
            .split(SEGMENT_SEPARATOR)
            .next()
            .ok_or_else(|| refuse(Rejection::Malformed))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(segment)
            .map_err(|_| refuse(Rejection::Malformed))?;
        let header: EncodedHeader =
            serde_json::from_slice(&bytes).map_err(|_| refuse(Rejection::Malformed))?;
        let algorithm =
            algorithm::parse(&header.alg).ok_or_else(|| refuse(Rejection::Algorithm))?;
        let advertised = self
            .advertised
            .is_none_or(|advertised| advertised.contains(&header.alg));
        if !advertised {
            return Err(refuse(Rejection::Algorithm));
        }
        Ok(Header {
            algorithm,
            key_id: header.kid,
        })
    }

    pub(crate) fn verify(&self, token: &str, header: &Header, key: &Jwk) -> Result<Claims, Error> {
        let claims = self.signed(token, header, key)?;
        self.check(&claims)?;
        Ok(claims)
    }

    fn signed(&self, token: &str, header: &Header, key: &Jwk) -> Result<Claims, Error> {
        let declared = key
            .common
            .key_algorithm
            .is_none_or(|declared| Algorithm::try_from(declared).ok() == Some(header.algorithm));
        let decoding = DecodingKey::from_jwk(key).map_err(|_| refuse(Rejection::KeyMismatch))?;
        if !declared || decoding.family() != header.algorithm.family() {
            return Err(refuse(Rejection::KeyMismatch));
        }
        let mut validation = Validation::new(header.algorithm);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        jsonwebtoken::decode::<Claims>(token, &decoding, &validation)
            .map(|data| data.claims)
            .map_err(|error| {
                refuse(match error.kind() {
                    ErrorKind::InvalidSignature => Rejection::Signature,
                    ErrorKind::InvalidAlgorithm => Rejection::Algorithm,
                    ErrorKind::InvalidEcdsaKey
                    | ErrorKind::InvalidEddsaKey
                    | ErrorKind::InvalidRsaKey(_)
                    | ErrorKind::InvalidKeyFormat => Rejection::KeyMismatch,
                    _ => Rejection::Malformed,
                })
            })
    }

    fn check(&self, claims: &Claims) -> Result<(), Error> {
        let client_id = self.client_id.as_str();
        let now = self.now.timestamp();
        if claims.subject.is_empty() {
            return Err(refuse(Rejection::Malformed));
        }
        if !self.issuer.accepts(&claims.issuer) {
            return Err(refuse(Rejection::Issuer));
        }
        if !claims.audience.contains(client_id) {
            return Err(refuse(Rejection::Audience));
        }
        let party = claims.authorized_party.as_deref();
        let party_required = claims.audience.several();
        if (party_required && party != Some(client_id))
            || party.is_some_and(|party| party != client_id)
        {
            return Err(refuse(Rejection::AuthorizedParty));
        }
        if claims.expires_at.saturating_add(LEEWAY_SECONDS) < now {
            return Err(refuse(Rejection::Expired));
        }
        if claims.issued_at > now.saturating_add(LEEWAY_SECONDS) {
            return Err(refuse(Rejection::IssuedInFuture));
        }
        if claims.nonce.as_deref() != Some(self.nonce) {
            return Err(refuse(Rejection::Nonce));
        }
        Ok(())
    }
}

fn refuse(rejection: Rejection) -> Error {
    Error::IdTokenInvalid(rejection)
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use jsonwebtoken::jwk::JwkSet;
    use serde_json::json;

    use super::*;
    use crate::sso::audience::Audience;
    use crate::sso::issuer::{GOOGLE, GOOGLE_HOST};
    use crate::sso::testing;

    const ISSUER: &str = "https://acme.okta.com";
    const CLIENT: &str = "zone";
    const NONCE: &str = "nonce-1";

    struct Fixture {
        issuer: Issuer,
        client_id: ClientId,
        now: DateTime<Utc>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                issuer: Issuer::parse(ISSUER).unwrap(),
                client_id: ClientId::parse(CLIENT).unwrap(),
                now: Utc::now(),
            }
        }

        fn verifier<'a>(&'a self, advertised: Option<&'a [String]>) -> Verifier<'a> {
            Verifier {
                issuer: &self.issuer,
                client_id: &self.client_id,
                nonce: NONCE,
                now: self.now,
                advertised,
            }
        }

        fn claims(&self) -> Claims {
            testing::claims(ISSUER, CLIENT, NONCE, self.now)
        }

        fn verify(&self, token: &str) -> Result<Claims, Error> {
            let verifier = self.verifier(None);
            let header = verifier.header(token)?;
            verifier.verify(token, &header, &key())
        }
    }

    fn key() -> Jwk {
        let set: JwkSet = serde_json::from_value(testing::key_set()).unwrap();
        set.keys[0].clone()
    }

    fn unsigned(header: serde_json::Value, claims: &Claims) -> String {
        let encode = |value: &serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
        format!(
            "{}.{}.{}",
            encode(&header),
            encode(&serde_json::to_value(claims).unwrap()),
            URL_SAFE_NO_PAD.encode(b"signature")
        )
    }

    #[test]
    fn accepts_a_valid_token() {
        let fixture = Fixture::new();
        let claims = fixture.claims();
        assert_eq!(fixture.verify(&testing::id_token(&claims)).unwrap(), claims);
    }

    #[test]
    fn reads_the_header() {
        let fixture = Fixture::new();
        let header = fixture
            .verifier(None)
            .header(&testing::id_token(&fixture.claims()))
            .unwrap();
        assert_eq!(header.algorithm, Algorithm::RS256);
        assert_eq!(header.key_id.as_deref(), Some(testing::KEY_ID));
    }

    #[test]
    fn refuses_a_bad_signature() {
        let fixture = Fixture::new();
        let token = testing::id_token(&fixture.claims());
        let (signed, _) = token.rsplit_once('.').unwrap();
        let forged = format!("{signed}.{}", URL_SAFE_NO_PAD.encode([7u8; 256]));
        assert_eq!(
            fixture.verify(&forged),
            Err(Error::IdTokenInvalid(Rejection::Signature))
        );
    }

    #[test]
    fn refuses_tampered_claims() {
        let fixture = Fixture::new();
        let token = testing::id_token(&fixture.claims());
        let mut segments: Vec<&str> = token.split('.').collect();
        let tampered = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Claims {
                subject: "admin".into(),
                ..fixture.claims()
            })
            .unwrap(),
        );
        segments[1] = &tampered;
        assert_eq!(
            fixture.verify(&segments.join(".")),
            Err(Error::IdTokenInvalid(Rejection::Signature))
        );
    }

    #[test]
    fn refuses_the_wrong_issuer() {
        let fixture = Fixture::new();
        for issuer in ["https://evil.test", "https://acme.okta.com/"] {
            let claims = Claims {
                issuer: issuer.into(),
                ..fixture.claims()
            };
            assert_eq!(
                fixture.verify(&testing::id_token(&claims)),
                Err(Error::IdTokenInvalid(Rejection::Issuer)),
                "{issuer}"
            );
        }
    }

    #[test]
    fn a_google_token_may_name_its_issuer_without_the_scheme() {
        let fixture = Fixture {
            issuer: Issuer::parse(GOOGLE).unwrap(),
            ..Fixture::new()
        };
        for issuer in [GOOGLE, GOOGLE_HOST] {
            let claims = Claims {
                issuer: issuer.into(),
                ..fixture.claims()
            };
            assert_eq!(
                fixture.verify(&testing::id_token(&claims)),
                Ok(claims),
                "{issuer}"
            );
        }
        let claims = Claims {
            issuer: "https://evil.test".into(),
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&claims)),
            Err(Error::IdTokenInvalid(Rejection::Issuer))
        );
    }

    #[test]
    fn any_other_issuer_without_its_scheme_is_refused() {
        let fixture = Fixture::new();
        let claims = Claims {
            issuer: "acme.okta.com".into(),
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&claims)),
            Err(Error::IdTokenInvalid(Rejection::Issuer))
        );
    }

    #[test]
    fn refuses_the_wrong_audience() {
        let fixture = Fixture::new();
        for audience in [
            Audience::One("other".into()),
            Audience::Many(vec!["other".into(), "api".into()]),
            Audience::Many(Vec::new()),
        ] {
            let claims = Claims {
                audience: audience.clone(),
                ..fixture.claims()
            };
            assert_eq!(
                fixture.verify(&testing::id_token(&claims)),
                Err(Error::IdTokenInvalid(Rejection::Audience)),
                "{audience:?}"
            );
        }
    }

    #[test]
    fn several_audiences_need_this_client_as_authorized_party() {
        let fixture = Fixture::new();
        let several = Audience::Many(vec![CLIENT.into(), "api".into()]);
        for party in [None, Some("api")] {
            let claims = Claims {
                audience: several.clone(),
                authorized_party: party.map(str::to_owned),
                ..fixture.claims()
            };
            assert_eq!(
                fixture.verify(&testing::id_token(&claims)),
                Err(Error::IdTokenInvalid(Rejection::AuthorizedParty)),
                "{party:?}"
            );
        }
        let claims = Claims {
            audience: several,
            authorized_party: Some(CLIENT.into()),
            ..fixture.claims()
        };
        assert!(fixture.verify(&testing::id_token(&claims)).is_ok());
    }

    #[test]
    fn a_present_authorized_party_must_be_this_client() {
        let fixture = Fixture::new();
        let claims = Claims {
            authorized_party: Some("api".into()),
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&claims)),
            Err(Error::IdTokenInvalid(Rejection::AuthorizedParty))
        );
    }

    #[test]
    fn a_single_audience_in_an_array_needs_no_authorized_party() {
        let fixture = Fixture::new();
        let claims = Claims {
            audience: Audience::Many(vec![CLIENT.into()]),
            ..fixture.claims()
        };
        assert!(fixture.verify(&testing::id_token(&claims)).is_ok());
    }

    #[test]
    fn expiry_allows_sixty_seconds_of_skew() {
        let fixture = Fixture::new();
        let now = fixture.now.timestamp();
        let within = Claims {
            expires_at: now - LEEWAY_SECONDS,
            ..fixture.claims()
        };
        assert!(fixture.verify(&testing::id_token(&within)).is_ok());
        let expired = Claims {
            expires_at: now - LEEWAY_SECONDS - 1,
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&expired)),
            Err(Error::IdTokenInvalid(Rejection::Expired))
        );
    }

    #[test]
    fn issue_time_allows_sixty_seconds_of_skew() {
        let fixture = Fixture::new();
        let now = fixture.now.timestamp();
        let within = Claims {
            issued_at: now + LEEWAY_SECONDS,
            ..fixture.claims()
        };
        assert!(fixture.verify(&testing::id_token(&within)).is_ok());
        let future = Claims {
            issued_at: now + LEEWAY_SECONDS + 1,
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&future)),
            Err(Error::IdTokenInvalid(Rejection::IssuedInFuture))
        );
    }

    #[test]
    fn refuses_a_wrong_or_missing_nonce() {
        let fixture = Fixture::new();
        for nonce in [Some("nonce-2"), None] {
            let claims = Claims {
                nonce: nonce.map(str::to_owned),
                ..fixture.claims()
            };
            assert_eq!(
                fixture.verify(&testing::id_token(&claims)),
                Err(Error::IdTokenInvalid(Rejection::Nonce)),
                "{nonce:?}"
            );
        }
    }

    #[test]
    fn refuses_an_empty_subject() {
        let fixture = Fixture::new();
        let claims = Claims {
            subject: String::new(),
            ..fixture.claims()
        };
        assert_eq!(
            fixture.verify(&testing::id_token(&claims)),
            Err(Error::IdTokenInvalid(Rejection::Malformed))
        );
    }

    #[test]
    fn refuses_alg_none() {
        let fixture = Fixture::new();
        let token = unsigned(
            json!({"alg": "none", "kid": testing::KEY_ID}),
            &fixture.claims(),
        );
        assert_eq!(
            fixture.verifier(None).header(&token),
            Err(Error::IdTokenInvalid(Rejection::Algorithm))
        );
    }

    #[test]
    fn refuses_hmac_even_with_the_public_key_as_secret() {
        let fixture = Fixture::new();
        for algorithm in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
            let mut header = jsonwebtoken::Header::new(algorithm);
            header.kid = Some(testing::KEY_ID.into());
            let token = jsonwebtoken::encode(
                &header,
                &fixture.claims(),
                &jsonwebtoken::EncodingKey::from_secret(testing::MODULUS.as_bytes()),
            )
            .unwrap();
            assert_eq!(
                fixture.verify(&token),
                Err(Error::IdTokenInvalid(Rejection::Algorithm)),
                "{algorithm:?}"
            );
        }
    }

    #[test]
    fn refuses_an_algorithm_the_provider_does_not_advertise() {
        let fixture = Fixture::new();
        let token = testing::id_token(&fixture.claims());
        let advertised = vec!["ES256".to_owned()];
        assert_eq!(
            fixture.verifier(Some(&advertised)).header(&token),
            Err(Error::IdTokenInvalid(Rejection::Algorithm))
        );
        let advertised = vec!["ES256".to_owned(), "RS256".to_owned()];
        assert!(fixture.verifier(Some(&advertised)).header(&token).is_ok());
    }

    #[test]
    fn refuses_a_key_declared_for_another_algorithm() {
        let fixture = Fixture::new();
        let mut key = key();
        key.common.key_algorithm = Some(jsonwebtoken::jwk::KeyAlgorithm::RS512);
        let token = testing::id_token(&fixture.claims());
        let verifier = fixture.verifier(None);
        let header = verifier.header(&token).unwrap();
        assert_eq!(
            verifier.verify(&token, &header, &key),
            Err(Error::IdTokenInvalid(Rejection::KeyMismatch))
        );
    }

    #[test]
    fn refuses_a_key_from_another_family() {
        let fixture = Fixture::new();
        let ec: Jwk = serde_json::from_value(json!({
            "kty": "EC", "crv": "P-256", "kid": testing::KEY_ID,
            "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
            "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
        }))
        .unwrap();
        let token = testing::id_token(&fixture.claims());
        let verifier = fixture.verifier(None);
        let header = verifier.header(&token).unwrap();
        assert_eq!(
            verifier.verify(&token, &header, &ec),
            Err(Error::IdTokenInvalid(Rejection::KeyMismatch))
        );
    }

    #[test]
    fn refuses_garbage() {
        let fixture = Fixture::new();
        for token in ["", "abc", "!!!.e30.e30", "e30.e30.e30"] {
            assert!(
                matches!(fixture.verify(token), Err(Error::IdTokenInvalid(_))),
                "{token}"
            );
        }
    }

    #[test]
    fn accepts_a_token_signed_a_minute_ago() {
        let mut fixture = Fixture::new();
        let claims = fixture.claims();
        fixture.now += TimeDelta::minutes(1);
        assert!(fixture.verify(&testing::id_token(&claims)).is_ok());
    }
}
