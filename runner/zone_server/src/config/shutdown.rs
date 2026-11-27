use std::time::Duration;

use super::ConfigError;
use super::Environment;

pub const DELAY_VARIABLE: &str = "ZONE_SHUTDOWN_DELAY_SECONDS";
pub const GRACE_VARIABLE: &str = "ZONE_SHUTDOWN_GRACE_SECONDS";

/// How the server stops on SIGTERM: how long it keeps serving while load
/// balancers notice it is draining, then how long requests in flight get to
/// finish once it stops listening.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shutdown {
    pub delay: Duration,
    pub grace: Duration,
}

impl Shutdown {
    pub fn from_process_environment() -> Result<Self, ConfigError> {
        Self::from_lookup(|variable| std::env::var(variable).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let environment = Environment::new(lookup);
        Ok(Self {
            delay: seconds(
                environment.read(DELAY_VARIABLE),
                "ZONE_SHUTDOWN_DELAY_SECONDS must be a whole number of seconds",
            )?,
            grace: seconds(
                environment.read(GRACE_VARIABLE),
                "ZONE_SHUTDOWN_GRACE_SECONDS must be a whole number of seconds",
            )?,
        })
    }
}

fn seconds(value: Option<String>, refusal: &'static str) -> Result<Duration, ConfigError> {
    match value {
        None => Ok(Duration::ZERO),
        Some(value) => value
            .trim()
            .parse()
            .map(Duration::from_secs)
            .map_err(|_| ConfigError::Invalid(refusal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(variables: &[(&'static str, &'static str)]) -> Result<Shutdown, ConfigError> {
        Shutdown::from_lookup(|variable| {
            variables
                .iter()
                .find(|(name, _)| *name == variable)
                .map(|(_, value)| value.to_string())
        })
    }

    #[test]
    fn unset_or_blank_settings_stop_at_once() {
        assert_eq!(reading(&[]).expect("the defaults"), Shutdown::default());
        assert_eq!(
            reading(&[(DELAY_VARIABLE, " "), (GRACE_VARIABLE, "")]).expect("the defaults"),
            Shutdown {
                delay: Duration::ZERO,
                grace: Duration::ZERO,
            }
        );
    }

    #[test]
    fn both_settings_are_read_as_seconds() {
        assert_eq!(
            reading(&[(DELAY_VARIABLE, "10"), (GRACE_VARIABLE, " 30 ")]).expect("valid settings"),
            Shutdown {
                delay: Duration::from_secs(10),
                grace: Duration::from_secs(30),
            }
        );
    }

    #[test]
    fn a_setting_that_is_not_whole_seconds_stops_startup() {
        for value in ["-1", "1.5", "ten", "10s"] {
            for variable in [DELAY_VARIABLE, GRACE_VARIABLE] {
                let refused =
                    Shutdown::from_lookup(|name| (name == variable).then(|| value.into()));
                assert!(
                    matches!(refused, Err(ConfigError::Invalid(message)) if message.starts_with(variable)),
                    "{variable}={value:?} must be refused"
                );
            }
        }
    }
}
