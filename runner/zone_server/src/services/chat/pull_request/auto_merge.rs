use abnegate_vcs::PullRequestAutomation;
use serde::Serialize;

use super::mergeable::Mergeable;

/// Whether GitHub merges the pull request on its own once it can, and what
/// stops turning that on when nothing can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoMerge {
    pub enabled: bool,
    pub unavailable: Option<String>,
}

impl AutoMerge {
    pub const SETTLED: &str = "It is no longer open.";
    pub const NOT_ALLOWED: &str = "Auto-merge is turned off for this repository.";
    pub const DRAFT: &str = "Mark it ready for review first.";
    pub const MERGEABLE: &str = "It can be merged now.";
    pub const FORBIDDEN: &str = "This source's token cannot turn auto-merge on.";
    pub const LOCKED: &str = "This source's token cannot cancel its auto-merge.";
    pub const UNREAD: &str = "GitHub did not say whether it can merge on its own.";

    pub fn unavailable(reason: &str) -> Self {
        Self {
            enabled: false,
            unavailable: Some(reason.to_string()),
        }
    }

    /// Auto-merge as GitHub's `automation` describes it, on a pull request
    /// whose merge state is `mergeable`.
    pub fn read(automation: &PullRequestAutomation, mergeable: Mergeable) -> Self {
        let enabled = automation.auto_merge.is_some();
        let reason = if enabled {
            (!automation.can_disable_auto_merge).then_some(Self::LOCKED)
        } else if !automation.settings.auto_merge {
            Some(Self::NOT_ALLOWED)
        } else if automation.draft {
            Some(Self::DRAFT)
        } else if matches!(mergeable, Mergeable::Clean | Mergeable::Unstable) {
            Some(Self::MERGEABLE)
        } else if !automation.can_enable_auto_merge {
            Some(Self::FORBIDDEN)
        } else {
            None
        };
        Self {
            enabled,
            unavailable: reason.map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::{EnabledAutoMerge, MergeMethod, MergeSettings};

    use super::*;

    fn automation(allowed: bool) -> PullRequestAutomation {
        PullRequestAutomation::new(
            "PR_kwDOchat",
            MergeSettings::new(allowed, vec![MergeMethod::Squash]),
        )
        .with_can_enable_auto_merge(true)
        .with_can_disable_auto_merge(true)
    }

    fn reason(automation: &PullRequestAutomation, mergeable: Mergeable) -> Option<String> {
        AutoMerge::read(automation, mergeable).unavailable
    }

    #[test]
    fn a_blocked_pull_request_in_a_repository_that_allows_it_can_be_set_to_merge() {
        assert_eq!(
            AutoMerge::read(&automation(true), Mergeable::Blocked),
            AutoMerge {
                enabled: false,
                unavailable: None,
            }
        );
    }

    #[test]
    fn auto_merge_set_on_it_reads_as_enabled_and_can_be_cancelled() {
        let set =
            automation(true).with_auto_merge(EnabledAutoMerge::new(Some(MergeMethod::Squash)));
        assert_eq!(
            AutoMerge::read(&set, Mergeable::Blocked),
            AutoMerge {
                enabled: true,
                unavailable: None,
            }
        );
        let locked = set.with_can_disable_auto_merge(false);
        assert_eq!(
            AutoMerge::read(&locked, Mergeable::Blocked),
            AutoMerge {
                enabled: true,
                unavailable: Some(AutoMerge::LOCKED.to_string()),
            }
        );
    }

    #[test]
    fn each_reason_github_would_refuse_is_named_before_it_is_asked() {
        assert_eq!(
            reason(&automation(false), Mergeable::Blocked).as_deref(),
            Some(AutoMerge::NOT_ALLOWED)
        );
        assert_eq!(
            reason(&automation(true).with_draft(true), Mergeable::Draft).as_deref(),
            Some(AutoMerge::DRAFT)
        );
        for mergeable in [Mergeable::Clean, Mergeable::Unstable] {
            assert_eq!(
                reason(&automation(true), mergeable).as_deref(),
                Some(AutoMerge::MERGEABLE),
                "{mergeable:?}"
            );
        }
        assert_eq!(
            reason(
                &automation(true).with_can_enable_auto_merge(false),
                Mergeable::Blocked
            )
            .as_deref(),
            Some(AutoMerge::FORBIDDEN)
        );
    }

    #[test]
    fn a_repository_that_forbids_auto_merge_says_so_before_anything_else() {
        let everything = automation(false)
            .with_draft(true)
            .with_can_enable_auto_merge(false);
        assert_eq!(
            reason(&everything, Mergeable::Clean).as_deref(),
            Some(AutoMerge::NOT_ALLOWED)
        );
    }
}
