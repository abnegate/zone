use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU32;
use std::time::Duration;

use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::TimeDelta;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use zone_enterprise::{GRACE_DAYS, Payload, Plan, TrustedKey};

use super::*;
use crate::logging;
use crate::private_key::PrivateKey;

const KEY_ID: &str = "test-gate-1";
const SEED: [u8; 32] = [9; 32];
const OTHER_SEED: [u8; 32] = [10; 32];
const CUSTOMER: &str = "Acme Gate Ltd";
const LICENSE: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_1e01);
const OTHER: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_1e02);
const SUBSCRIPTION: Uuid = Uuid::from_u128(0xb7e4_c2a1_3d5f_4e6a_9b8c_0d1e_2f3a_4b5c);

fn issued() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
        .unwrap()
        .to_utc()
}

fn expires() -> DateTime<Utc> {
    issued() + TimeDelta::days(365)
}

fn grace_ends() -> DateTime<Utc> {
    expires() + TimeDelta::days(GRACE_DAYS)
}

fn active() -> DateTime<Utc> {
    issued()
}

fn in_grace() -> DateTime<Utc> {
    expires()
}

fn last_second_of_grace() -> DateTime<Utc> {
    grace_ends() - TimeDelta::seconds(1)
}

fn second_after_grace() -> DateTime<Utc> {
    grace_ends() + TimeDelta::seconds(1)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new(SEED).public()).unwrap()]).unwrap()
}

fn payload(plan: Option<Plan>) -> Payload {
    Payload {
        id: LICENSE,
        customer: CUSTOMER.to_owned(),
        seats: 10,
        issued_at: issued(),
        expires_at: expires(),
        key_id: KEY_ID.to_owned(),
        subscription: None,
        plan,
    }
}

fn revoking(licenses: &[Uuid], subscriptions: &[Uuid]) -> Gate {
    let revoked = Revoked::new(
        licenses.iter().copied().collect(),
        subscriptions.iter().copied().collect(),
    );
    Gate::new(keyring(), revoked, TrustedProxies::default()).with_clock(active)
}

fn subscribed(id: Uuid, subscription: &str) -> String {
    let payload = Payload {
        id,
        subscription: Some(subscription.to_owned()),
        ..payload(Some(Plan::Enterprise))
    };
    signed(&payload, SEED)
}

fn signed(payload: &Payload, seed: [u8; 32]) -> String {
    zone_enterprise::sign(payload, &PrivateKey::new(seed).into_signing_key())
}

fn key(plan: Plan) -> String {
    signed(&payload(Some(plan)), SEED)
}

fn gate() -> Gate {
    Gate::new(keyring(), Revoked::default(), TrustedProxies::default()).with_clock(active)
}

fn authorization(value: &[u8]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_bytes(value).unwrap());
    headers
}

fn basic(user: &str, password: &str) -> HeaderMap {
    let encoded = STANDARD.encode(format!("{user}:{password}"));
    authorization(format!("Basic {encoded}").as_bytes())
}

fn presented(key: &str) -> HeaderMap {
    basic(&LICENSE.to_string(), key)
}

fn refused(gate: &Gate, headers: &HeaderMap) -> Denial {
    gate.admit(headers).expect_err("refused")
}

async fn body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn peer(address: &str) -> Option<SocketAddr> {
    Some(SocketAddr::new(address.parse().unwrap(), 41_000))
}

fn forwarded(values: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for value in values {
        headers.append(FORWARDED_FOR, HeaderValue::from_str(value).unwrap());
    }
    headers
}

fn proxies(list: &str) -> TrustedProxies {
    TrustedProxies::parse(list).unwrap()
}

