use chrono::{DateTime, TimeDelta, Utc};
use zone_enterprise::License;

pub const WINDOW: TimeDelta = TimeDelta::days(14);

pub fn due(license: &License, now: DateTime<Utc>) -> bool {
    license.subscription().is_some() && license.expires_at() - now <= WINDOW
}

#[cfg(test)]
mod tests;
