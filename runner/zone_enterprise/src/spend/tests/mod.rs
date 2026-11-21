use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use super::*;

const WORKSPACE: Uuid = Uuid::from_u128(0x2222_2222_2222_4222_8222_2222_2222_2222);
const USER: Uuid = Uuid::from_u128(0x3333_3333_3333_4333_8333_3333_3333_3333);
const SCOPES: [Scope; 3] = [Scope::Organization, Scope::Workspace, Scope::User];

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

fn draft(body: Value) -> Draft {
    serde_json::from_value(body).unwrap()
}

fn budget(limit: i64, threshold_percent: u8) -> Budget {
    Budget {
        limit: Microdollars(limit),
        threshold_percent,
        hard: false,
    }
}

fn every_error() -> [Error; 5] {
    [
        Error::LimitInvalid,
        Error::ThresholdInvalid,
        Error::ScopeInvalid,
        Error::Exists,
        Error::RangeInvalid,
    ]
}

#[test]
fn codes_are_the_wire_contract() {
    let codes: Vec<&str> = every_error().iter().map(Error::code).collect();
    assert_eq!(
        codes,
        [
            "budget_limit_invalid",
            "budget_threshold_invalid",
            "budget_scope_invalid",
            "budget_exists",
            "spend_range_invalid",
        ]
    );
}

#[test]
fn constants_are_the_wire_contract() {
    assert_eq!(BODY_INVALID, "spend_body_invalid");
    assert_eq!(RANGE_INVALID, "spend_range_invalid");
    assert_eq!(LIMIT_INVALID, "budget_limit_invalid");
    assert_eq!(THRESHOLD_INVALID, "budget_threshold_invalid");
    assert_eq!(SCOPE_INVALID, "budget_scope_invalid");
    assert_eq!(BUDGET_EXISTS, "budget_exists");
    assert_eq!(BUDGET_EXHAUSTED, "budget_exhausted");
}

#[test]
fn every_message_is_one_line() {
    for error in every_error() {
        let message = error.to_string();
        assert!(!message.is_empty() && !message.contains('\n'), "{error:?}");
    }
}

#[test]
fn rounds_dollars_half_away_from_zero() {
    for (dollars, expected) in [
        (0.0000005, 1),
        (0.0000004, 0),
        (1.2345675, 1_234_568),
        (1.2345674, 1_234_567),
        (0.000001, 1),
        (12.5, 12_500_000),
        (0.0, 0),
    ] {
        assert_eq!(
            Microdollars::from_dollars(dollars),
            Microdollars(expected),
            "{dollars}"
        );
    }
}

#[test]
fn clamps_negative_and_unreadable_dollars() {
    assert_eq!(Microdollars::from_dollars(-1.0), Microdollars::ZERO);
    assert_eq!(Microdollars::from_dollars(-0.0000005), Microdollars::ZERO);
    assert_eq!(Microdollars::from_dollars(f64::NAN), Microdollars::ZERO);
    assert_eq!(
        Microdollars::from_dollars(f64::INFINITY),
        Microdollars(i64::MAX)
    );
    assert_eq!(Microdollars::from_dollars(1e300), Microdollars(i64::MAX));
}

#[test]
fn formats_dollars_and_exact_amounts() {
    for (microdollars, dollars, exact) in [
        (0, "0.00", "0.000000"),
        (1, "0.00", "0.000001"),
        (4_999, "0.00", "0.004999"),
        (5_000, "0.01", "0.005000"),
        (1_234_568, "1.23", "1.234568"),
        (50_000_000, "50.00", "50.000000"),
        (99_995_000, "100.00", "99.995000"),
        (-1_500_000, "-1.50", "-1.500000"),
    ] {
        let amount = Microdollars(microdollars);
        assert_eq!(amount.dollars(), dollars, "{microdollars}");
        assert_eq!(amount.exact(), exact, "{microdollars}");
    }
}

