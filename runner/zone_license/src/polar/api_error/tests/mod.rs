use super::*;

#[test]
fn only_an_unavailable_api_is_retryable() {
    assert!(ApiError::Unavailable.is_retryable());
    assert!(!ApiError::Refused { status: 401 }.is_retryable());
    assert!(!ApiError::Malformed.is_retryable());
}

#[test]
fn messages_are_fixed() {
    assert_eq!(
        ApiError::Unavailable.to_string(),
        "the Polar API was unreachable, timed out or was unavailable"
    );
    assert_eq!(
        ApiError::Refused { status: 403 }.to_string(),
        "the Polar API refused the request with status 403"
    );
    assert_eq!(
        ApiError::Malformed.to_string(),
        "the Polar API answered with a body that could not be read"
    );
}
