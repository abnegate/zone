//! The sign-in link and one-time code that `codex login --device-auth` prints on stdout.

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use chrono::{DateTime, TimeDelta, Utc};
use regex::Regex;
use zone_core::secret::REDACTED;

pub(super) const NO_LINK: &str = "no sign-in link";
pub(super) const NO_CODE: &str = "no one-time code";
pub(super) const NO_EXPIRY: &str = "no expiry for its one-time code";

static ESCAPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("the escape pattern is a valid regex")
});
static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(https?://[^\s]+)[ \t]*$").expect("the link pattern is a valid regex")
});
static CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*([A-Z0-9]+(?:-[A-Z0-9]+)+)[ \t]*$")
        .expect("the code pattern is a valid regex")
});
static EXPIRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"expires in (\d+) minutes").expect("the expiry pattern is a valid regex")
});

#[derive(Clone, PartialEq, Eq)]
pub struct Prompt {
    pub verification_url: String,
    pub user_code: String,
    pub expires_at: DateTime<Utc>,
}

impl Prompt {
    /// The prompt in `text`, codex's stdout with its colour codes stripped, as read at `now`.
    /// Until all of it has arrived, the first part codex has not printed yet.
    ///
    /// Only a line after the link can be the code: codex always prints the link first.
    pub(super) fn read(text: &str, now: DateTime<Utc>) -> Result<Self, &'static str> {
        let link = LINK.captures(text).ok_or(NO_LINK)?;
        let after = link.get(0).map_or(0, |whole| whole.end());
        let code = CODE.captures_at(text, after).ok_or(NO_CODE)?;
        let expires_at = EXPIRY
            .captures(text)
            .and_then(|expiry| expiry[1].parse::<i64>().ok())
            .and_then(TimeDelta::try_minutes)
            .and_then(|lifetime| now.checked_add_signed(lifetime))
            .ok_or(NO_EXPIRY)?;
        Ok(Self {
            verification_url: link[1].to_string(),
            user_code: code[1].to_string(),
            expires_at,
        })
    }
}

/// Whoever holds the code can finish the sign-in with their own account.
impl fmt::Debug for Prompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Prompt")
            .field("verification_url", &self.verification_url)
            .field("user_code", &REDACTED)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// `text` without the colour codes codex prints even when its output is not a terminal.
pub(crate) fn strip(text: &str) -> Cow<'_, str> {
    ESCAPE.replace_all(text, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::login::codex::testing::{
        EXCHANGE_FAILED, EXCHANGE_FAILED_PROMPT, NO_COLOR_PROMPT, POLL_FAILED, POLL_FAILED_PROMPT,
        PROMPT, REFUSED, RELOGIN_ABANDONED_PROMPT, SUCCESS, SUCCESS_PROMPT, TERM_DUMB_PROMPT,
        THROTTLED, UNSUPPORTED,
    };

    fn stripped(raw: &[u8]) -> String {
        String::from_utf8_lossy(raw)
            .lines()
            .map(|line| format!("{}\n", strip(line)))
            .collect()
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_148_196, 0).expect("a valid timestamp")
    }

    #[test]
    fn the_link_code_and_expiry_are_read_from_codexs_own_prompt() {
        let prompt = Prompt::read(&stripped(PROMPT), now()).expect("a whole prompt");

        assert_eq!(
            prompt.verification_url,
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(prompt.user_code, "ABCD-EFGHI");
        assert_eq!(prompt.expires_at, now() + TimeDelta::minutes(15));
    }

    #[test]
    fn every_recorded_prompt_reads_the_same_whatever_the_terminal() {
        for raw in [
            SUCCESS_PROMPT,
            NO_COLOR_PROMPT,
            TERM_DUMB_PROMPT,
            POLL_FAILED_PROMPT,
            EXCHANGE_FAILED_PROMPT,
            RELOGIN_ABANDONED_PROMPT,
        ] {
            let prompt = Prompt::read(&stripped(raw), now()).expect("a whole prompt");

            assert!(
                prompt.verification_url.starts_with("http://127.0.0.1:")
                    && prompt.verification_url.ends_with("/codex/device"),
                "not the fake issuer's link: {}",
                prompt.verification_url
            );
            assert_eq!(prompt.user_code, "FXTR-9Z9Z9");
            assert_eq!(prompt.expires_at, now() + TimeDelta::minutes(15));
        }
    }

    #[test]
    fn colour_codes_are_stripped_from_the_link() {
        let raw = String::from_utf8_lossy(PROMPT);
        let line = raw
            .lines()
            .find(|line| line.contains("https://"))
            .expect("the link line");

        assert!(line.contains('\u{1b}'), "the capture lost its colour codes");
        assert_eq!(strip(line), "   https://auth.openai.com/codex/device");
    }

    #[test]
    fn a_prompt_still_arriving_names_the_part_that_is_missing() {
        let full = stripped(PROMPT);
        let through = |needle: &str| {
            let end = full.find(needle).expect("the needle") + needle.len();
            full[..end].to_string()
        };

        assert_eq!(Prompt::read("", now()), Err(NO_LINK));
        assert_eq!(
            Prompt::read(&through("codex/device\n"), now()),
            Err(NO_CODE)
        );
        assert_eq!(
            Prompt::read(
                &full.replace("expires in 15 minutes", "expires soon"),
                now()
            ),
            Err(NO_EXPIRY)
        );
        assert_eq!(
            Prompt::read(
                "   ABCD-EFGHI\n   https://auth.openai.com/codex/device\n",
                now()
            ),
            Err(NO_CODE),
            "a code printed before the link was taken for the code"
        );
    }

    #[test]
    fn nothing_else_codex_prints_reads_as_a_prompt() {
        for raw in [
            SUCCESS,
            REFUSED,
            UNSUPPORTED,
            THROTTLED,
            POLL_FAILED,
            EXCHANGE_FAILED,
        ] {
            assert_eq!(
                Prompt::read(&stripped(raw), now()),
                Err(NO_LINK),
                "{}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn an_expiry_too_large_to_represent_is_no_expiry() {
        let full = stripped(PROMPT).replace(
            "expires in 15 minutes",
            "expires in 99999999999999999 minutes",
        );

        assert_eq!(Prompt::read(&full, now()), Err(NO_EXPIRY));
    }

    #[test]
    fn the_code_never_reaches_debug_output() {
        let prompt = Prompt::read(&stripped(PROMPT), now()).expect("a whole prompt");

        let rendered = format!("{prompt:?}");

        assert!(!rendered.contains("ABCD-EFGHI"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
    }
}
