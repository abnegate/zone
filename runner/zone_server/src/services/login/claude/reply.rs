//! What claude.com sends a browser back to Zone's callback listener with: the code and state of
//! an approved sign-in, or why it was not approved.

use super::{Code, Refusal};

const CODE: &str = "code";
const STATE: &str = "state";
const ERROR: &str = "error";
const DESCRIPTION: &str = "error_description";

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Approved(Code),
    Refused { state: String, refusal: Refusal },
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
                refusal: Refusal::named(error, single(pairs, DESCRIPTION)),
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
    fn a_refusal_names_its_state_and_why() {
        for (mut query, refusal) in [
            (vec![("error", "access_denied")], Refusal::Declined),
            (vec![("error", "invalid_scope")], Refusal::Scope),
            (vec![("error", "server_error")], Refusal::Unavailable),
            (
                vec![
                    ("error", "access_denied"),
                    ("error_description", "account_on_hold"),
                ],
                Refusal::OnHold,
            ),
            (
                vec![
                    ("error", "access_denied"),
                    ("error_description", "<b>ignored</b>"),
                ],
                Refusal::Declined,
            ),
        ] {
            query.push(("state", "fake-state"));

            assert_eq!(
                read(&query),
                Some(Reply::Refused {
                    state: "fake-state".to_string(),
                    refusal,
                }),
                "{query:?}"
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