#[test]
fn serialises_microdollars_as_a_bare_integer() {
    assert_eq!(json!(Microdollars(42)), json!(42));
    assert_eq!(
        serde_json::from_value::<Microdollars>(json!(7)).unwrap(),
        Microdollars(7)
    );
}

#[test]
fn a_month_runs_from_the_first_to_the_next_first() {
    let month = Month::of(at("2026-11-21T10:04:00Z"));
    assert_eq!(month.label(), "2026-11");
    assert_eq!(month.date(), day("2026-11-01"));
    assert_eq!(month.start(), at("2026-11-01T00:00:00Z"));
    assert_eq!(month.end(), at("2026-12-01T00:00:00Z"));
}

#[test]
fn a_month_rolls_over_the_year_and_keeps_its_edges() {
    let december = Month::of(at("2026-12-31T23:59:59.999999Z"));
    assert_eq!(december.label(), "2026-12");
    assert_eq!(december.end(), at("2027-01-01T00:00:00Z"));
    assert_eq!(Month::of(at("2027-01-01T00:00:00Z")).label(), "2027-01");
    assert_eq!(
        Month::of(at("2026-11-01T00:00:00Z")),
        Month::of(at("2026-11-30T23:59:59Z"))
    );
    assert_eq!(
        Month::of(at("2028-02-29T12:00:00Z")).end(),
        at("2028-03-01T00:00:00Z")
    );
}

#[test]
fn the_current_month_contains_now() {
    let before = Utc::now();
    let month = Month::current();
    let after = Utc::now();
    assert!(month == Month::of(before) || month == Month::of(after));
    assert!(month.start() <= after && before < month.end());
}

#[test]
fn a_range_defaults_to_the_month_so_far() {
    let range = Range::parse(None, None, day("2026-11-21")).unwrap();
    assert_eq!(
        range,
        Range {
            from: day("2026-11-01"),
            to: day("2026-11-21"),
        }
    );
    assert_eq!(range.start(), at("2026-11-01T00:00:00Z"));
    assert_eq!(range.end(), at("2026-11-22T00:00:00Z"));
}

#[test]
fn a_range_takes_either_end_alone() {
    let today = day("2026-11-21");
    assert_eq!(
        Range::parse(Some("2026-10-15"), None, today).unwrap(),
        Range {
            from: day("2026-10-15"),
            to: today,
        }
    );
    assert_eq!(
        Range::parse(None, Some("2026-11-05"), today).unwrap(),
        Range {
            from: day("2026-11-01"),
            to: day("2026-11-05"),
        }
    );
}

#[test]
fn a_single_day_range_covers_that_day() {
    let range = Range::parse(Some("2026-11-03"), Some("2026-11-03"), day("2026-11-21")).unwrap();
    assert_eq!(range.start(), at("2026-11-03T00:00:00Z"));
    assert_eq!(range.end(), at("2026-11-04T00:00:00Z"));
}

#[test]
fn refuses_bad_dates_and_backward_ranges() {
    let today = day("2026-11-21");
    for (from, to) in [
        (Some("2026-11-31"), None),
        (Some("2026/11/01"), None),
        (Some("2026-11-1"), None),
        (Some(""), None),
        (None, Some("yesterday")),
        (Some(" 2026-11-01"), None),
        (Some("2026-11-02"), Some("2026-11-01")),
        (Some("2026-11-22"), None),
    ] {
        assert_eq!(
            Range::parse(from, to, today),
            Err(Error::RangeInvalid),
            "{from:?} to {to:?}"
        );
    }
}

#[test]
fn a_range_covers_at_most_a_leap_year_of_days() {
    let today = day("2026-11-21");
    let longest = Range::parse(Some("2025-11-21"), None, today).unwrap();
    assert_eq!(longest.length(), MAXIMUM_DAYS);
    assert_eq!(longest.days().count(), 366);
    assert_eq!(
        Range::parse(Some("2025-11-20"), None, today),
        Err(Error::RangeInvalid),
        "a day longer than the longest range"
    );
    assert_eq!(
        Range::parse(Some("2000-01-01"), Some("2026-11-21"), today),
        Err(Error::RangeInvalid)
    );
}

