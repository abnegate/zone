use std::num::NonZeroU32;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeDelta, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{Value, json};
use uuid::Uuid;

use super::*;
use crate::private_key::PrivateKey;
use crate::signer::Signer;

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [11; 32];
const SERVICE: &str = "registry.example.test";
const KNOWN_KEY_ID: &str = "registry-test-1-1";
const KNOWN_X: &str = "g1kcsJcjDfdiHO-H5SdA0bsEfEgsEA-7VBBJc1LBHlE";
const KNOWN_SECOND_X: &str = "Fq5Iaj668aHaqj-0YFMQJ0fGRqDm5DOhDkKSMjqtxxY";

fn signer() -> Signer {
    Signer::new(PrivateKey::new(SEED), KEY_ID)
}

fn generation(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn key() -> TokenKey {
    TokenKey::derive(&signer(), generation(1))
}

fn subject() -> Uuid {
    Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_1e01)
}

fn jti() -> Uuid {
    Uuid::from_u128(0x6f1c_2b3a_4d5e_4f60_8a7b_9c0d_1e2f_3a4b)
}

fn issued_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-23T04:05:06Z")
        .unwrap()
        .to_utc()
}

fn server_pull() -> Vec<Access> {
    vec![Access {
        kind: ResourceType::Repository,
        name: "zone/server".into(),
        actions: vec![Action::Pull],
    }]
}

fn decoded(segment: &str) -> Value {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).unwrap()).unwrap()
}

fn segments(token: &str) -> [&str; 3] {
    let parts: Vec<&str> = token.split('.').collect();
    parts.try_into().expect("a compact JWS has three segments")
}

fn scopes(values: &[&str]) -> Scopes {
    Scopes::parse(values.iter().copied()).unwrap()
}

fn granted(values: &[&str]) -> Grant {
    grant(&scopes(values), &Repositories::default())
}

fn pull(name: &str) -> Access {
    Access {
        kind: ResourceType::Repository,
        name: name.into(),
        actions: vec![Action::Pull],
    }
}

#[test]
fn derives_the_known_token_key() {
    let key = key();
    assert_eq!(key.key_id(), KNOWN_KEY_ID);
    assert_eq!(URL_SAFE_NO_PAD.encode(key.public()), KNOWN_X);
    assert_eq!(key.jwk().x(), KNOWN_X);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(TokenKey::derive(&signer(), generation(2)).public()),
        KNOWN_SECOND_X
    );
}

#[test]
fn derives_with_the_documented_salt_and_key_id_prefix() {
    assert_eq!(SALT, b"zone-license registry token");
    assert_eq!(KEY_ID_PREFIX, "registry");
    assert_eq!(
        TokenKey::derive(&Signer::new(PrivateKey::new(SEED), "2026-2"), generation(7)).key_id(),
        "registry-2026-2-7"
    );
}

#[test]
fn derivation_is_deterministic() {
    assert_eq!(key().public(), key().public());
}

#[test]
fn derived_keys_differ_by_generation() {
    assert_ne!(
        key().public(),
        TokenKey::derive(&signer(), generation(2)).public()
    );
}

#[test]
fn derived_keys_differ_by_key_id() {
    let other = Signer::new(PrivateKey::new(SEED), "test-2");
    assert_ne!(
        key().public(),
        TokenKey::derive(&other, generation(1)).public()
    );
}

#[test]
fn derived_keys_differ_by_seed() {
    let other = Signer::new(PrivateKey::new([12; 32]), KEY_ID);
    assert_ne!(
        key().public(),
        TokenKey::derive(&other, generation(1)).public()
    );
}

#[test]
fn the_derived_key_is_not_the_license_key() {
    assert_ne!(key().public(), signer().public());
    assert_ne!(key().public(), PrivateKey::new(SEED).public());
}

