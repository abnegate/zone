//! The relay account mail and email notices are sent through.

use abnegate_notify::Sender;
use abnegate_notify::SmtpConfig;

use crate::services::mail::Error;
use crate::services::mail::SenderPolicy;
use crate::services::mail::Variable;

pub const DEFAULT_PORT: u16 = 587;
pub const DEFAULT_SENDER: &str = "noreply@zone.app";
pub const DEFAULT_SENDER_NAME: &str = "Zone";

/// The relay and sender mail goes out through.
#[derive(Clone, Debug)]
pub struct Config {
    relay: SmtpConfig,
}

impl Config {
    /// The relay the process environment describes.
    pub fn from_environment(sender: SenderPolicy) -> Result<Self, Error> {
        Self::from_variables(Variable::read, sender)
    }

    /// The relay `read` describes.
    ///
    /// Compose passes every `SMTP_*` variable through even when `.env` leaves
    /// it out, so a value that is blank once trimmed counts as unset. The
    /// password is the exception: it is taken exactly as given, blank
    /// included, because a relay may accept an empty one.
    ///
    /// `SMTP_HOST`, `SMTP_USER` and `SMTP_PASSWORD` are required. The port and
    /// sender name fall back to [`DEFAULT_PORT`] and [`DEFAULT_SENDER_NAME`],
    /// and so does a port that is not a number, with a warning. `sender`
    /// decides whether the address falls back to [`DEFAULT_SENDER`].
    pub fn from_variables(
        read: impl Fn(Variable) -> Option<String>,
        sender: SenderPolicy,
    ) -> Result<Self, Error> {
        let value = |variable: Variable| {
            read(variable)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let required = |variable: Variable| value(variable).ok_or(Error::Missing(variable));

        let host = required(Variable::Host)?;
        let port = value(Variable::Port).map_or(DEFAULT_PORT, |port| port_or_default(&port));
        let user = required(Variable::User)?;
        let password = read(Variable::Password).ok_or(Error::Missing(Variable::Password))?;
        let address = match sender {
            SenderPolicy::Default => {
                value(Variable::From).unwrap_or_else(|| DEFAULT_SENDER.to_string())
            }
            SenderPolicy::Required => required(Variable::From)?,
        };
        let name = value(Variable::FromName).unwrap_or_else(|| DEFAULT_SENDER_NAME.to_string());

        Ok(Self {
            relay: SmtpConfig::new(
                host,
                port,
                user,
                password,
                Sender::new(address).with_name(name),
            ),
        })
    }

    pub fn relay(&self) -> &SmtpConfig {
        &self.relay
    }
}

fn port_or_default(port: &str) -> u16 {
    port.parse().unwrap_or_else(|error| {
        tracing::warn!(
            %error,
            value = port,
            default = DEFAULT_PORT,
            "{} is not a port number; using the default",
            Variable::Port.name()
        );
        DEFAULT_PORT
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "hunter2-not-a-real-password";

    fn configured(variable: Variable) -> Option<String> {
        match variable {
            Variable::Host => Some("smtp.zone.test".to_string()),
            Variable::User => Some("zone".to_string()),
            Variable::Password => Some(PASSWORD.to_string()),
            _ => None,
        }
    }

    fn blank(blanked: Variable) -> impl Fn(Variable) -> Option<String> {
        move |variable| {
            if variable == blanked {
                Some("  ".to_string())
            } else {
                configured(variable)
            }
        }
    }

    #[test]
    fn the_required_variables_alone_configure_a_relay_with_the_defaults() {
        let config = Config::from_variables(configured, SenderPolicy::Default).expect("configured");
        let relay = config.relay();

        assert_eq!(relay.host, "smtp.zone.test");
        assert_eq!(relay.port, DEFAULT_PORT);
        assert_eq!(relay.user, "zone");
        assert_eq!(relay.password.expose(), PASSWORD);
        assert_eq!(
            relay.sender,
            Sender::new(DEFAULT_SENDER).with_name(DEFAULT_SENDER_NAME)
        );
    }

    #[test]
    fn every_optional_variable_is_honoured() {
        let config = Config::from_variables(
            |variable| match variable {
                Variable::Port => Some("465".to_string()),
                Variable::From => Some("accounts@zone.test".to_string()),
                Variable::FromName => Some("Zone Accounts".to_string()),
                other => configured(other),
            },
            SenderPolicy::Default,
        )
        .expect("configured");

        assert_eq!(config.relay().port, 465);
        assert_eq!(
            config.relay().sender,
            Sender::new("accounts@zone.test").with_name("Zone Accounts")
        );
    }

    #[test]
    fn each_required_variable_is_named_when_it_is_missing() {
        for missing in [Variable::Host, Variable::User, Variable::Password] {
            let error = Config::from_variables(
                |variable| {
                    (variable != missing)
                        .then(|| configured(variable))
                        .flatten()
                },
                SenderPolicy::Default,
            )
            .expect_err("incomplete");

            assert!(
                matches!(error, Error::Missing(variable) if variable == missing),
                "{missing:?}: {error}"
            );
        }
    }

    #[test]
    fn a_blank_host_is_named_as_missing() {
        let error = Config::from_variables(blank(Variable::Host), SenderPolicy::Default)
            .expect_err("a blank host names no relay");

        assert!(matches!(error, Error::Missing(Variable::Host)), "{error}");
    }

    #[test]
    fn a_blank_user_is_named_as_missing() {
        let error = Config::from_variables(blank(Variable::User), SenderPolicy::Default)
            .expect_err("a blank user signs in as no one");

        assert!(matches!(error, Error::Missing(Variable::User)), "{error}");
    }

    #[test]
    fn a_blank_password_is_kept_for_a_relay_that_takes_an_empty_one() {
        for password in ["", "  "] {
            let config = Config::from_variables(
                |variable| match variable {
                    Variable::Password => Some(password.to_string()),
                    other => configured(other),
                },
                SenderPolicy::Default,
            )
            .expect("an empty password is still a password");

            assert_eq!(config.relay().password.expose(), password);
        }
    }

    #[test]
    fn a_blank_sender_falls_back_to_the_default() {
        let config = Config::from_variables(
            |variable| match variable {
                Variable::From | Variable::FromName | Variable::Port => Some(String::new()),
                other => configured(other),
            },
            SenderPolicy::Default,
        )
        .expect("a blank sender is no sender");

        assert_eq!(config.relay().port, DEFAULT_PORT);
        assert_eq!(
            config.relay().sender,
            Sender::new(DEFAULT_SENDER).with_name(DEFAULT_SENDER_NAME)
        );
    }

    #[test]
    fn a_required_sender_is_named_when_it_is_missing_or_blank() {
        for from in [None, Some(" ".to_string())] {
            let error = Config::from_variables(
                |variable| match variable {
                    Variable::From => from.clone(),
                    other => configured(other),
                },
                SenderPolicy::Required,
            )
            .expect_err("no sender");

            assert!(
                matches!(error, Error::Missing(Variable::From)),
                "{from:?}: {error}"
            );
        }
    }

    #[test]
    fn values_are_trimmed_but_the_password_is_kept_as_given() {
        let config = Config::from_variables(
            |variable| match variable {
                Variable::Host => Some(" smtp.zone.test ".to_string()),
                Variable::Port => Some(" 2525 ".to_string()),
                Variable::User => Some(" zone ".to_string()),
                Variable::Password => Some(" spaced password ".to_string()),
                Variable::From => Some(" alerts@zone.test ".to_string()),
                Variable::FromName => Some(" Zone Alerts ".to_string()),
            },
            SenderPolicy::Required,
        )
        .expect("configured");
        let relay = config.relay();

        assert_eq!(relay.host, "smtp.zone.test");
        assert_eq!(relay.port, 2525);
        assert_eq!(relay.user, "zone");
        assert_eq!(relay.password.expose(), " spaced password ");
        assert_eq!(
            relay.sender,
            Sender::new("alerts@zone.test").with_name("Zone Alerts")
        );
    }

    #[test]
    fn an_unreadable_port_falls_back_to_the_default() {
        let config = Config::from_variables(
            |variable| match variable {
                Variable::Port => Some("submission".to_string()),
                other => configured(other),
            },
            SenderPolicy::Default,
        )
        .expect("a bad port is not a missing relay");

        assert_eq!(config.relay().port, DEFAULT_PORT);
    }

    #[test]
    fn the_password_never_appears_in_debug() {
        let config = Config::from_variables(configured, SenderPolicy::Default).expect("configured");
        let rendered = format!("{config:?}");

        assert!(!rendered.contains("hunter2"), "leaked: {rendered}");
        assert!(rendered.contains("smtp.zone.test"));
    }
}
