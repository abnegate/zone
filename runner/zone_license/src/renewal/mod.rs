mod reason;
mod verdict;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use zone_enterprise::License;

use crate::license_id;
use crate::polar::{OrderSubscription, SubscriptionStatus};
use crate::terms::Terms;

pub use reason::{ENDED, MISSING, PRODUCT, Reason, SETTLING, STATUS};
pub use verdict::Verdict;

pub const SETTLEMENT: TimeDelta = TimeDelta::days(3);

/// A key is renewed only when the subscription's period ends later than it,
/// or at the same time with other seats, so Zone never receives a key that is
/// no improvement on the one it holds.
pub fn decide(
    license: &License,
    found: Option<&OrderSubscription>,
    products: &[String],
    now: DateTime<Utc>,
) -> Verdict {
    let Some(subscription) = found else {
        return Verdict::Ended(Reason::Missing);
    };
    if subscription.ended_at.is_some() {
        return Verdict::Ended(Reason::Ended);
    }
    if !listed(subscription, products) {
        return Verdict::Ended(Reason::Product);
    }
    match subscription.status {
        SubscriptionStatus::Active => {}
        SubscriptionStatus::Trialing => return Verdict::Current,
        _ => return Verdict::Ended(Reason::Status),
    }
    let Some(seats) = seats(subscription) else {
        return Verdict::Misconfigured;
    };
    let expires_at = subscription.current_period_end.trunc_subsecs(0);
    if !newer(license, expires_at, seats) {
        return Verdict::Current;
    }
    let issued_at = subscription.current_period_start.trunc_subsecs(0);
    if settling(license, issued_at, now) {
        return Verdict::Settling;
    }
    let id = license.subscription().unwrap_or(&subscription.id);
    Verdict::Renew(Terms {
        id: license_id::derive(id, expires_at, seats),
        customer: license.customer().to_owned(),
        seats,
        issued_at,
        expires_at,
        subscription: Some(id.to_owned()),
    })
}

fn listed(subscription: &OrderSubscription, products: &[String]) -> bool {
    subscription
        .product_id
        .as_deref()
        .is_some_and(|product| products.iter().any(|listed| listed == product))
}

fn seats(subscription: &OrderSubscription) -> Option<u32> {
    subscription
        .seats
        .and_then(|seats| u32::try_from(seats).ok())
        .filter(|seats| *seats >= 1)
}

fn newer(license: &License, expires_at: DateTime<Utc>, seats: u32) -> bool {
    expires_at > license.expires_at()
        || (expires_at == license.expires_at() && seats != license.seats())
}

fn settling(license: &License, issued_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    issued_at >= license.expires_at()
        && issued_at
            .checked_add_signed(SETTLEMENT)
            .is_none_or(|settled| now < settled)
}

#[cfg(test)]
mod tests;