#[test]
fn debug_shows_the_key_id_and_public_key_only() {
    let shown = format!("{:?} {:#?}", key(), key());
    assert!(shown.contains(KNOWN_KEY_ID), "{shown}");
    assert!(shown.contains(KNOWN_X), "{shown}");
    assert!(!shown.contains("11, 11"), "{shown}");
    assert!(!shown.contains(&PrivateKey::new(SEED).encode()), "{shown}");
}

#[test]
fn the_jwk_has_the_go_jose_okp_shape() {
    assert_eq!(
        serde_json::to_value(key().jwk()).unwrap(),
        json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "alg": "EdDSA",
            "use": "sig",
            "kid": KNOWN_KEY_ID,
            "x": KNOWN_X,
        })
    );
}

#[test]
fn the_key_set_lists_each_generation_once() {
    let set = Jwks::derive(&signer(), &[generation(1), generation(2), generation(1)]);
    assert_eq!(
        serde_json::to_value(&set).unwrap(),
        json!({
            "keys": [
                {
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "alg": "EdDSA",
                    "use": "sig",
                    "kid": "registry-test-1-1",
                    "x": KNOWN_X,
                },
                {
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "alg": "EdDSA",
                    "use": "sig",
                    "kid": "registry-test-1-2",
                    "x": KNOWN_SECOND_X,
                },
            ]
        })
    );
}

#[test]
fn a_minted_token_has_the_contract_header() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        issued_at(),
        jti(),
    );
    let [header, _, _] = segments(&token);
    assert_eq!(
        decoded(header),
        json!({"alg": "EdDSA", "typ": "JWT", "kid": KNOWN_KEY_ID})
    );
    let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    assert_eq!(
        raw,
        r#"{"alg":"EdDSA","typ":"JWT","kid":"registry-test-1-1"}"#
    );
}

#[test]
fn a_minted_token_carries_exactly_the_contract_claims() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        issued_at(),
        jti(),
    );
    let [_, claims, _] = segments(&token);
    let issued = issued_at().timestamp();
    assert_eq!(
        decoded(claims),
        json!({
            "iss": "zone-license",
            "sub": "0193a8e2-7c4b-7d10-8f00-4d2c5a9b1e01",
            "aud": SERVICE,
            "iat": issued,
            "nbf": issued,
            "exp": issued + 300,
            "jti": "6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b",
            "access": [{"type": "repository", "name": "zone/server", "actions": ["pull"]}],
        })
    );
    assert_eq!(ISSUER, "zone-license");
    assert_eq!(LIFETIME_SECONDS, 300);
}

#[test]
fn a_token_without_access_carries_an_empty_list() {
    let token = mint(&key(), SERVICE, subject(), &[], issued_at(), jti());
    let [_, claims, _] = segments(&token);
    assert_eq!(decoded(claims)["access"], json!([]));
}

#[test]
fn a_minted_token_is_unpadded_base64url() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        issued_at(),
        jti(),
    );
    assert!(
        token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character)),
        "{token}"
    );
    let [_, _, signature] = segments(&token);
    assert_eq!(URL_SAFE_NO_PAD.decode(signature).unwrap().len(), 64);
}

#[test]
fn minting_is_deterministic_for_the_same_inputs() {
    assert_eq!(
        mint(
            &key(),
            SERVICE,
            subject(),
            &server_pull(),
            issued_at(),
            jti()
        ),
        mint(
            &key(),
            SERVICE,
            subject(),
            &server_pull(),
            issued_at(),
            jti()
        )
    );
    assert_ne!(
        mint(
            &key(),
            SERVICE,
            subject(),
            &server_pull(),
            issued_at(),
            jti()
        ),
        mint(
            &key(),
            SERVICE,
            subject(),
            &server_pull(),
            issued_at(),
            Uuid::from_u128(1)
        )
    );
}

