use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;
use zone_enterprise::{Keyring, TrustedKey};

use super::*;
use crate::private_key::PrivateKey;
use crate::signer::Signer;

const KEY_ID: &str = "test-1";
const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const CUSTOMER: &str = "Acme Ltd";

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn period_start() -> DateTime<Utc> {
    at("2026-11-18T11:59:58Z")
}

fn period_end() -> DateTime<Utc> {
    at("2027-11-18T11:59:58Z")
}

fn settled() -> DateTime<Utc> {
    period_start() + SETTLEMENT
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new([7; 32]), KEY_ID)
}

fn license(expires_at: DateTime<Utc>, seats: u32) -> License {
    signed(expires_at - TimeDelta::days(365), expires_at, seats)
}

fn signed(issued_at: DateTime<Utc>, expires_at: DateTime<Utc>, seats: u32) -> License {
    let signer = signer();
    let text = signer
        .sign(&Terms {
            id: Uuid::from_u128(1),
            customer: CUSTOMER.to_owned(),
            seats,
            issued_at,
            expires_at,
            subscription: Some(SUBSCRIPTION.to_owned()),
        })
        .unwrap();
    Keyring::new([TrustedKey::new(KEY_ID, signer.public()).unwrap()])
        .unwrap()
        .verify(&text)
        .unwrap()
}

fn expiring() -> License {
    license(period_start(), 5)
}

fn subscription() -> OrderSubscription {
    OrderSubscription {
        id: SUBSCRIPTION.to_owned(),
        status: SubscriptionStatus::Active,
        seats: Some(5),
        current_period_start: period_start(),
        current_period_end: period_end(),
        product_id: Some(PRODUCT.to_owned()),
        cancel_at_period_end: false,
        ended_at: None,
    }
}

fn products() -> Vec<String> {
    vec![PRODUCT.to_owned()]
}

fn verdict(license: &License, subscription: &OrderSubscription) -> Verdict {
    verdict_at(license, subscription, settled())
}

fn verdict_at(license: &License, subscription: &OrderSubscription, now: DateTime<Utc>) -> Verdict {
    decide(license, Some(subscription), &products(), now)
}

fn renewed(verdict: Verdict) -> Terms {
    match verdict {
        Verdict::Renew(terms) => terms,
        other => panic!("expected a renewal, got {other:?}"),
    }
}

#[test]
fn an_active_subscription_with_a_later_period_renews_for_that_period() {
    let terms = renewed(verdict(&expiring(), &subscription()));
    assert_eq!(
        terms,
        Terms {
            id: license_id::derive(SUBSCRIPTION, period_end(), 5),
            customer: CUSTOMER.to_owned(),
            seats: 5,
            issued_at: period_start(),
            expires_at: period_end(),
            subscription: Some(SUBSCRIPTION.to_owned()),
        }
    );
}

#[test]
fn the_renewed_id_matches_the_one_the_webhook_derives() {
    let first = renewed(verdict(&expiring(), &subscription()));
    let again = renewed(verdict(&license(period_start(), 9), &subscription()));
    assert_eq!(first.id, again.id);
}

#[test]
fn the_renewed_period_is_cut_to_whole_seconds() {
    let precise = OrderSubscription {
        current_period_start: at("2026-11-18T11:59:58.123456Z"),
        current_period_end: at("2027-11-18T11:59:58.654321Z"),
        ..subscription()
    };
    assert_eq!(
        renewed(verdict(&expiring(), &precise)),
        renewed(verdict(&expiring(), &subscription()))
    );
}

#[test]
fn a_seat_change_in_the_same_period_renews() {
    let terms = renewed(verdict(&license(period_end(), 3), &subscription()));
    assert_eq!(terms.seats, 5);
    assert_eq!(terms.expires_at, period_end());
    assert_eq!(terms.id, license_id::derive(SUBSCRIPTION, period_end(), 5));
}

#[test]
fn the_same_period_and_seats_is_current() {
    assert_eq!(
        verdict(&license(period_end(), 5), &subscription()),
        Verdict::Current
    );
}

