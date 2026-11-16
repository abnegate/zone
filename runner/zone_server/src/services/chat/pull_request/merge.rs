use abnegate_vcs::MergeMethod;

use super::mergeable::Mergeable;
use super::state::State;

/// Why a pull request cannot be merged now, if it cannot.
///
/// Only what GitHub would refuse whatever the token is stands here: a settled
/// pull request, a draft, a branch that conflicts, and a merge state nobody
/// could read. What branch protection might still refuse — a review it waits
/// on, a check that has not passed — is left for GitHub to answer when the
/// merge is asked for, because this reading can be a minute old and refusing
/// here would hide a merge GitHub would have taken.
pub struct Merge;

impl Merge {
    pub const SETTLED: &str = "It is no longer open.";
    pub const DRAFT: &str = "Mark it ready for review first.";
    pub const CONFLICTED: &str = "Its branch conflicts with its base; repair the conflict first.";
    pub const UNREAD: &str = "GitHub did not say whether it can be merged.";

    /// Why a merge cannot be asked for, on a pull request that is `state`,
    /// whose merge state GitHub summed up as `mergeable`, in a repository that
    /// allows `methods`.
    pub fn unavailable(
        state: State,
        draft: bool,
        mergeable: Mergeable,
        methods: &[MergeMethod],
    ) -> Option<String> {
        let reason = if state != State::Open {
            Self::SETTLED
        } else if draft {
            Self::DRAFT
        } else if mergeable == Mergeable::Conflicted {
            Self::CONFLICTED
        } else if mergeable == Mergeable::Unknown || methods.is_empty() {
            // An empty method list is what an unread repository leaves behind:
            // GitHub keeps at least one method on every repository.
            Self::UNREAD
        } else {
            return None;
        };
        Some(reason.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUASH: [MergeMethod; 1] = [MergeMethod::Squash];

    #[test]
    fn an_open_pull_request_with_a_method_can_be_merged() {
        for mergeable in [
            Mergeable::Clean,
            Mergeable::Blocked,
            Mergeable::Behind,
            Mergeable::Unstable,
        ] {
            assert_eq!(
                Merge::unavailable(State::Open, false, mergeable, &SQUASH),
                None,
                "{mergeable:?}"
            );
        }
    }

    #[test]
    fn each_reason_github_would_refuse_is_named_before_it_is_asked() {
        assert_eq!(
            Merge::unavailable(State::Merged, false, Mergeable::Clean, &SQUASH).as_deref(),
            Some(Merge::SETTLED)
        );
        assert_eq!(
            Merge::unavailable(State::Closed, false, Mergeable::Clean, &SQUASH).as_deref(),
            Some(Merge::SETTLED)
        );
        assert_eq!(
            Merge::unavailable(State::Open, true, Mergeable::Draft, &SQUASH).as_deref(),
            Some(Merge::DRAFT)
        );
        assert_eq!(
            Merge::unavailable(State::Open, false, Mergeable::Conflicted, &SQUASH).as_deref(),
            Some(Merge::CONFLICTED)
        );
        assert_eq!(
            Merge::unavailable(State::Open, false, Mergeable::Unknown, &SQUASH).as_deref(),
            Some(Merge::UNREAD)
        );
    }

    #[test]
    fn a_repository_whose_merge_methods_went_unread_is_not_offered_a_merge() {
        assert_eq!(
            Merge::unavailable(State::Open, false, Mergeable::Clean, &[]).as_deref(),
            Some(Merge::UNREAD)
        );
    }

    #[test]
    fn a_settled_pull_request_says_so_before_anything_else() {
        assert_eq!(
            Merge::unavailable(State::Merged, true, Mergeable::Conflicted, &[]).as_deref(),
            Some(Merge::SETTLED)
        );
    }
}