#[test]
fn a_range_walks_its_days_in_order() {
    let range = Range::parse(Some("2026-02-27"), Some("2026-03-02"), day("2026-11-21")).unwrap();
    assert_eq!(
        range.days().collect::<Vec<_>>(),
        [
            day("2026-02-27"),
            day("2026-02-28"),
            day("2026-03-01"),
            day("2026-03-02"),
        ]
    );
    assert_eq!(range.length(), 4);
}

#[test]
fn state_follows_the_threshold_and_the_limit() {
    let budget = budget(100_000_000, 80);
    for (spent, state) in [
        (0, State::Under),
        (79_999_999, State::Under),
        (80_000_000, State::Warning),
        (99_999_999, State::Warning),
        (100_000_000, State::Exhausted),
        (150_000_000, State::Exhausted),
    ] {
        assert_eq!(budget.state(Microdollars(spent)), state, "{spent}");
    }
}

#[test]
fn state_rounds_no_fraction_of_the_threshold() {
    let budget = budget(3, 50);
    assert_eq!(budget.state(Microdollars(1)), State::Under);
    assert_eq!(budget.state(Microdollars(2)), State::Warning);
    assert_eq!(budget.state(Microdollars(3)), State::Exhausted);
}

#[test]
fn a_full_threshold_goes_straight_to_exhausted() {
    let budget = budget(10, 100);
    assert_eq!(budget.state(Microdollars(9)), State::Under);
    assert_eq!(budget.state(Microdollars(10)), State::Exhausted);
}

#[test]
fn state_survives_the_largest_amounts() {
    let budget = budget(i64::MAX, 100);
    assert_eq!(budget.state(Microdollars(i64::MAX - 1)), State::Under);
    assert_eq!(budget.state(Microdollars(i64::MAX)), State::Exhausted);
}

#[test]
fn states_and_scopes_serialise_as_the_contract() {
    assert_eq!(
        json!([State::Under, State::Warning, State::Exhausted]),
        json!(["under", "warning", "exhausted"])
    );
    assert_eq!(json!(SCOPES), json!(["organization", "workspace", "user"]));
    assert_eq!(
        json!([
            Uncounted::CodingAgent,
            Uncounted::DirectProvider,
            Uncounted::ExternalEndpoint,
            Uncounted::SavedKey,
        ]),
        json!([
            "coding_agent",
            "direct_provider",
            "external_endpoint",
            "saved_key"
        ])
    );
}

#[test]
fn a_draft_defaults_its_threshold_and_softness() {
    let (target, budget) =
        draft(json!({"scope": "organization", "limit_microdollars": 50_000_000}))
            .validate()
            .unwrap();
    assert_eq!(
        target,
        Target {
            scope: Scope::Organization,
            workspace: None,
            user: None,
        }
    );
    assert_eq!(
        budget,
        Budget {
            limit: Microdollars(50_000_000),
            threshold_percent: DEFAULT_THRESHOLD,
            hard: false,
        }
    );
    assert_eq!(DEFAULT_THRESHOLD, 80);
}

#[test]
fn a_draft_names_the_workspace_or_member_it_covers() {
    let (workspace, _) = draft(json!({
        "scope": "workspace",
        "workspace_id": WORKSPACE,
        "user_id": null,
        "limit_microdollars": 1,
        "threshold_percent": 100,
        "hard": true,
    }))
    .validate()
    .unwrap();
    assert_eq!(workspace, Target::of(Some(WORKSPACE), None));
    assert_eq!(workspace.scope, Scope::Workspace);
    let (user, budget) = draft(json!({
        "scope": "user",
        "user_id": USER,
        "limit_microdollars": 1,
        "threshold_percent": 1,
    }))
    .validate()
    .unwrap();
    assert_eq!(user, Target::of(None, Some(USER)));
    assert_eq!(user.scope, Scope::User);
    assert_eq!(budget.threshold_percent, 1);
}