#[test]
fn a_sub_second_difference_in_the_period_end_is_current() {
    let precise = OrderSubscription {
        current_period_end: at("2027-11-18T11:59:58.9Z"),
        ..subscription()
    };
    assert_eq!(
        verdict(&license(period_end(), 5), &precise),
        Verdict::Current
    );
}

#[test]
fn an_earlier_period_is_current_even_with_other_seats() {
    let later = license(period_end() + TimeDelta::days(30), 5);
    assert_eq!(verdict(&later, &subscription()), Verdict::Current);
    let fewer = license(period_end() + TimeDelta::days(30), 2);
    assert_eq!(verdict(&fewer, &subscription()), Verdict::Current);
}

#[test]
fn a_trialing_subscription_is_current() {
    let trialing = OrderSubscription {
        status: SubscriptionStatus::Trialing,
        ..subscription()
    };
    assert_eq!(verdict(&expiring(), &trialing), Verdict::Current);
}

#[test]
fn a_trialing_subscription_without_seats_is_still_current() {
    let trialing = OrderSubscription {
        status: SubscriptionStatus::Trialing,
        seats: None,
        ..subscription()
    };
    assert_eq!(verdict(&expiring(), &trialing), Verdict::Current);
}

#[test]
fn every_other_status_has_ended() {
    for status in [
        SubscriptionStatus::Incomplete,
        SubscriptionStatus::IncompleteExpired,
        SubscriptionStatus::PastDue,
        SubscriptionStatus::Canceled,
        SubscriptionStatus::Unpaid,
        SubscriptionStatus::Paused,
        SubscriptionStatus::Other,
    ] {
        let other = OrderSubscription {
            status,
            ..subscription()
        };
        assert_eq!(
            verdict(&expiring(), &other),
            Verdict::Ended(Reason::Status),
            "{status:?}"
        );
    }
}

#[test]
fn a_missing_subscription_has_ended() {
    assert_eq!(
        decide(&expiring(), None, &products(), settled()),
        Verdict::Ended(Reason::Missing)
    );
}

#[test]
fn a_subscription_with_an_end_has_ended_whatever_its_status() {
    let ended = OrderSubscription {
        ended_at: Some(at("2027-01-02T03:04:05Z")),
        ..subscription()
    };
    assert_eq!(verdict(&expiring(), &ended), Verdict::Ended(Reason::Ended));
}

#[test]
fn an_unlisted_or_missing_product_has_ended() {
    let other = OrderSubscription {
        product_id: Some("another-product".to_owned()),
        ..subscription()
    };
    assert_eq!(
        verdict(&expiring(), &other),
        Verdict::Ended(Reason::Product)
    );
    let none = OrderSubscription {
        product_id: None,
        ..subscription()
    };
    assert_eq!(verdict(&expiring(), &none), Verdict::Ended(Reason::Product));
    assert_eq!(
        decide(&expiring(), Some(&subscription()), &[], settled()),
        Verdict::Ended(Reason::Product)
    );
}

#[test]
fn null_zero_negative_or_oversized_seats_are_misconfigured() {
    for seats in [None, Some(0), Some(-2), Some(5_000_000_000_i64)] {
        let broken = OrderSubscription {
            seats,
            ..subscription()
        };
        assert_eq!(
            verdict(&expiring(), &broken),
            Verdict::Misconfigured,
            "{seats:?}"
        );
    }
}

#[test]
fn a_scheduled_cancellation_still_renews_to_the_current_end() {
    let cancelling = OrderSubscription {
        cancel_at_period_end: true,
        ..subscription()
    };
    let terms = renewed(verdict(&expiring(), &cancelling));
    assert_eq!(terms.expires_at, period_end());
}

#[test]
fn a_new_period_is_settling_until_polar_has_had_time_to_collect() {
    for now in [
        period_start(),
        period_start() + TimeDelta::hours(1),
        settled() - TimeDelta::seconds(1),
    ] {
        assert_eq!(
            verdict_at(&expiring(), &subscription(), now),
            Verdict::Settling,
            "{now}"
        );
    }
}

