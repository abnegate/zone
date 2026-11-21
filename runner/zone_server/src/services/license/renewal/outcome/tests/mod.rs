use super::*;

const WIRE: [(Outcome, &str); 6] = [
    (Outcome::Renewed, "renewed"),
    (Outcome::Current, "current"),
    (Outcome::Ended, "ended"),
    (Outcome::Refused, "refused"),
    (Outcome::Rejected, "rejected"),
    (Outcome::Unreachable, "unreachable"),
];

#[test]
fn wire_strings_are_pinned() {
    for (outcome, wire) in WIRE {
        assert_eq!(outcome.as_str(), wire);
        assert_eq!(serde_json::to_value(outcome).unwrap(), wire);
    }
    assert_eq!(Outcome::ALL, WIRE.map(|(outcome, _)| outcome));
}

#[test]
fn every_outcome_parses_back() {
    for outcome in Outcome::ALL {
        assert_eq!(Outcome::parse(outcome.as_str()), Some(outcome));
        assert_eq!(
            serde_json::from_value::<Outcome>(serde_json::to_value(outcome).unwrap()).unwrap(),
            outcome
        );
    }
}

#[test]
fn an_unknown_value_does_not_parse() {
    for unknown in ["", "Renewed", " renewed", "expired"] {
        assert_eq!(Outcome::parse(unknown), None, "{unknown:?}");
    }
}
