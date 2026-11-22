use super::*;

#[test]
fn each_outcome_has_a_fixed_wire_name() {
    let names: Vec<&str> = TokenOutcome::ALL
        .iter()
        .map(|outcome| outcome.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "issued",
            "disabled",
            "unreadable",
            "anonymous",
            "malformed",
            "unverified",
            "mismatched",
            "revoked",
            "ineligible",
            "lapsed",
            "limited",
            "unaddressable",
        ]
    );
}

#[test]
fn each_outcome_answers_its_contracted_status() {
    let statuses: Vec<u16> = TokenOutcome::ALL
        .iter()
        .map(|outcome| outcome.status().as_u16())
        .collect();
    assert_eq!(
        statuses,
        [200, 404, 400, 401, 401, 401, 401, 401, 403, 403, 429, 500]
    );
}

#[test]
fn displays_its_wire_name() {
    for outcome in TokenOutcome::ALL {
        assert_eq!(outcome.to_string(), outcome.as_str());
    }
}

#[test]
fn every_refusal_is_an_outcome_with_the_gate_status() {
    for refusal in Refusal::ALL {
        let outcome = TokenOutcome::from(refusal);
        assert!(TokenOutcome::ALL.contains(&outcome), "{refusal}");
        assert_eq!(outcome.status(), refusal.status(), "{refusal}");
        assert_eq!(outcome.as_str(), refusal.as_str());
    }
}

#[test]
fn unaddressable_answers_as_the_gate_does() {
    assert_eq!(
        TokenOutcome::Unaddressable.status(),
        crate::gate::Unaddressable.status()
    );
}
