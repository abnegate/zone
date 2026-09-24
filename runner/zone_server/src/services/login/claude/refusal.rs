//! Why claude.com did not approve a sign-in, as far as it matters for what to do next.

const ACCOUNT_ON_HOLD: &str = "account_on_hold";
const INVALID_SCOPE: &str = "invalid_scope";
const SERVER_ERROR: &str = "server_error";
const TEMPORARILY_UNAVAILABLE: &str = "temporarily_unavailable";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The person declined, or claude.com refused for a reason starting again can fix.
    Declined,
    /// claude.com would not grant the access Zone asked for.
    Scope,
    /// The Claude account is on hold, so no sign-in will succeed until it is resolved.
    OnHold,
    /// claude.com could not answer just now.
    Unavailable,
}

impl Refusal {
    /// The refusal an `error` and its `error_description` name. claude.com marks an account on
    /// hold in the description, as the claude CLI reads it.
    pub fn named(error: &str, description: Option<&str>) -> Self {
        if description == Some(ACCOUNT_ON_HOLD) {
            return Self::OnHold;
        }
        match error {
            INVALID_SCOPE => Self::Scope,
            SERVER_ERROR | TEMPORARILY_UNAVAILABLE => Self::Unavailable,
            _ => Self::Declined,
        }
    }

    /// What the admin should do, in Zone's own words.
    pub fn explained(self) -> &'static str {
        match self {
            Self::Declined => "Claude did not approve the sign-in. Start again.",
            Self::Scope => {
                "claude.com would not grant the access Zone asked for. Try again with full access."
            }
            Self::OnHold => {
                "Your Claude account is on hold, so it cannot sign in to Claude Code. See why, or \
                 appeal, at claude.ai/restricted."
            }
            Self::Unavailable => {
                "claude.com could not finish the sign-in just now. Try again in a few minutes."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_says_what_to_do_instead_of_starting_the_same_sign_in_again() {
        for (error, description, refusal) in [
            ("access_denied", None, Refusal::Declined),
            ("invalid_request", Some("anything"), Refusal::Declined),
            ("invalid_scope", None, Refusal::Scope),
            ("server_error", None, Refusal::Unavailable),
            ("temporarily_unavailable", None, Refusal::Unavailable),
            ("access_denied", Some("account_on_hold"), Refusal::OnHold),
            ("server_error", Some("account_on_hold"), Refusal::OnHold),
        ] {
            assert_eq!(
                Refusal::named(error, description),
                refusal,
                "{error} {description:?}"
            );
        }
        for refusal in [Refusal::OnHold, Refusal::Unavailable] {
            assert!(
                !refusal.explained().contains("Start again"),
                "{refusal:?} sends the admin round the same loop"
            );
        }
    }
}