#[test]
fn a_new_period_renews_once_settled() {
    for now in [settled(), settled() + TimeDelta::days(30)] {
        let terms = renewed(verdict_at(&expiring(), &subscription(), now));
        assert_eq!(terms.expires_at, period_end(), "{now}");
    }
}

#[test]
fn a_new_period_settles_from_its_start_in_whole_seconds() {
    let precise = OrderSubscription {
        current_period_start: at("2026-11-18T11:59:58.9Z"),
        ..subscription()
    };
    assert!(matches!(
        verdict_at(&expiring(), &precise, settled()),
        Verdict::Renew(_)
    ));
}

#[test]
fn a_period_starting_after_the_key_expired_is_still_settling() {
    let lapsed = license(period_start() - TimeDelta::days(20), 5);
    assert_eq!(
        verdict_at(&lapsed, &subscription(), period_start()),
        Verdict::Settling
    );
}

#[test]
fn a_seat_change_in_the_same_period_is_not_held() {
    let terms = renewed(verdict_at(
        &license(period_end(), 3),
        &subscription(),
        period_start() + TimeDelta::hours(1),
    ));
    assert_eq!(terms.seats, 5);
}

#[test]
fn a_seat_change_on_a_key_issued_in_whole_seconds_for_the_period_is_not_held() {
    let precise = signed(at("2026-11-18T11:59:58.4Z"), period_end(), 3);
    let terms = renewed(verdict_at(&precise, &subscription(), period_start()));
    assert_eq!(terms.seats, 5);
}

#[test]
fn a_period_starting_inside_the_key_term_is_settling_then_renews() {
    let anchored = license(period_start() + TimeDelta::days(30), 5);
    for now in [period_start(), settled() - TimeDelta::seconds(1)] {
        assert_eq!(
            verdict_at(&anchored, &subscription(), now),
            Verdict::Settling,
            "{now}"
        );
    }
    let terms = renewed(verdict_at(&anchored, &subscription(), settled()));
    assert_eq!(terms.expires_at, period_end());
}

#[test]
fn a_seat_change_in_a_period_the_key_already_reaches_into_is_settling() {
    let partway = license(period_start() + TimeDelta::days(1), 3);
    assert_eq!(
        verdict_at(&partway, &subscription(), period_start()),
        Verdict::Settling
    );
}

#[test]
fn a_converted_trial_is_settling_under_a_key_that_outlasts_the_trial() {
    let trial = signed(
        period_start() - TimeDelta::days(14),
        period_start() + TimeDelta::days(7),
        5,
    );
    assert_eq!(
        verdict_at(
            &trial,
            &subscription(),
            period_start() + TimeDelta::hours(1)
        ),
        Verdict::Settling
    );
}

#[test]
fn a_key_with_a_sub_second_expiry_is_settling() {
    let precise = signed(at("2025-11-18T11:59:58Z"), at("2026-11-18T11:59:58.5Z"), 5);
    assert_eq!(
        verdict_at(&precise, &subscription(), period_start()),
        Verdict::Settling
    );
}

#[test]
fn a_trialing_subscription_is_current_while_settling() {
    let trialing = OrderSubscription {
        status: SubscriptionStatus::Trialing,
        ..subscription()
    };
    assert_eq!(
        verdict_at(&expiring(), &trialing, period_start()),
        Verdict::Current
    );
}

#[test]
fn a_key_covering_the_period_is_current_while_settling() {
    assert_eq!(
        verdict_at(&license(period_end(), 5), &subscription(), period_start()),
        Verdict::Current
    );
}

#[test]
fn the_settlement_window_is_three_days() {
    assert_eq!(SETTLEMENT, TimeDelta::days(3));
}

#[test]
fn the_reasons_have_fixed_wire_names() {
    assert_eq!(Reason::Missing.as_str(), "missing");
    assert_eq!(Reason::Status.as_str(), "status");
    assert_eq!(Reason::Ended.as_str(), "ended");
    assert_eq!(Reason::Product.as_str(), "product");
    assert_eq!(Reason::Product.to_string(), super::PRODUCT);
    assert_eq!(SETTLING, "settling");
}
