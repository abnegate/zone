use super::*;

#[test]
fn team_ranks_below_enterprise() {
    assert!(Plan::Team < Plan::Enterprise);
    let mut plans = [Plan::Enterprise, Plan::Team];
    plans.sort_unstable();
    assert_eq!(plans, Plan::ALL);
}

#[test]
fn parses_each_wire_value() {
    for plan in Plan::ALL {
        assert_eq!(plan.as_str().parse::<Plan>(), Ok(plan), "{plan:?}");
    }
    assert_eq!("team".parse::<Plan>(), Ok(Plan::Team));
    assert_eq!("enterprise".parse::<Plan>(), Ok(Plan::Enterprise));
}

#[test]
fn refuses_anything_but_an_exact_wire_value() {
    for value in [
        "",
        "Team",
        "ENTERPRISE",
        " team",
        "team ",
        "business",
        "community",
    ] {
        assert_eq!(
            value.parse::<Plan>(),
            Err(UnknownPlan(value.to_owned())),
            "accepted {value:?}"
        );
    }
}

#[test]
fn names_the_unknown_plan() {
    let error = "business".parse::<Plan>().unwrap_err();
    assert!(error.to_string().contains("business"), "{error}");
}

#[test]
fn displays_its_wire_value() {
    let values: Vec<String> = Plan::ALL.iter().map(Plan::to_string).collect();
    assert_eq!(values, ["team", "enterprise"]);
    for plan in Plan::ALL {
        assert_eq!(plan.to_string(), plan.as_str());
        assert_eq!(serde_json::to_value(plan).unwrap(), plan.as_str());
    }
}

#[test]
fn deserializes_exactly_its_wire_values() {
    for plan in Plan::ALL {
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&json).unwrap(), plan);
    }
    for value in [r#""Team""#, r#""business""#, r#""community""#, r#""""#] {
        assert!(
            serde_json::from_str::<Plan>(value).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn team_includes_spend_and_members_only() {
    for feature in Feature::ALL {
        assert_eq!(
            Plan::Team.includes(feature),
            matches!(feature, Feature::Spend | Feature::Members),
            "{feature:?}"
        );
    }
}

#[test]
fn enterprise_includes_every_feature() {
    for feature in Feature::ALL {
        assert!(Plan::Enterprise.includes(feature), "{feature:?}");
    }
}