fn address(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn limits(address: u32, license: u32) -> Limits {
    Limits {
        address: NonZeroU32::new(address).unwrap(),
        license: NonZeroU32::new(license).unwrap(),
    }
}

#[test]
fn each_refusal_has_a_fixed_wire_name() {
    assert_eq!(
        Refusal::ALL.map(Refusal::as_str),
        [
            "anonymous",
            "malformed",
            "unverified",
            "mismatched",
            "revoked",
            "ineligible",
            "lapsed",
        ]
    );
}

#[test]
fn each_refusal_answers_its_contracted_status() {
    assert_eq!(
        Refusal::ALL.map(|refusal| refusal.status().as_u16()),
        [401, 401, 401, 401, 401, 403, 403]
    );
}

#[test]
fn a_refusal_displays_its_wire_name() {
    for refusal in Refusal::ALL {
        assert_eq!(refusal.to_string(), refusal.as_str());
    }
}

#[tokio::test]
async fn an_unauthorized_refusal_challenges_for_basic_credentials() {
    for refusal in Refusal::ALL
        .into_iter()
        .filter(|refusal| refusal.status() == StatusCode::UNAUTHORIZED)
    {
        let response = refusal.respond();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{refusal}");
        assert_eq!(
            response.headers().get(WWW_AUTHENTICATE).unwrap(),
            r#"Basic realm="Zone license", charset="UTF-8""#,
            "{refusal}"
        );
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json",
            "{refusal}"
        );
        assert_eq!(
            body(response).await,
            json!({ "errors": [{ "code": "UNAUTHORIZED", "message": refusal.message() }] }),
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn a_forbidden_refusal_is_denied_without_a_challenge() {
    for refusal in [Refusal::Ineligible, Refusal::Lapsed] {
        let response = refusal.respond();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{refusal}");
        assert!(
            response.headers().get(WWW_AUTHENTICATE).is_none(),
            "{refusal}"
        );
        assert_eq!(
            body(response).await,
            json!({ "errors": [{ "code": "DENIED", "message": refusal.message() }] }),
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn a_denial_responds_as_its_refusal() {
    let denial = refused(&gate(), &HeaderMap::new());
    let response = denial.respond();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        body(response).await,
        json!({ "errors": [{ "code": "UNAUTHORIZED", "message": Refusal::Anonymous.message() }] })
    );
}

#[test]
fn no_credentials_are_anonymous() {
    let denial = refused(&gate(), &HeaderMap::new());
    assert_eq!(denial, Denial::new(Refusal::Anonymous));
}

#[test]
fn another_scheme_is_malformed() {
    for value in ["Bearer abc", "Basicx Zm9vOmJhcg==", "Digest username=\"x\""] {
        let denial = refused(&gate(), &authorization(value.as_bytes()));
        assert_eq!(denial.refusal, Refusal::Malformed, "{value}");
    }
}

#[test]
fn bad_base64_is_malformed() {
    for value in [
        "Basic !!!",
        "Basic Zm9vOmJhcg",
        "Basic Zm9v_mJhcg==",
        "Basic",
        "Basic ",
        "",
    ] {
        let denial = refused(&gate(), &authorization(value.as_bytes()));
        assert_eq!(denial.refusal, Refusal::Malformed, "{value:?}");
    }
}

#[test]
fn credentials_without_a_colon_are_malformed() {
    let encoded = STANDARD.encode(LICENSE.to_string());
    let denial = refused(
        &gate(),
        &authorization(format!("Basic {encoded}").as_bytes()),
    );
    assert_eq!(denial.refusal, Refusal::Malformed);
}

#[test]
fn credentials_that_are_not_utf8_are_malformed() {
    let encoded = STANDARD.encode([0xff, 0xfe, b':', 0xc3, 0x28]);
    let denial = refused(
        &gate(),
        &authorization(format!("Basic {encoded}").as_bytes()),
    );
    assert_eq!(denial.refusal, Refusal::Malformed);
}

#[test]
fn a_header_value_that_is_not_utf8_is_malformed() {
    let denial = refused(&gate(), &authorization(b"Basic \xff\xfe"));
    assert_eq!(denial.refusal, Refusal::Malformed);
}

#[test]
fn two_authorization_headers_are_malformed() {
    let mut headers = presented(&key(Plan::Enterprise));
    headers.append(
        AUTHORIZATION,
        HeaderValue::from_static("Basic Zm9vOmJhcg=="),
    );
    assert_eq!(refused(&gate(), &headers).refusal, Refusal::Malformed);
}

#[test]
fn the_scheme_is_case_insensitive() {
    let encoded = STANDARD.encode(format!("{LICENSE}:{}", key(Plan::Enterprise)));
    for scheme in ["basic", "BASIC", "bAsIc"] {
        let headers = authorization(format!("{scheme} {encoded}").as_bytes());
        assert_eq!(gate().admit(&headers).unwrap().id(), LICENSE, "{scheme}");
    }
}

#[test]
fn the_password_is_split_at_the_first_colon() {
    let credentials = Credentials::from_headers(&basic("user", "pass:word:")).unwrap();
    assert_eq!(credentials.user(), "user");
    assert_eq!(credentials.password(), "pass:word:");
}

#[test]
fn credentials_never_show_the_password() {
    let credentials = Credentials::from_headers(&basic("user", "hunter2-gate")).unwrap();
    let shown = format!("{credentials:?} {credentials:#?}");
    assert!(!shown.contains("hunter2-gate"), "{shown}");
    assert!(shown.contains("[redacted]"), "{shown}");
}

#[test]
fn a_garbage_key_is_unverified_as_malformed() {
    let denial = refused(&gate(), &presented("not-a-license-key"));
    assert_eq!(denial, Denial::unverified("license_malformed"));
    assert_eq!(denial.license, None);
}

#[test]
fn a_key_from_an_untrusted_key_id_is_unverified_as_unknown() {
    let foreign = signed(
        &Payload {
            key_id: "test-gate-2".to_owned(),
            ..payload(Some(Plan::Enterprise))
        },
        SEED,
    );
    let denial = refused(&gate(), &presented(&foreign));
    assert_eq!(denial, Denial::unverified("license_unknown_key"));
}

#[test]
fn a_tampered_signature_is_unverified() {
    let forged = signed(&payload(Some(Plan::Enterprise)), OTHER_SEED);
    let denial = refused(&gate(), &presented(&forged));
    assert_eq!(denial, Denial::unverified("license_signature_invalid"));
}

#[test]
fn another_license_id_as_the_user_name_is_mismatched() {
    for user in [OTHER.to_string(), "admin".to_owned(), String::new()] {
        let denial = refused(&gate(), &basic(&user, &key(Plan::Enterprise)));
        assert_eq!(denial.refusal, Refusal::Mismatched, "{user:?}");
        assert_eq!(denial.license, Some(LICENSE), "{user:?}");
    }
}

#[test]
fn an_upper_case_license_id_is_accepted() {
    let user = LICENSE.to_string().to_uppercase();
    let license = gate().admit(&basic(&user, &key(Plan::Enterprise))).unwrap();
    assert_eq!(license.id(), LICENSE);
}

#[test]
fn the_key_is_trimmed() {
    let wrapped = format!("  {}\n", key(Plan::Enterprise));
    assert_eq!(gate().admit(&presented(&wrapped)).unwrap().id(), LICENSE);
}

#[test]
fn a_revoked_license_is_refused() {
    let gate = revoking(&[OTHER, LICENSE], &[]);
    let denial = refused(&gate, &presented(&key(Plan::Enterprise)));
    assert_eq!(
        denial,
        Denial {
            refusal: Refusal::Revoked,
            license: Some(LICENSE),
            plan: Some(Plan::Enterprise),
            status: Some(Status::Active),
            error: None,
        }
    );
}

#[test]
fn revocation_is_checked_before_the_user_name() {
    let gate = revoking(&[LICENSE], &[]);
    let denial = refused(&gate, &basic("admin", &key(Plan::Enterprise)));
    assert_eq!(denial.refusal, Refusal::Revoked);
}

#[test]
fn a_revoked_subscription_refuses_every_key_on_it_whatever_its_id() {
    let gate = revoking(&[], &[SUBSCRIPTION]);
    let subscription = SUBSCRIPTION.to_string().to_uppercase();
    for id in [LICENSE, OTHER] {
        let headers = basic(&id.to_string(), &subscribed(id, &subscription));
        let denial = refused(&gate, &headers);
        assert_eq!(denial.refusal, Refusal::Revoked, "{id}");
        assert_eq!(denial.license, Some(id), "{id}");
    }
}

#[test]
fn a_revoked_license_id_leaves_the_other_keys_on_its_subscription_admitted() {
    let gate = revoking(&[LICENSE], &[]);
    let subscription = SUBSCRIPTION.to_string();
    let denial = refused(&gate, &presented(&subscribed(LICENSE, &subscription)));
    assert_eq!(denial.refusal, Refusal::Revoked);
    let renewed = basic(&OTHER.to_string(), &subscribed(OTHER, &subscription));
    assert_eq!(gate.admit(&renewed).unwrap().id(), OTHER);
}

#[test]
fn a_key_without_a_subscription_or_on_another_one_is_not_revoked_by_subscription() {
    let gate = revoking(&[], &[SUBSCRIPTION]);
    assert_eq!(
        gate.admit(&presented(&key(Plan::Enterprise))).unwrap().id(),
        LICENSE
    );
    let another = subscribed(LICENSE, "c8f5d3b2-4e6a-4f7b-8c9d-1e2f3a4b5c6d");
    assert_eq!(gate.admit(&presented(&another)).unwrap().id(), LICENSE);
    let unparsable = subscribed(LICENSE, "sub_01HZX0Q7M8");
    assert_eq!(gate.admit(&presented(&unparsable)).unwrap().id(), LICENSE);
}

#[test]
fn a_team_license_is_ineligible() {
    let denial = refused(&gate(), &presented(&key(Plan::Team)));
    assert_eq!(denial.refusal, Refusal::Ineligible);
    assert_eq!(denial.plan, Some(Plan::Team));
    assert_eq!(denial.license, Some(LICENSE));
}

#[test]
fn a_lapsed_team_license_is_ineligible_because_the_plan_is_checked_first() {
    let gate = gate().with_clock(second_after_grace);
    let denial = refused(&gate, &presented(&key(Plan::Team)));
    assert_eq!(denial.refusal, Refusal::Ineligible);
    assert_eq!(denial.status, Some(Status::Lapsed));
}

#[test]
fn an_enterprise_license_is_admitted_while_active_or_in_grace() {
    let clocks: [fn() -> DateTime<Utc>; 3] = [active, in_grace, last_second_of_grace];
    for clock in clocks {
        let gate = gate().with_clock(clock);
        let license = gate.admit(&presented(&key(Plan::Enterprise))).unwrap();
        assert_eq!(license.id(), LICENSE);
        assert!(Feature::Distribution.entitled(Some(&license), clock()));
    }
}

#[test]
fn an_enterprise_license_one_second_after_grace_is_lapsed() {
    let gate = gate().with_clock(second_after_grace);
    let denial = refused(&gate, &presented(&key(Plan::Enterprise)));
    assert_eq!(
        denial,
        Denial {
            refusal: Refusal::Lapsed,
            license: Some(LICENSE),
            plan: Some(Plan::Enterprise),
            status: Some(Status::Lapsed),
            error: None,
        }
    );
}

#[test]
fn a_key_without_a_plan_is_enterprise_and_admitted() {
    let unplanned = signed(&payload(None), SEED);
    let license = gate().admit(&presented(&unplanned)).unwrap();
    assert_eq!(license.plan(), Plan::Enterprise);
}

#[test]
fn a_denial_never_carries_the_key_or_the_customer() {
    let key = key(Plan::Team);
    let denial = refused(&gate(), &presented(&key));
    let shown = format!("{denial:?}");
    assert!(!shown.contains(&key), "{shown}");
    assert!(!shown.contains(CUSTOMER), "{shown}");
}

#[test]
fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
    let headers = forwarded(&["203.0.113.9"]);
    assert_eq!(
        client_address(peer("198.51.100.4"), &headers, &proxies("172.18.0.0/16")),
        Some(address("198.51.100.4"))
    );
    assert_eq!(
        client_address(peer("198.51.100.4"), &headers, &TrustedProxies::default()),
        Some(address("198.51.100.4"))
    );
}

#[test]
fn a_trusted_peer_forwards_the_rightmost_entry() {
    let headers = forwarded(&["203.0.113.9"]);
    assert_eq!(
        client_address(peer("172.18.0.2"), &headers, &proxies("172.18.0.0/16")),
        Some(address("203.0.113.9"))
    );
}

#[test]
fn a_spoofed_leftmost_entry_is_ignored() {
    let proxies = proxies("172.18.0.0/16");
    let single = forwarded(&["10.9.9.9, 192.0.2.1,203.0.113.9"]);
    assert_eq!(
        client_address(peer("172.18.0.2"), &single, &proxies),
        Some(address("203.0.113.9"))
    );
    let repeated = forwarded(&["10.9.9.9", "192.0.2.1, 203.0.113.9"]);
    assert_eq!(
        client_address(peer("172.18.0.2"), &repeated, &proxies),
        Some(address("203.0.113.9"))
    );
}

#[test]
fn a_trusted_peer_without_a_usable_entry_is_the_client() {
    let proxies = proxies("172.18.0.0/16");
    for headers in [
        HeaderMap::new(),
        forwarded(&["203.0.113.9, unknown"]),
        forwarded(&["203.0.113.9,"]),
    ] {
        assert_eq!(
            client_address(peer("172.18.0.2"), &headers, &proxies),
            Some(address("172.18.0.2")),
            "{headers:?}"
        );
    }
}

#[test]
fn a_forwarded_entry_with_a_port_or_a_mapped_address_is_read() {
    let proxies = proxies("172.18.0.0/16");
    for (entry, expected) in [
        ("203.0.113.9:5123", "203.0.113.9"),
        ("[2001:db8::7]:443", "2001:db8::7"),
        ("2001:db8::7", "2001:db8::7"),
        ("::ffff:203.0.113.9", "203.0.113.9"),
    ] {
        assert_eq!(
            client_address(peer("172.18.0.2"), &forwarded(&[entry]), &proxies),
            Some(address(expected)),
            "{entry}"
        );
    }
}

#[test]
fn a_mapped_peer_is_matched_as_ipv4() {
    let headers = forwarded(&["203.0.113.9"]);
    assert_eq!(
        client_address(
            peer("::ffff:172.18.0.2"),
            &headers,
            &proxies("172.18.0.0/16")
        ),
        Some(address("203.0.113.9"))
    );
    assert_eq!(
        client_address(
            peer("::ffff:198.51.100.4"),
            &headers,
            &proxies("172.18.0.0/16")
        ),
        Some(address("198.51.100.4"))
    );
}

#[test]
fn no_peer_has_no_client_address() {
    assert_eq!(
        client_address(None, &forwarded(&["203.0.113.9"]), &proxies("0.0.0.0/0")),
        None
    );
}

#[test]
fn the_gate_resolves_the_client_through_its_proxies() {
    let gate = Gate::new(keyring(), Revoked::default(), proxies("172.18.0.0/16"));
    assert_eq!(gate.proxies(), &proxies("172.18.0.0/16"));
    assert_eq!(
        gate.client(peer("172.18.0.2"), &forwarded(&["203.0.113.9"])),
        Ok(address("203.0.113.9"))
    );
}

#[test]
fn the_gate_cannot_address_a_request_without_a_peer() {
    assert_eq!(
        gate().client(None, &forwarded(&["203.0.113.9"])),
        Err(Unaddressable)
    );
}

#[tokio::test]
async fn an_unaddressable_request_is_a_server_error_in_the_registry_shape() {
    let response = Unaddressable.respond();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
    assert_eq!(
        body(response).await,
        json!({ "errors": [{ "code": "UNKNOWN", "message": "The client address can't be read." }] })
    );
    assert_eq!(UNADDRESSABLE, "unaddressable");
}

#[test]
fn trusted_proxies_accept_ranges_and_bare_addresses() {
    let proxies = proxies(" 172.18.0.1/16 ,, 10.0.0.5, 2001:db8::/32 ");
    assert_eq!(proxies.len(), 3);
    assert_eq!(
        proxies.to_string(),
        "172.18.0.0/16,10.0.0.5/32,2001:db8::/32"
    );
    assert!(proxies.trusts(address("172.18.255.255")));
    assert!(proxies.trusts(address("10.0.0.5")));
    assert!(!proxies.trusts(address("10.0.0.6")));
    assert!(proxies.trusts(address("2001:db8:1::1")));
    assert!(!proxies.trusts(address("2001:db9::1")));
}

#[test]
fn a_bad_proxy_entry_is_named() {
    assert_eq!(
        TrustedProxies::parse("172.18.0.0/16,caddy"),
        Err(Unparsable {
            entry: "caddy".to_owned(),
            expected: "a CIDR range or an IP address, such as 172.18.0.0/16",
        })
    );
}

#[test]
fn revoked_ids_are_read_in_any_case() {
    let revoked = Revoked::parse_licenses(&format!(
        " {} ,,{OTHER}",
        LICENSE.to_string().to_uppercase()
    ))
    .unwrap();
    assert_eq!(revoked, HashSet::from([LICENSE, OTHER]));
    let subscriptions =
        Revoked::parse_subscriptions(&SUBSCRIPTION.to_string().to_uppercase()).unwrap();
    assert_eq!(subscriptions, HashSet::from([SUBSCRIPTION]));
}

#[test]
fn a_bad_revoked_entry_is_named() {
    assert_eq!(
        Revoked::parse_licenses("not-a-uuid"),
        Err(Unparsable {
            entry: "not-a-uuid".to_owned(),
            expected: "a license ID, which is a UUID",
        })
    );
    assert_eq!(
        Revoked::parse_subscriptions("sub_01HZX0Q7M8"),
        Err(Unparsable {
            entry: "sub_01HZX0Q7M8".to_owned(),
            expected: "a subscription ID, which is a UUID",
        })
    );
}

#[test]
fn revoked_is_empty_only_without_licenses_and_subscriptions() {
    assert!(Revoked::default().is_empty());
    assert!(!revoking(&[], &[SUBSCRIPTION]).revoked.is_empty());
    assert!(!revoking(&[LICENSE], &[]).revoked.is_empty());
}

#[test]
fn ipv6_addresses_in_one_slash_64_share_a_key() {
    assert_eq!(
        ThrottleKey::from(address("2001:db8:1:2::1")),
        ThrottleKey::from(address("2001:db8:1:2:ffff:ffff:ffff:ffff"))
    );
    assert_eq!(
        ThrottleKey::from(address("2001:db8:1:2:abcd::9")).address(),
        address("2001:db8:1:2::")
    );
}

#[test]
fn ipv6_addresses_in_different_slash_64s_do_not() {
    assert_ne!(
        ThrottleKey::from(address("2001:db8:1:2::1")),
        ThrottleKey::from(address("2001:db8:1:3::1"))
    );
}

#[test]
fn an_ipv4_mapped_address_is_keyed_as_ipv4() {
    let mapped = IpAddr::V6(Ipv4Addr::new(203, 0, 113, 9).to_ipv6_mapped());
    assert_eq!(
        ThrottleKey::from(mapped),
        ThrottleKey::from(address("203.0.113.9"))
    );
    assert_eq!(ThrottleKey::from(mapped).address(), address("203.0.113.9"));
}

#[test]
fn ipv4_addresses_are_keyed_individually() {
    assert_ne!(
        ThrottleKey::from(address("203.0.113.9")),
        ThrottleKey::from(address("203.0.113.10"))
    );
    assert_ne!(
        ThrottleKey::from(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
        ThrottleKey::from(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    );
}

#[test]
fn the_default_limits_are_contracted() {
    assert_eq!(Limits::default(), limits(120, 240));
    assert_eq!(WINDOW, Duration::from_secs(60));
}

#[test]
fn the_address_limit_trips_without_touching_the_license_limit() {
    let throttle = Throttle::new(limits(2, 5));
    let client = address("203.0.113.9");
    assert_eq!(throttle.check_address(client), Ok(()));
    assert_eq!(throttle.check_address(client), Ok(()));
    let limited = throttle.check_address(client).expect_err("over the limit");
    assert!(limited.retry_after() > 0);
    assert!(limited.retry_after() <= WINDOW.as_secs());
    assert_eq!(throttle.check_address(address("203.0.113.10")), Ok(()));
    assert_eq!(throttle.check_license(LICENSE), Ok(()));
}

#[test]
fn the_license_limit_trips_without_touching_the_address_limit() {
    let throttle = Throttle::new(limits(5, 2));
    assert_eq!(throttle.check_license(LICENSE), Ok(()));
    assert_eq!(throttle.check_license(LICENSE), Ok(()));
    let limited = throttle.check_license(LICENSE).expect_err("over the limit");
    assert!(limited.retry_after() > 0);
    assert_eq!(throttle.check_license(OTHER), Ok(()));
    assert_eq!(throttle.check_address(address("203.0.113.9")), Ok(()));
}

#[test]
fn one_slash_64_shares_one_address_allowance() {
    let throttle = Throttle::new(limits(1, 5));
    assert_eq!(throttle.check_address(address("2001:db8:1:2::1")), Ok(()));
    assert!(throttle.check_address(address("2001:db8:1:2::2")).is_err());
    assert_eq!(throttle.check_address(address("2001:db8:1:3::1")), Ok(()));
}

#[test]
fn a_clone_shares_its_counts() {
    let throttle = Throttle::new(limits(1, 1));
    let clone = throttle.clone();
    assert_eq!(throttle.check_license(LICENSE), Ok(()));
    assert!(clone.check_license(LICENSE).is_err());
}

#[test]
fn retry_after_rounds_up_to_at_least_a_second() {
    assert_eq!(Limited::after(Duration::ZERO).retry_after(), 1);
    assert_eq!(Limited::after(Duration::from_millis(1)).retry_after(), 1);
    assert_eq!(
        Limited::after(Duration::from_millis(1_001)).retry_after(),
        2
    );
    assert_eq!(Limited::after(Duration::from_secs(59)).retry_after(), 59);
}

#[tokio::test]
async fn a_limited_request_answers_429_with_retry_after() {
    let limited = Limited::after(Duration::from_millis(29_500));
    let response = limited.respond();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get(RETRY_AFTER).unwrap(), "30");
    assert_eq!(
        body(response).await,
        json!({ "errors": [{ "code": "TOOMANYREQUESTS", "message": limited::MESSAGE }] })
    );
}

#[tokio::test]
async fn the_cleanup_runs_until_it_is_aborted() {
    let throttle = Throttle::new(Limits::default());
    let cleanup = throttle.spawn_cleanup();
    tokio::task::yield_now().await;
    assert!(!cleanup.is_finished());
    cleanup.abort();
    assert!(cleanup.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn announce_logs_the_trusted_proxies() {
    let logged = logging::capture(tracing::Level::INFO, async {
        announce(&proxies("172.18.0.0/16,10.0.0.5"));
    })
    .await;
    assert!(logged.contains(" INFO "), "{logged}");
    assert!(
        logged.contains("proxies=172.18.0.0/16,10.0.0.5/32"),
        "{logged}"
    );
    assert!(!logged.contains("WARN"), "{logged}");
}

#[tokio::test]
async fn announce_warns_without_trusted_proxies() {
    let logged = logging::capture(tracing::Level::INFO, async {
        announce(&TrustedProxies::default());
    })
    .await;
    assert!(logged.contains(" WARN "), "{logged}");
    assert!(
        logged.contains("variable=\"ZONE_LICENSE_TRUSTED_PROXIES\""),
        "{logged}"
    );
}