#[test]
fn a_draft_refuses_a_limit_of_nothing() {
    for limit in [0, -1, i64::MIN] {
        let body = json!({"scope": "organization", "limit_microdollars": limit});
        assert_eq!(draft(body).validate(), Err(Error::LimitInvalid), "{limit}");
    }
}

#[test]
fn a_draft_refuses_a_threshold_outside_one_to_a_hundred() {
    for threshold in [0, 101, -1, 256, 300] {
        let body = json!({
            "scope": "organization",
            "limit_microdollars": 1,
            "threshold_percent": threshold,
        });
        assert_eq!(
            draft(body).validate(),
            Err(Error::ThresholdInvalid),
            "{threshold}"
        );
    }
}

#[test]
fn a_draft_refuses_ids_its_scope_does_not_take() {
    for body in [
        json!({"scope": "organization", "workspace_id": WORKSPACE, "limit_microdollars": 1}),
        json!({"scope": "organization", "user_id": USER, "limit_microdollars": 1}),
        json!({"scope": "workspace", "limit_microdollars": 1}),
        json!({"scope": "workspace", "workspace_id": WORKSPACE, "user_id": USER, "limit_microdollars": 1}),
        json!({"scope": "user", "limit_microdollars": 1}),
        json!({"scope": "user", "workspace_id": WORKSPACE, "user_id": USER, "limit_microdollars": 1}),
    ] {
        assert_eq!(
            draft(body.clone()).validate(),
            Err(Error::ScopeInvalid),
            "{body}"
        );
    }
}

