use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};

use crate::sso::verifier::LEEWAY_SECONDS;

pub const LEEWAY: TimeDelta = TimeDelta::seconds(LEEWAY_SECONDS);

const UTC: char = 'Z';
const FRACTION: char = '.';
const FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const MAXIMUM_FRACTION_DIGITS: usize = 9;
const SEPARATORS: &[u8] = b"-:T";

pub fn parse(value: &str) -> Option<DateTime<Utc>> {
    let body = value.strip_suffix(UTC)?;
    let (seconds, fraction) = match body.split_once(FRACTION) {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (body, None),
    };
    if !seconds
        .bytes()
        .all(|byte| byte.is_ascii_digit() || SEPARATORS.contains(&byte))
    {
        return None;
    }
    let instant = NaiveDateTime::parse_from_str(seconds, FORMAT).ok()?;
    let nanoseconds = match fraction {
        Some(fraction) => nanoseconds(fraction)?,
        None => 0,
    };
    instant
        .checked_add_signed(TimeDelta::nanoseconds(nanoseconds))
        .map(|instant| instant.and_utc())
}

fn nanoseconds(fraction: &str) -> Option<i64> {
    if fraction.is_empty()
        || fraction.len() > MAXIMUM_FRACTION_DIGITS
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    format!("{fraction:0<MAXIMUM_FRACTION_DIGITS$}")
        .parse()
        .ok()
}
