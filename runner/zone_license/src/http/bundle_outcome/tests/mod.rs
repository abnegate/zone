use super::*;
use crate::gate::Refusal;

#[test]
fn each_outcome_has_a_fixed_wire_name() {
    assert_eq!(
        BundleOutcome::ALL.map(BundleOutcome::as_str),
        ["issued", "disabled", "unknown", "limited", "unavailable"]
    );
}

#[test]
fn each_outcome_answers_its_contracted_status() {
    assert_eq!(
        BundleOutcome::ALL.map(|outcome| outcome.status().as_u16()),
        [200, 404, 404, 429, 503]
    );
}

#[test]
fn displays_its_wire_name() {
    for outcome in BundleOutcome::ALL {
        assert_eq!(outcome.to_string(), outcome.as_str());
    }
}

#[test]
fn no_outcome_shares_a_name_with_a_refusal() {
    for outcome in BundleOutcome::ALL {
        assert!(
            Refusal::ALL
                .iter()
                .all(|refusal| refusal.as_str() != outcome.as_str()),
            "{outcome}"
        );
    }
}
