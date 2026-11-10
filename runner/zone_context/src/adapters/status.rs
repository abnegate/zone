use reqwest::StatusCode;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Status(pub(crate) u16);

impl Status {
    pub(crate) const UNAUTHORIZED: Self = Self(401);
    pub(crate) const FORBIDDEN: Self = Self(403);
    pub(crate) const NOT_FOUND: Self = Self(404);
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match StatusCode::from_u16(self.0) {
            Ok(status) => write!(formatter, "{status}"),
            Err(_) => write!(formatter, "{}", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_written_with_its_standard_reason() {
        for (status, written) in [
            (Status::UNAUTHORIZED, "401 Unauthorized"),
            (Status::FORBIDDEN, "403 Forbidden"),
            (Status::NOT_FOUND, "404 Not Found"),
            (Status(500), "500 Internal Server Error"),
        ] {
            assert_eq!(status.to_string(), written);
        }
    }

    #[test]
    fn a_status_with_no_standard_reason_says_so() {
        assert_eq!(Status(599).to_string(), "599 <unknown status code>");
    }

    #[test]
    fn a_number_outside_the_status_range_is_written_bare() {
        assert_eq!(Status(42).to_string(), "42");
    }
}