#[test]
fn a_minted_token_verifies_with_ed25519_dalek() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        issued_at(),
        jti(),
    );
    let (message, signature) = token.rsplit_once('.').unwrap();
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
    let public = VerifyingKey::from_bytes(&key().public()).unwrap();
    public.verify(message.as_bytes(), &signature).unwrap();
    public
        .verify_strict(message.as_bytes(), &signature)
        .unwrap();
    let other =
        VerifyingKey::from_bytes(&TokenKey::derive(&signer(), generation(2)).public()).unwrap();
    assert!(other.verify(message.as_bytes(), &signature).is_err());
    let tampered = message.replacen('e', "f", 1);
    assert!(public.verify(tampered.as_bytes(), &signature).is_err());
}

fn validation(audience: &str) -> Validation {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(&[audience]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "nbf", "iat", "aud", "iss", "sub"]);
    validation.validate_nbf = true;
    validation
}

fn decoding_key(key: &TokenKey) -> DecodingKey {
    DecodingKey::from_ed_components(key.jwk().x()).unwrap()
}

#[test]
fn a_minted_token_verifies_with_jsonwebtoken() {
    let now = Utc::now();
    let token = mint(&key(), SERVICE, subject(), &server_pull(), now, jti());
    let data =
        jsonwebtoken::decode::<Value>(&token, &decoding_key(&key()), &validation(SERVICE)).unwrap();
    assert_eq!(data.header.alg, Algorithm::EdDSA);
    assert_eq!(data.header.kid.as_deref(), Some(KNOWN_KEY_ID));
    assert_eq!(data.claims["sub"], subject().to_string());
    assert_eq!(data.claims["exp"], now.timestamp() + LIFETIME_SECONDS);
}

#[test]
fn jsonwebtoken_refuses_another_audience() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        Utc::now(),
        jti(),
    );
    let error = jsonwebtoken::decode::<Value>(
        &token,
        &decoding_key(&key()),
        &validation("registry.other.test"),
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        &jsonwebtoken::errors::ErrorKind::InvalidAudience
    );
}

#[test]
fn jsonwebtoken_refuses_an_expired_token() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        Utc::now() - TimeDelta::hours(1),
        jti(),
    );
    let error = jsonwebtoken::decode::<Value>(&token, &decoding_key(&key()), &validation(SERVICE))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        &jsonwebtoken::errors::ErrorKind::ExpiredSignature
    );
}

#[test]
fn jsonwebtoken_refuses_another_generation_key() {
    let token = mint(
        &key(),
        SERVICE,
        subject(),
        &server_pull(),
        Utc::now(),
        jti(),
    );
    let other = TokenKey::derive(&signer(), generation(2));
    let error = jsonwebtoken::decode::<Value>(&token, &decoding_key(&other), &validation(SERVICE))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        &jsonwebtoken::errors::ErrorKind::InvalidSignature
    );
}

#[test]
fn parses_a_repository_scope() {
    assert_eq!(
        Scope::parse("repository:zone/server:pull,push").unwrap(),
        Scope {
            kind: ResourceType::Repository,
            name: "zone/server".into(),
            actions: vec![Action::Pull, Action::Push],
        }
    );
}

#[test]
fn parses_names_with_separators_and_a_host_port_prefix() {
    for (scope, name) in [
        ("repository:zone/lite-llm:pull", "zone/lite-llm"),
        ("repository:zone/a.b_c__d--e:pull", "zone/a.b_c__d--e"),
        ("repository:library/traefik:pull", "library/traefik"),
        ("repository:server:pull", "server"),
        (
            "repository:registry.bitecode.ltd:5000/zone/server:pull",
            "registry.bitecode.ltd:5000/zone/server",
        ),
        (
            "repository:Localhost/zone/server:pull",
            "Localhost/zone/server",
        ),
        ("repository:Zone/server:pull", "Zone/server"),
    ] {
        assert_eq!(Scope::parse(scope).unwrap().name, name, "{scope}");
    }
}

