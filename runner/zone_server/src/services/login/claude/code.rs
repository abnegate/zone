//! The code a user pastes back from Claude's sign-in page, or from the address bar of a
//! browser claude.com sent to a callback it could not reach.

use std::str::FromStr;

use reqwest::Url;
use zone_core::secret::SecretValue;

use super::{CALLBACK_PATH, Error, LOOPBACK_HOST, LOOPBACK_SCHEME, REDIRECT_URL};

const SEPARATOR: char = '#';
const UNREADABLE: &str = "Paste the code Claude showed, as code#state, or the whole callback URL";
const FOREIGN: &str = "That URL is not Claude's sign-in callback";
const DECLINED: &str =
    "Claude did not approve the sign-in. Open the link, approve it, then paste the new code";
const INCOMPLETE: &str = "The callback URL is missing its code or its state";

#[derive(Debug, PartialEq, Eq)]
pub struct Code {
    pub value: SecretValue,
    pub state: String,
}

impl FromStr for Code {
    type Err = Error;

    fn from_str(pasted: &str) -> Result<Self, Error> {
        let pasted = pasted.trim();
        match Url::parse(pasted) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => Self::from_callback(&url),
            _ => {
                let (value, state) = pasted
                    .split_once(SEPARATOR)
                    .ok_or(Error::Malformed(UNREADABLE))?;
                Self::new(value, state)
            }
        }
    }
}

impl Code {
    /// A code and the state it answers, each non-empty and free of whitespace and `#`.
    pub fn new(value: &str, state: &str) -> Result<Self, Error> {
        let clean = |part: &str| {
            !part.is_empty() && !part.contains(SEPARATOR) && !part.contains(char::is_whitespace)
        };
        if !clean(value) || !clean(state) {
            return Err(Error::Malformed(UNREADABLE));
        }
        Ok(Self {
            value: SecretValue::new(value),
            state: state.to_string(),
        })
    }

    fn from_callback(url: &Url) -> Result<Self, Error> {
        let mut callback = url.clone();
        callback.set_query(None);
        callback.set_fragment(None);
        let loopback = callback.scheme() == LOOPBACK_SCHEME
            && callback.host_str() == Some(LOOPBACK_HOST)
            && callback.port().is_some()
            && callback.path() == CALLBACK_PATH
            && callback.username().is_empty()
            && callback.password().is_none();
        if callback.as_str() != REDIRECT_URL && !loopback {
            return Err(Error::Malformed(FOREIGN));
        }

        let parameter = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        if parameter("error").is_some() {
            return Err(Error::Malformed(DECLINED));
        }
        match (parameter("code"), parameter("state")) {
            (Some(value), Some(state)) => Self::new(&value, &state),
            _ => Err(Error::Malformed(INCOMPLETE)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASTED_URL: &str =
        "https://platform.claude.com/oauth/code/callback?code=fake-code&state=fake-state";

    fn code() -> Code {
        Code {
            value: SecretValue::new("fake-code"),
            state: "fake-state".to_string(),
        }
    }

    fn refusal(pasted: &str) -> &'static str {
        match pasted.parse::<Code>() {
            Err(Error::Malformed(message)) => message,
            other => panic!("{pasted:?} must be refused as malformed, got {other:?}"),
        }
    }

    #[test]
    fn a_pasted_code_splits_into_its_code_and_state() {
        assert_eq!(
            "fake-code#fake-state".parse::<Code>().expect("parses"),
            code()
        );
    }

    #[test]
    fn a_pasted_callback_url_gives_up_its_code_and_state() {
        assert_eq!(PASTED_URL.parse::<Code>().expect("parses"), code());
        assert_eq!(
            format!("{PASTED_URL}#fragment")
                .replace("https://platform", "HTTPS://PLATFORM")
                .parse::<Code>()
                .expect("scheme and host case do not matter"),
            code()
        );
    }

    #[test]
    fn a_paste_without_a_separator_or_with_an_empty_half_is_refused() {
        for pasted in [
            "fake-code",
            "#fake-state",
            "fake-code#",
            "#",
            "fake-code#fake-state#extra",
        ] {
            assert_eq!(refusal(pasted), UNREADABLE, "{pasted:?}");
        }
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_and_inner_whitespace_refused() {
        assert_eq!(
            "  fake-code#fake-state\n"
                .parse::<Code>()
                .expect("the paste is trimmed"),
            code()
        );
        for pasted in [
            "fake-code #fake-state",
            "fake code#fake-state",
            "fake-code#fake\tstate",
            "",
            " \n\t ",
        ] {
            assert_eq!(refusal(pasted), UNREADABLE, "{pasted:?}");
        }
    }

    #[test]
    fn the_address_of_a_callback_the_browser_could_not_reach_gives_up_its_code_and_state() {
        for pasted in [
            "http://localhost:54545/callback?code=fake-code&state=fake-state",
            "http://LOCALHOST:1/callback?state=fake-state&code=fake-code#fragment",
        ] {
            assert_eq!(pasted.parse::<Code>().expect("parses"), code(), "{pasted}");
        }
    }

    #[test]
    fn a_url_other_than_claudes_callback_is_refused() {
        for pasted in [
            "https://attacker.example/oauth/code/callback?code=fake-code&state=fake-state",
            "http://platform.claude.com/oauth/code/callback?code=fake-code&state=fake-state",
            "https://platform.claude.com.attacker.example/oauth/code/callback?code=fake-code&state=fake-state",
            "https://platform.claude.com/oauth/code/callback/more?code=fake-code&state=fake-state",
            "https://someone@platform.claude.com/oauth/code/callback?code=fake-code&state=fake-state",
            "https://localhost:54545/callback?code=fake-code&state=fake-state",
            "http://127.0.0.1:54545/callback?code=fake-code&state=fake-state",
            "http://localhost/callback?code=fake-code&state=fake-state",
            "http://localhost:54545/other?code=fake-code&state=fake-state",
            "http://someone@localhost:54545/callback?code=fake-code&state=fake-state",
            "http://localhost.attacker.example:54545/callback?code=fake-code&state=fake-state",
        ] {
            assert_eq!(refusal(pasted), FOREIGN, "{pasted}");
        }
    }

    #[test]
    fn a_callback_without_a_code_is_refused_and_a_declined_one_says_so() {
        assert_eq!(
            refusal("https://platform.claude.com/oauth/code/callback?state=fake-state"),
            INCOMPLETE
        );
        assert_eq!(
            refusal(
                "https://platform.claude.com/oauth/code/callback\
                 ?error=access_denied&state=fake-state"
            ),
            DECLINED
        );
    }

    /// A declined callback is refused before any state is claimed, so the
    /// sign-in still waits on the same link.
    #[test]
    fn a_declined_callback_asks_for_the_same_link_to_be_approved() {
        let message = refusal(
            "https://platform.claude.com/oauth/code/callback\
             ?error=access_denied&state=fake-state",
        )
        .to_ascii_lowercase();

        assert!(!message.contains("start"), "{message}");
        for step in ["open the link", "approve it", "paste the new code"] {
            assert!(message.contains(step), "{step:?} missing from {message:?}");
        }
    }
}
