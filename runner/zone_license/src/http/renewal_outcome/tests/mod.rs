use super::*;

#[test]
fn each_outcome_has_a_fixed_wire_name() {
    let names: Vec<&str> = RenewalOutcome::ALL
        .iter()
        .map(|outcome| outcome.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "renewed",
            "current",
            "unreadable",
            "disabled",
            "oversize",
            "unverified",
            "unsubscribed",
            "malformed",
            "ended",
            "unsignable",
            "unauthorized",
            "unexpected",
            "misconfigured",
            "unavailable",
        ]
    );
}

#[test]
fn each_outcome_answers_its_contracted_status() {
    let statuses: Vec<u16> = RenewalOutcome::ALL
        .iter()
        .map(|outcome| outcome.status().as_u16())
        .collect();
    assert_eq!(
        statuses,
        [
            200, 204, 400, 404, 413, 422, 422, 422, 410, 500, 502, 502, 502, 503
        ]
    );
}

#[test]
fn displays_its_wire_name() {
    for outcome in RenewalOutcome::ALL {
        assert_eq!(outcome.to_string(), outcome.as_str());
    }
}

#[test]
fn only_an_oversize_body_is_oversize() {
    assert_eq!(
        RenewalOutcome::from_body(StatusCode::PAYLOAD_TOO_LARGE),
        RenewalOutcome::Oversize
    );
    assert_eq!(
        RenewalOutcome::from_body(StatusCode::BAD_REQUEST),
        RenewalOutcome::Unreadable
    );
}

#[test]
fn polar_refusing_the_token_is_unauthorized() {
    for status in [401, 403] {
        assert_eq!(
            RenewalOutcome::from(ApiError::Refused { status }),
            RenewalOutcome::Unauthorized
        );
    }
}

#[test]
fn any_other_polar_answer_is_unexpected_or_unavailable() {
    assert_eq!(
        RenewalOutcome::from(ApiError::Refused { status: 400 }),
        RenewalOutcome::Unexpected
    );
    assert_eq!(
        RenewalOutcome::from(ApiError::Malformed),
        RenewalOutcome::Unexpected
    );
    assert_eq!(
        RenewalOutcome::from(ApiError::Unavailable),
        RenewalOutcome::Unavailable
    );
}