#[test]
fn parses_registry_catalog_and_wildcard_actions() {
    assert_eq!(
        Scope::parse("registry:catalog:*").unwrap(),
        Scope {
            kind: ResourceType::Registry,
            name: "catalog".into(),
            actions: vec![Action::Wildcard],
        }
    );
    assert_eq!(
        Scope::parse("repository(plugin):zone/server:pull")
            .unwrap()
            .kind,
        ResourceType::Other("repository(plugin)".into())
    );
    assert_eq!(
        Scope::parse("repository:zone/server:pull,delete,metadata")
            .unwrap()
            .actions,
        [
            Action::Pull,
            Action::Delete,
            Action::Other("metadata".into())
        ]
    );
}

#[test]
fn a_repeated_action_is_kept_once() {
    assert_eq!(
        Scope::parse("repository:zone/server:pull,pull")
            .unwrap()
            .actions,
        [Action::Pull]
    );
}

#[test]
fn refuses_malformed_scopes() {
    for scope in [
        "repository",
        "repository:zone/server",
        "repository::pull",
        ":zone/server:pull",
        "Repository:zone/server:pull",
        "repository(:zone/server:pull",
        "repository(plugin:zone/server:pull",
        "repository:zone/server:",
        "repository:zone/server:pull,",
        "repository:zone/server:Pull",
        "repository:zone/server:pull push",
        "repository:zone/Server:pull",
        "repository:zone//server:pull",
        "repository:/zone/server:pull",
        "repository:zone/server/:pull",
        "repository:zone/-server:pull",
        "repository:zone/server.:pull",
        "repository:zone/a...b:pull",
        "repository:localhost:/zone/server:pull",
        "repository:-host/zone/server:pull",
        "repository:host:50a0/zone/server:pull",
        "repository:zone/server:pull:push",
        "repository:zone/ser\u{e9}ver:pull",
    ] {
        assert!(
            matches!(Scope::parse(scope), Err(ScopeError::Malformed { scope: found }) if found == scope),
            "accepted {scope:?}"
        );
    }
}

#[test]
fn parses_repeated_and_space_separated_scope_values() {
    let parsed = scopes(&[
        "repository:zone/server:pull repository:zone/manager:pull",
        "repository:zone/litellm:pull",
    ]);
    let names: Vec<&str> = parsed.iter().map(|scope| scope.name.as_str()).collect();
    assert_eq!(names, ["zone/server", "zone/manager", "zone/litellm"]);
}

#[test]
fn empty_scope_values_parse_to_no_scopes() {
    assert!(scopes(&[]).is_empty());
    assert!(scopes(&["", " ", "  "]).is_empty());
}

#[test]
fn one_malformed_scope_refuses_the_whole_request() {
    let error = Scopes::parse(["repository:zone/server:pull nonsense"]).unwrap_err();
    assert_eq!(
        error,
        ScopeError::Malformed {
            scope: "nonsense".into()
        }
    );
}

#[test]
fn a_pull_is_granted() {
    let grant = granted(&["repository:zone/server:pull"]);
    assert_eq!(grant.access, [pull("zone/server")]);
    assert_eq!(grant.dropped, 0);
    assert_eq!(grant.granted(), "zone/server:pull");
}

#[test]
fn push_alone_grants_nothing() {
    let grant = granted(&["repository:zone/server:push"]);
    assert!(grant.access.is_empty());
    assert_eq!(grant.dropped, 1);
    assert_eq!(grant.granted(), "");
}

#[test]
fn pull_and_push_grants_only_pull() {
    let grant = granted(&["repository:zone/server:pull,push"]);
    assert_eq!(grant.access, [pull("zone/server")]);
    assert_eq!(grant.dropped, 1);
}

#[test]
fn pull_push_and_delete_grants_only_pull() {
    let grant = granted(&["repository:zone/server:pull,push,delete"]);
    assert_eq!(grant.access, [pull("zone/server")]);
    assert_eq!(grant.dropped, 2);
}

