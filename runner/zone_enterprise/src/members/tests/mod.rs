use chrono::TimeDelta;

use super::*;
use crate::fixtures;
use crate::payload::Payload;
use crate::plan::Plan;

fn license(plan: Plan) -> License {
    fixtures::license(&Payload {
        plan: Some(plan),
        ..fixtures::payload()
    })
}

fn lapsed(license: &License) -> DateTime<Utc> {
    license.grace_ends_at() + TimeDelta::days(1)
}

fn capped(active: u32) -> Members {
    Members {
        active,
        cap: Some(COMMUNITY_MEMBERS),
    }
}

#[test]
fn community_allows_three_members() {
    assert_eq!(COMMUNITY_MEMBERS, 3);
}

#[test]
fn code_is_the_wire_contract() {
    assert_eq!(MEMBER_CAP_REACHED, "member_cap_reached");
    assert_eq!(MEMBER_ADMISSION_BUSY, "member_admission_busy");
}

#[test]
fn no_license_is_capped() {
    let now = fixtures::payload().issued_at;
    assert_eq!(Members::cap(None, now), Some(COMMUNITY_MEMBERS));
}

#[test]
fn a_lapsed_license_is_capped_on_either_plan() {
    for plan in Plan::ALL {
        let license = license(plan);
        assert_eq!(
            Members::cap(Some(&license), lapsed(&license)),
            Some(COMMUNITY_MEMBERS),
            "{plan:?}"
        );
    }
}

#[test]
fn an_active_or_grace_license_is_uncapped_on_either_plan() {
    for plan in Plan::ALL {
        let license = license(plan);
        for now in [license.issued_at(), license.expires_at()] {
            assert_eq!(Members::cap(Some(&license), now), None, "{plan:?} at {now}");
        }
    }
}

#[test]
fn a_license_without_a_plan_is_uncapped_while_active() {
    let license = fixtures::license(&fixtures::payload());
    assert_eq!(Members::cap(Some(&license), license.issued_at()), None);
}

#[test]
fn fills_at_the_cap() {
    assert!(!capped(2).full());
    assert!(capped(3).full());
    assert!(capped(4).full());
}

#[test]
fn is_over_only_beyond_the_cap() {
    assert!(!capped(2).over());
    assert!(!capped(3).over());
    assert!(capped(4).over());
}

#[test]
fn uncapped_is_never_full_or_over() {
    for active in [0, 2, 3, 4, u32::MAX] {
        let members = Members { active, cap: None };
        assert!(!members.full(), "{active}");
        assert!(!members.over(), "{active}");
    }
}
