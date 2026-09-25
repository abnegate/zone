//! A pending codex sign-in, as a status shows it.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zone_core::secret::REDACTED;

use super::super::codex;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub verification_url: String,
    /// Absent for a viewer who may not see it.
    pub user_code: Option<String>,
    pub expires_at: DateTime<Utc>,
}

impl Prompt {
    /// What codex printed, with its code only when `visible`.
    pub fn shown(prompt: &codex::Prompt, visible: bool) -> Self {
        Self {
            verification_url: prompt.verification_url.clone(),
            user_code: visible.then(|| prompt.user_code.clone()),
            expires_at: prompt.expires_at,
        }
    }
}

impl fmt::Debug for Prompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Prompt")
            .field("verification_url", &self.verification_url)
            .field("user_code", &self.user_code.as_ref().map(|_| REDACTED))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "ABCD-EFGHI";

    fn printed() -> codex::Prompt {
        codex::Prompt {
            verification_url: "https://auth.openai.com/codex/device".to_string(),
            user_code: CODE.to_string(),
            expires_at: DateTime::from_timestamp(1_790_149_096, 0).expect("a valid timestamp"),
        }
    }

    #[test]
    fn the_code_is_shown_only_when_visible() {
        let printed = printed();

        let shown = Prompt::shown(&printed, true);
        let hidden = Prompt::shown(&printed, false);

        assert_eq!(shown.user_code.as_deref(), Some(CODE));
        assert_eq!(hidden.user_code, None);
        for prompt in [shown, hidden] {
            assert_eq!(prompt.verification_url, printed.verification_url);
            assert_eq!(prompt.expires_at, printed.expires_at);
        }
    }

    #[test]
    fn the_code_never_reaches_debug_output() {
        let rendered = format!("{:?}", Prompt::shown(&printed(), true));

        assert!(!rendered.contains(CODE), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
    }
}