#[test]
fn a_wildcard_is_dropped() {
    let grant = granted(&["repository:zone/server:*"]);
    assert!(grant.access.is_empty());
    assert_eq!(grant.dropped, 1);
}

#[test]
fn a_repository_outside_the_list_is_dropped() {
    let grant = granted(&[
        "repository:zone/runner:pull",
        "repository:library/traefik:pull",
        "repository:registry.bitecode.ltd/zone/server:pull",
        "repository:zone/server-extra:pull",
        "repository:Zone/server:pull",
    ]);
    assert!(grant.access.is_empty());
    assert_eq!(grant.dropped, 5);
}

#[test]
fn the_catalog_is_dropped() {
    let grant = granted(&["registry:catalog:*"]);
    assert!(grant.access.is_empty());
    assert_eq!(grant.dropped, 1);
}

#[test]
fn a_resource_class_is_dropped() {
    let grant = granted(&["repository(plugin):zone/server:pull"]);
    assert!(grant.access.is_empty());
    assert_eq!(grant.dropped, 1);
}

#[test]
fn duplicate_requests_merge() {
    let grant = granted(&[
        "repository:zone/server:pull repository:zone/server:pull,push",
        "repository:zone/server:pull,push",
        "repository:zone/manager:pull",
    ]);
    assert_eq!(grant.access, [pull("zone/server"), pull("zone/manager")]);
    assert_eq!(grant.dropped, 1);
    assert_eq!(grant.granted(), "zone/server:pull zone/manager:pull");
}

#[test]
fn space_separated_scopes_are_each_granted() {
    let grant = granted(&["repository:zone/server:pull repository:zone/litellm:pull"]);
    assert_eq!(grant.access, [pull("zone/server"), pull("zone/litellm")]);
    assert_eq!(grant.dropped, 0);
}

#[test]
fn no_scope_grants_nothing_and_drops_nothing() {
    assert_eq!(granted(&[]), Grant::default());
}

#[test]
fn grants_follow_the_configured_list() {
    let repositories = Repositories::parse("zone/runner").unwrap();
    let grant = grant(
        &scopes(&["repository:zone/runner:pull repository:zone/server:pull"]),
        &repositories,
    );
    assert_eq!(grant.access, [pull("zone/runner")]);
    assert_eq!(grant.dropped, 1);
}

#[test]
fn the_default_repositories_are_the_lts_images() {
    assert_eq!(
        Repositories::default().iter().collect::<Vec<_>>(),
        DEFAULT_REPOSITORIES
    );
    assert_eq!(
        DEFAULT_REPOSITORIES,
        ["zone/server", "zone/manager", "zone/litellm"]
    );
    assert!(Repositories::default().contains("zone/server"));
    assert!(!Repositories::default().contains("zone/serve"));
}

#[test]
fn parses_a_repository_list() {
    let repositories = Repositories::parse(" zone/server ,, zone/manager,").unwrap();
    assert_eq!(
        repositories.iter().collect::<Vec<_>>(),
        ["zone/server", "zone/manager"]
    );
}

#[test]
fn refuses_a_bad_repository_list() {
    assert_eq!(Repositories::parse(""), Err(RepositoriesError::Empty));
    assert_eq!(Repositories::parse(" , "), Err(RepositoriesError::Empty));
    assert_eq!(
        Repositories::parse("zone/server,zone/server"),
        Err(RepositoriesError::Duplicate {
            entry: "zone/server".into()
        })
    );
    for entry in [
        "Zone/server",
        "zone/",
        "localhost:5000/zone/server",
        "zone server",
    ] {
        assert_eq!(
            Repositories::parse(entry),
            Err(RepositoriesError::Invalid {
                entry: entry.into()
            }),
            "{entry}"
        );
    }
}

#[test]
fn access_serializes_in_the_contract_shape() {
    assert_eq!(
        serde_json::to_value(pull("zone/server")).unwrap(),
        json!({"type": "repository", "name": "zone/server", "actions": ["pull"]})
    );
}
