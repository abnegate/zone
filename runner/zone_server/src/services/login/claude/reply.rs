//! What claude.com sends a browser back to Zone's callback listener with: the code and state of
//! an approved sign-in, or why it was not approved.

use super::Code;

const CODE: &str = "code";
const STATE: &str = "state";
const ERROR: &str = "error";
const INVALID_SCOPE: &str = "invalid_scope";

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Approved(Code),
    Refused { state: String, refusal: Refusal },
}

/// Why claude.com did not approve a sign-in, as far as Zone tells the reasons apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The person declined, or claude.com refused for a reason Zone cannot act on.
    Declined,
    /// claude.com would not grant the access Zone asked for.
    Scope,
}

impl Reply {
    /// The reply in a callback's query, decoded into `pairs`. `None` when it is not one claude.com
    /// sends: a state and either a code or an error, each named once.
    pub fn read(pairs: &[(String, String)]) -> Option<Self> {
        let state = single(pairs, STATE)?;
        match (single(pairs, CODE), single(pairs, ERROR)) {
            (Some(code), None) => Code::new(code, state).ok().map(Self::Approved),
            (None, Some(error)) => Some(Self::Refused {
                state: state.to_string(),
                refusal: if error == INVALID_SCOPE {
                    Refusal::Scope
                } else {
                    Refusal::Declined
                },
            }),
            _ => None,
        }
    }

    pub fn state(&self) -> &str {
        match self {
            Self::Approved(code) => &code.state,
            Self::Refused { state, .. } => state,
        }
    }
}

/// The value of `name` when it appears exactly once.
fn single<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let mut values = pairs
        .iter()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.as_str());
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use zone_core::secret::SecretValue;

    use super::*;

    fn pairs(query: &[(&str, &str)]) -> Vec<(String, String)> {
        query
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn read(query: &[(&str, &str)]) -> Option<Reply> {
        Reply::read(&pairs(query))
    }

    #[test]
    fn an_approved_sign_in_carries_its_code_and_state() {
        let reply = read(&[("code", "fake-code"), ("state", "fake-state")]).expect("a reply");

        assert_eq!(
            reply,
            Reply::Approved(Code {
                value: SecretValue::new("fake-code"),
                state: "fake-state".to_string(),
            })
        );
        assert_eq!(reply.state(), "fake-state");
    }

    #[test]
    fn a_refusal_names_its_state_and_whether_the_scope_was_the_reason() {
        for (error, refusal) in [
            ("access_denied", Refusal::Declined),
            ("invalid_scope", Refusal::Scope),
            ("server_error", Refusal::Declined),
        ] {
            let reply = read(&[
                ("error", error),
                ("error_description", "<b>ignored</b>"),
                ("state", "fake-state"),
            ])
            .expect("a refusal");

            assert_eq!(
                reply,
                Reply::Refused {
                    state: "fake-state".to_string(),
                    refusal,
                },
                "{error}"
            );
        }
    }

    #[test]
    fn a_reply_claude_never_sends_is_unreadable() {
        for query in [
            vec![],
            vec![("code", "fake-code")],
            vec![("state", "fake-state")],
            vec![("error", "access_denied")],
            vec![
                ("code", "fake-code"),
                ("error", "access_denied"),
                ("state", "fake-state"),
            ],
            vec![
                ("code", "fake-code"),
                ("state", "fake-state"),
                ("state", "other-state"),
            ],
            vec![
                ("code", "fake-code"),
                ("code", "other-code"),
                ("state", "fake-state"),
            ],
            vec![("code", ""), ("state", "fake-state")],
            vec![("code", "fake-code"), ("state", "")],
            vec![("code", "fake code"), ("state", "fake-state")],
            vec![("code", "fake-code"), ("state", "fake#state")],
        ] {
            assert_eq!(read(&query), None, "{query:?}");
        }
    }
}
