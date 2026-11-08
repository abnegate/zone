//! What an admin should do about a sign-in claude.com did not approve, in Zone's own words.

use abnegate_agent_cli::login::claude::Refusal;

const DECLINED: &str = "Claude did not approve the sign-in. Start again.";
const SCOPE: &str =
    "claude.com would not grant the access Zone asked for. Try again with full access.";
const ON_HOLD: &str = "Your Claude account is on hold, so it cannot sign in to Claude Code. See \
                       why, or appeal, at claude.ai/restricted.";
const UNAVAILABLE: &str =
    "claude.com could not finish the sign-in just now. Try again in a few minutes.";

pub trait Explained {
    /// What the admin should do next.
    fn explained(self) -> &'static str;
}

impl Explained for Refusal {
    fn explained(self) -> &'static str {
        match self {
            Self::Declined => DECLINED,
            Self::Scope => SCOPE,
            Self::OnHold => ON_HOLD,
            Self::Unavailable => UNAVAILABLE,
            _ => DECLINED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_is_explained_in_zones_words() {
        for (refusal, explained) in [
            (Refusal::Declined, DECLINED),
            (Refusal::Scope, SCOPE),
            (Refusal::OnHold, ON_HOLD),
            (Refusal::Unavailable, UNAVAILABLE),
        ] {
            assert_eq!(refusal.explained(), explained, "{refusal:?}");
        }
    }

    #[test]
    fn each_refusal_says_what_to_do_instead_of_starting_the_same_sign_in_again() {
        for refusal in [Refusal::OnHold, Refusal::Unavailable] {
            assert!(
                !refusal.explained().contains("Start again"),
                "{refusal:?} sends the admin round the same loop"
            );
        }
    }
}