#[test]
fn a_draft_refuses_malformed_bodies() {
    for body in [
        json!({"limit_microdollars": 1}),
        json!({"scope": "team", "limit_microdollars": 1}),
        json!({"scope": "organization"}),
        json!({"scope": "organization", "limit_microdollars": 1.5}),
        json!({"scope": "organization", "limit_microdollars": "1"}),
        json!({"scope": "organization", "limit_microdollars": 1, "threshold_percent": null}),
        json!({"scope": "organization", "limit_microdollars": 1, "soft": true}),
        json!({"scope": "workspace", "workspace_id": "not-a-uuid", "limit_microdollars": 1}),
    ] {
        assert!(
            serde_json::from_value::<Draft>(body.clone()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn a_patch_changes_only_what_it_names() {
    let current = Budget {
        limit: Microdollars(50_000_000),
        threshold_percent: 80,
        hard: false,
    };
    let patch: Patch = serde_json::from_value(json!({})).unwrap();
    assert_eq!(patch.applied(&current), Ok(current));
    let patch: Patch = serde_json::from_value(json!({"hard": true})).unwrap();
    assert_eq!(
        patch.applied(&current),
        Ok(Budget {
            hard: true,
            ..current
        })
    );
    let patch: Patch =
        serde_json::from_value(json!({"limit_microdollars": 7, "threshold_percent": 95})).unwrap();
    assert_eq!(
        patch.applied(&current),
        Ok(Budget {
            limit: Microdollars(7),
            threshold_percent: 95,
            hard: false,
        })
    );
}

#[test]
fn a_patch_refuses_what_a_draft_refuses() {
    let current = budget(50_000_000, 80);
    let limit: Patch = serde_json::from_value(json!({"limit_microdollars": 0})).unwrap();
    assert_eq!(limit.applied(&current), Err(Error::LimitInvalid));
    let threshold: Patch = serde_json::from_value(json!({"threshold_percent": 0})).unwrap();
    assert_eq!(threshold.applied(&current), Err(Error::ThresholdInvalid));
    for body in [json!({"scope": "user"}), json!({"hard": "yes"})] {
        assert!(
            serde_json::from_value::<Patch>(body.clone()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn a_stored_target_takes_its_scope_from_its_ids() {
    assert_eq!(Target::of(None, None).scope, Scope::Organization);
    assert_eq!(Target::of(Some(WORKSPACE), None).scope, Scope::Workspace);
    assert_eq!(Target::of(None, Some(USER)).scope, Scope::User);
}

#[test]
fn exhausted_messages_are_the_contract() {
    let messages: Vec<String> = SCOPES
        .into_iter()
        .map(|scope| Exhausted { scope }.to_string())
        .collect();
    assert_eq!(
        messages,
        [
            "Your organization's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).",
            "This workspace's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).",
            "Your monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).",
        ]
    );
    for scope in SCOPES {
        assert_eq!(Exhausted { scope }.code(), "budget_exhausted");
    }
}

#[cfg(feature = "audit")]
mod csv {
    use super::*;
    use crate::spend::csv::{HEADER, row};

    fn line() -> Line {
        Line {
            date: day("2026-11-03"),
            workspace_id: Some(WORKSPACE),
            workspace: Some("Platform".to_owned()),
            user_id: Some(USER),
            user_email: Some("ada@example.com".to_owned()),
            automated: false,
            model: "gpt-4o".to_owned(),
            provider: "openai".to_owned(),
            requests: 3,
            prompt_tokens: 1200,
            completion_tokens: 340,
            spend: Microdollars(1_234_568),
        }
    }

    #[test]
    fn header_names_every_column_and_ends_with_crlf() {
        assert_eq!(
            HEADER,
            "date,workspace_id,workspace,user_id,user_email,automated,model,provider,requests,prompt_tokens,completion_tokens,spend_usd\r\n"
        );
    }

    #[test]
    fn writes_a_row() {
        assert_eq!(
            row(&line()),
            concat!(
                "2026-11-03,22222222-2222-4222-8222-222222222222,Platform,",
                "33333333-3333-4333-8333-333333333333,ada@example.com,false,gpt-4o,openai,",
                "3,1200,340,1.234568\r\n",
            )
        );
    }

    #[test]
    fn writes_automation_and_deleted_workspaces_as_empty_fields() {
        let line = Line {
            workspace_id: None,
            workspace: None,
            user_id: None,
            user_email: None,
            automated: true,
            spend: Microdollars::ZERO,
            ..line()
        };
        assert_eq!(
            row(&line),
            "2026-11-03,,,,,true,gpt-4o,openai,3,1200,340,0.000000\r\n"
        );
    }

    #[test]
    fn writes_a_former_member_apart_from_automation() {
        let line = Line {
            user_id: None,
            user_email: None,
            ..line()
        };
        assert_eq!(
            row(&line),
            concat!(
                "2026-11-03,22222222-2222-4222-8222-222222222222,Platform,,,false,",
                "gpt-4o,openai,3,1200,340,1.234568\r\n",
            )
        );
    }

    #[test]
    fn quotes_delimiters_quotes_and_line_breaks() {
        let line = Line {
            workspace: Some("Ops, \"core\"".to_owned()),
            model: "one\r\ntwo".to_owned(),
            ..line()
        };
        let row = row(&line);
        assert!(row.contains(",\"Ops, \"\"core\"\"\","), "{row}");
        assert!(row.contains(",\"one\r\ntwo\","), "{row}");
    }

    #[test]
    fn neutralises_formula_prefixes() {
        for (value, expected) in [
            ("=SUM(A1)", "\"'=SUM(A1)\""),
            ("+1", "\"'+1\""),
            ("-1", "\"'-1\""),
            ("@cmd", "\"'@cmd\""),
        ] {
            let line = Line {
                model: value.to_owned(),
                provider: value.to_owned(),
                workspace: Some(value.to_owned()),
                user_email: Some(value.to_owned()),
                ..line()
            };
            let fields = row(&line);
            assert_eq!(
                fields.matches(expected).count(),
                4,
                "{value:?} in {fields:?}"
            );
        }
    }
}
