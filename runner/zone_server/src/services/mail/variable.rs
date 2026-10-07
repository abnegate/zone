//! The environment variables an SMTP relay is configured from.

/// One `SMTP_*` variable, shared by account mail and the email notification
/// channel so both read the same relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variable {
    Host,
    Port,
    User,
    Password,
    From,
    FromName,
}

impl Variable {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Host => "SMTP_HOST",
            Self::Port => "SMTP_PORT",
            Self::User => "SMTP_USER",
            Self::Password => "SMTP_PASSWORD",
            Self::From => "SMTP_FROM",
            Self::FromName => "SMTP_FROM_NAME",
        }
    }

    /// The variable's value in the process environment, when it is set.
    pub fn read(self) -> Option<String> {
        std::env::var(self.name()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variable_keeps_its_documented_name() {
        let names = [
            Variable::Host,
            Variable::Port,
            Variable::User,
            Variable::Password,
            Variable::From,
            Variable::FromName,
        ]
        .map(Variable::name);

        assert_eq!(
            names,
            [
                "SMTP_HOST",
                "SMTP_PORT",
                "SMTP_USER",
                "SMTP_PASSWORD",
                "SMTP_FROM",
                "SMTP_FROM_NAME",
            ]
        );
    }
}
