use chrono::NaiveDate;
use serde_json::json;
use uuid::Uuid;
use zone_enterprise::spend::{Error, Range};

use super::exports::filename;
use super::query::Query;
use crate::db::spend::report::Filter;

const WORKSPACE: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001);
const USER: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0002);
const ORGANIZATION: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0003);

fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn today() -> NaiveDate {
    date("2026-11-21")
}

fn read(raw: &str) -> Result<Query, Error> {
    Query::read(Some(raw), today())
}

#[test]
fn no_query_covers_the_month_so_far() {
    for raw in [None, Some("")] {
        let query = Query::read(raw, today()).unwrap();
        assert_eq!(
            query.range,
            Range {
                from: date("2026-11-01"),
                to: today(),
            }
        );
        assert_eq!(query.workspace, None);
        assert_eq!(query.user, None);
    }
}

#[test]
fn the_range_and_filters_are_read_from_the_query() {
    let query = read(&format!(
        "from=2026-10-01&to=2026-10-31&workspace_id={WORKSPACE}&user_id={USER}"
    ))
    .unwrap();

    assert_eq!(query.range.from, date("2026-10-01"));
    assert_eq!(query.range.to, date("2026-10-31"));
    assert_eq!(query.workspace, Some(WORKSPACE));
    assert_eq!(query.user, Some(USER));
    assert_eq!(
        query.filter(ORGANIZATION),
        Filter {
            organization: ORGANIZATION,
            range: query.range,
            workspace: Some(WORKSPACE),
            user: Some(USER),
        }
    );
}

#[test]
fn a_single_day_and_unknown_parameters_are_accepted() {
    let query = read("from=2026-11-05&to=2026-11-05&page=2").unwrap();

    assert_eq!(query.range.from, query.range.to);
}

#[test]
fn every_unreadable_value_is_an_invalid_range() {
    for raw in [
        "from=2026-11-22&to=2026-11-21",
        "from=2026-13-01",
        "from=2026-11-1",
        "to=yesterday",
        "from=",
        "from=2026-11-01&from=2026-11-02",
        "to=2026-11-02&to=2026-11-02",
        "workspace_id=not-a-workspace",
        "workspace_id=",
        "user_id=42",
        &format!("user_id={USER}&user_id={USER}"),
    ] {
        assert_eq!(read(raw), Err(Error::RangeInvalid), "{raw}");
    }
}

#[test]
fn an_export_is_named_for_its_days_and_audits_what_it_covered() {
    let everything = read("from=2026-11-01&to=2026-11-21").unwrap();
    let narrowed = read(&format!(
        "from=2026-11-01&to=2026-11-21&workspace_id={WORKSPACE}"
    ))
    .unwrap();

    assert_eq!(
        filename(&everything),
        "zone-spend-2026-11-01-2026-11-21.csv"
    );
    assert_eq!(
        everything.audited(),
        json!({ "from": "2026-11-01", "to": "2026-11-21", "workspace_id": null, "user_id": null })
    );
    assert_eq!(narrowed.audited()["workspace_id"], json!(WORKSPACE));
}
