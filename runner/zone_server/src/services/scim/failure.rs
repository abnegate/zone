use zone_enterprise::scim::{self, Error, ScimType};

use crate::auth::entitlement::ENTERPRISE_MESSAGE;
use crate::db::scim::{groups, users};
use crate::services::members::AdmissionError;

pub const UNAUTHENTICATED: &str = "A valid SCIM bearer token for this organization is required.";
pub const LAST_OWNER: &str =
    "That person is the organization's last active owner, so they can't be deactivated or removed.";
pub const TOO_LARGE: &str = "The request body is too large.";
pub const UNAVAILABLE: &str = "Internal server error";

const UNAUTHORIZED: u16 = 401;
const FORBIDDEN: u16 = 403;
const PAYLOAD_TOO_LARGE: u16 = 413;
const TOO_MANY_REQUESTS: u16 = 429;
const INTERNAL_SERVER_ERROR: u16 = 500;
const SERVICE_UNAVAILABLE: u16 = 503;
const EMAIL_KEYS: [&str; 2] = ["users_email_key", "users_email_lower_key"];
const EMAIL_TAKEN: &str = "Another account already has that email address.";
const USER_NAME_TAKEN: &str = "Another user in the organization already has that userName.";
const USER_EXTERNAL_ID_TAKEN: &str =
    "Another user in the organization already has that externalId.";
const DISPLAY_NAME_TAKEN: &str = "Another group in the organization already has that displayName.";
const GROUP_EXTERNAL_ID_TAKEN: &str =
    "Another group in the organization already has that externalId.";

#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    Protocol(#[from] Error),
    #[error("{UNAUTHENTICATED}")]
    Unauthenticated,
    #[error("{ENTERPRISE_MESSAGE}")]
    Unentitled,
    #[error("{0}")]
    Forbidden(String),
    #[error("{LAST_OWNER}")]
    LastOwner,
    #[error("{TOO_LARGE}")]
    TooLarge,
    #[error("Too many SCIM requests for this organization. Try again in {retry_after} seconds.")]
    Throttled { retry_after: u64 },
    #[error("Too many SCIM requests from your network. Try again in {retry_after} seconds.")]
    Flooded { retry_after: u64 },
    #[error("{0}")]
    Busy(String),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error("{0}")]
    Internal(&'static str),
}

impl Failure {
    pub fn stored(error: sqlx::Error) -> Self {
        let taken = match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                match database.constraint() {
                    Some(key) if EMAIL_KEYS.contains(&key) => Some(EMAIL_TAKEN),
                    Some(users::USER_NAME_KEY) => Some(USER_NAME_TAKEN),
                    Some(users::EXTERNAL_ID_KEY) => Some(USER_EXTERNAL_ID_TAKEN),
                    Some(groups::DISPLAY_NAME_KEY) => Some(DISPLAY_NAME_TAKEN),
                    Some(groups::EXTERNAL_ID_KEY) => Some(GROUP_EXTERNAL_ID_TAKEN),
                    _ => None,
                }
            }
            _ => None,
        };
        match taken {
            Some(detail) => Self::Protocol(Error::Uniqueness(detail.to_owned())),
            None => Self::Storage(error),
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Protocol(error) => error.status(),
            Self::Unauthenticated => UNAUTHORIZED,
            Self::Unentitled | Self::Forbidden(_) => FORBIDDEN,
            Self::LastOwner => scim::CONFLICT,
            Self::TooLarge => PAYLOAD_TOO_LARGE,
            Self::Throttled { .. } | Self::Flooded { .. } => TOO_MANY_REQUESTS,
            Self::Busy(_) => SERVICE_UNAVAILABLE,
            Self::Storage(_) | Self::Internal(_) => INTERNAL_SERVER_ERROR,
        }
    }

    pub fn scim_type(&self) -> Option<ScimType> {
        match self {
            Self::Protocol(error) => error.scim_type(),
            Self::Throttled { .. } | Self::Flooded { .. } => Some(ScimType::TooMany),
            Self::Unauthenticated
            | Self::Unentitled
            | Self::Forbidden(_)
            | Self::LastOwner
            | Self::TooLarge
            | Self::Busy(_)
            | Self::Storage(_)
            | Self::Internal(_) => None,
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Storage(_) | Self::Internal(_) => UNAVAILABLE.to_owned(),
            other => other.to_string(),
        }
    }

    pub fn wire(&self) -> scim::Failure {
        scim::Failure::new(self.status(), self.scim_type(), self.detail())
    }
}

impl From<AdmissionError> for Failure {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Full { .. } => Self::Forbidden(error.to_string()),
            AdmissionError::Busy => Self::Busy(error.to_string()),
            AdmissionError::Database(error) => Self::Storage(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_has_its_status_and_scim_type() {
        let cases = [
            (Failure::Protocol(Error::NotFound), 404, None),
            (
                Failure::Protocol(Error::InvalidFilter("bad".into())),
                400,
                Some(ScimType::InvalidFilter),
            ),
            (
                Failure::Protocol(Error::Uniqueness("taken".into())),
                409,
                Some(ScimType::Uniqueness),
            ),
            (Failure::Unauthenticated, 401, None),
            (Failure::Unentitled, 403, None),
            (Failure::Forbidden("full".into()), 403, None),
            (Failure::LastOwner, 409, None),
            (Failure::TooLarge, 413, None),
            (
                Failure::Throttled { retry_after: 3 },
                429,
                Some(ScimType::TooMany),
            ),
            (
                Failure::Flooded { retry_after: 3 },
                429,
                Some(ScimType::TooMany),
            ),
            (Failure::Busy("busy".into()), 503, None),
            (Failure::Storage(sqlx::Error::PoolTimedOut), 500, None),
            (Failure::Internal("hashing"), 500, None),
        ];
        for (failure, status, scim_type) in cases {
            assert_eq!(failure.status(), status, "{failure:?}");
            assert_eq!(failure.scim_type(), scim_type, "{failure:?}");
        }
    }

    #[test]
    fn a_lapsed_license_is_refused_with_the_enterprise_message() {
        assert_eq!(Failure::Unentitled.detail(), ENTERPRISE_MESSAGE);
    }

    #[test]
    fn an_internal_failure_never_shows_its_cause() {
        let failure = Failure::Storage(sqlx::Error::Protocol("relation scim_users".into()));
        assert_eq!(failure.detail(), UNAVAILABLE);
        assert_eq!(Failure::Internal("argon2 refused").detail(), UNAVAILABLE);
    }

    #[test]
    fn a_full_server_refuses_with_the_admission_message() {
        let failure = Failure::from(AdmissionError::Full { cap: 3 });
        assert_eq!(failure.status(), 403);
        assert_eq!(
            failure.detail(),
            AdmissionError::Full { cap: 3 }.to_string()
        );
    }

    #[test]
    fn a_held_seat_lock_is_unavailable_with_the_admission_message() {
        let failure = Failure::from(AdmissionError::Busy);
        assert_eq!(failure.status(), 503);
        assert_eq!(failure.scim_type(), None);
        assert_eq!(failure.detail(), AdmissionError::Busy.to_string());
    }

    #[test]
    fn other_storage_errors_stay_storage_errors() {
        assert!(matches!(
            Failure::stored(sqlx::Error::RowNotFound),
            Failure::Storage(sqlx::Error::RowNotFound)
        ));
    }
}
