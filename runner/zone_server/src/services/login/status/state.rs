//! Whether an organization can use a coding agent right now.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    SignedIn,
    SignedOut,
    /// A codex device sign-in is waiting for someone to enter its code.
    Pending,
    /// Zone keeps a sign-in for the organization that can no longer be used.
    Expired,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_state_travels_as_the_sign_in_panel_spells_it() {
        for (state, spelled) in [
            (State::SignedIn, "signed_in"),
            (State::SignedOut, "signed_out"),
            (State::Pending, "pending"),
            (State::Expired, "expired"),
        ] {
            assert_eq!(
                serde_json::to_value(state).expect("serialise"),
                json!(spelled)
            );
            assert_eq!(
                serde_json::from_value::<State>(json!(spelled)).expect("deserialise"),
                state
            );
        }
    }
}
