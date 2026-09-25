//! What the sign-in panel can offer once a pasted Claude code has failed.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The paste could not be read. The sign-in still waits for its code.
    InvalidCode,
    /// The sign-in cannot finish, so it has to start again.
    StartAgain,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_kind_travels_as_the_sign_in_panel_spells_it() {
        for (kind, spelled) in [
            (Kind::InvalidCode, "invalid_code"),
            (Kind::StartAgain, "start_again"),
        ] {
            assert_eq!(
                serde_json::to_value(kind).expect("serialise"),
                json!(spelled)
            );
        }
    }
}
