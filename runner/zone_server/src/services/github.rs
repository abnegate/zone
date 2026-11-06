//! How Zone words what GitHub said in refusing a request.

use abnegate_vcs::PullRequestError;
use reqwest::StatusCode;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

/// What GitHub said in refusing a request, worded to follow "GitHub refused:".
/// A status `abnegate-vcs` does not classify reads as the status and its
/// standard reason, then GitHub's words after a colon, without the crate's own
/// "GitHub answered with status" in front; every other error reads as the
/// crate words it.
pub struct Refusal<'a>(pub &'a PullRequestError);

impl Display for Refusal<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let PullRequestError::Refused {
            status, message, ..
        } = self.0
        else {
            return write!(formatter, "{}", self.0);
        };
        write!(formatter, "{status}")?;
        if let Some(reason) = StatusCode::from_u16(*status)
            .ok()
            .and_then(|code| code.canonical_reason())
        {
            write!(formatter, " {reason}")?;
        }
        if !message.is_empty() {
            write!(formatter, ": {message}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_its_status_reason_and_github_s_words_once() {
        assert_eq!(
            Refusal(&PullRequestError::refused(422, "Validation Failed")).to_string(),
            "422 Unprocessable Entity: Validation Failed"
        );
        assert_eq!(
            Refusal(&PullRequestError::refused(502, "")).to_string(),
            "502 Bad Gateway"
        );
        assert_eq!(
            Refusal(&PullRequestError::refused(599, "Odd")).to_string(),
            "599: Odd"
        );
        assert_eq!(
            Refusal(&PullRequestError::refused(599, "")).to_string(),
            "599"
        );
    }

    #[test]
    fn any_other_error_reads_as_the_crate_words_it() {
        assert_eq!(
            Refusal(&PullRequestError::Forbidden).to_string(),
            PullRequestError::Forbidden.to_string()
        );
    }
}
