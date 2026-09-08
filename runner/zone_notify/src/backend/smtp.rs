//! How to reach an SMTP relay.

use std::fmt;

use zone_core::SecretValue;

/// The connection details for one SMTP relay.
///
/// The password is a [`SecretValue`], so it is redacted in `Debug`, zeroized
/// when the config is dropped, and readable only at the point it is handed to
/// the transport.
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: SecretValue,
    pub from_address: String,
    pub from_name: String,
}

impl fmt::Debug for SmtpConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password)
            .field("from_address", &self.from_address)
            .field("from_name", &self.from_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.zone.test".to_string(),
            port: 587,
            user: "postmaster".to_string(),
            password: SecretValue::new("hunter2-not-a-real-password"),
            from_address: "noreply@zone.test".to_string(),
            from_name: "Zone".to_string(),
        }
    }

    #[test]
    fn the_password_never_appears_in_debug() {
        let rendered = format!("{:?}", config());
        assert!(!rendered.contains("hunter2"), "leaked: {rendered}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("smtp.zone.test"));
    }

    #[test]
    fn the_password_is_still_readable_at_the_point_of_use() {
        assert_eq!(config().password.expose(), "hunter2-not-a-real-password");
    }
}
