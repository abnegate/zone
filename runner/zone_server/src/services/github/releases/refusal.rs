use super::reconciliation::Reconciliation;

pub const LATEST_TAG_MOVED: &str = "latest_tag_moved";
pub const TAG_EXISTS: &str = "tag_exists";
pub const PREVIEW_STALE: &str = "preview_stale";
pub const RELEASE_IN_PROGRESS: &str = "release_in_progress";
pub const RECONCILIATION_REQUIRED: &str = "release_manual_reconciliation_required";
const NO_TAG: &str = "none";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    LatestTagMoved {
        expected: Option<String>,
        latest: Option<String>,
    },
    TagExists {
        tag: String,
        tag_object: Option<String>,
    },
    PreviewStale,
    InProgress,
    ReconciliationRequired(Reconciliation),
}

impl Refusal {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::LatestTagMoved { .. } => LATEST_TAG_MOVED,
            Self::TagExists { .. } => TAG_EXISTS,
            Self::PreviewStale => PREVIEW_STALE,
            Self::InProgress => RELEASE_IN_PROGRESS,
            Self::ReconciliationRequired(_) => RECONCILIATION_REQUIRED,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::LatestTagMoved { expected, latest } => format!(
                "The latest tag moved from {} to {}. Review the release again.",
                expected.as_deref().unwrap_or(NO_TAG),
                latest.as_deref().unwrap_or(NO_TAG)
            ),
            Self::TagExists { tag, .. } => {
                format!("The tag {tag} already exists. Choose another tag.")
            }
            Self::PreviewStale => {
                "What would be released changed since it was previewed. Review the release again."
                    .to_string()
            }
            Self::InProgress => {
                "A release is already being created for this source. Wait for it to finish."
                    .to_string()
            }
            Self::ReconciliationRequired(reconciliation) => reconciliation.message(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_is_coded_as_the_console_reads_it() {
        let reconciliation = Reconciliation {
            tag: "v1.4.1".into(),
            tag_object: "c".repeat(40),
            reference_created: false,
            release_id: None,
        };
        for (refusal, code) in [
            (
                Refusal::LatestTagMoved {
                    expected: None,
                    latest: Some("v1.4.0".into()),
                },
                "latest_tag_moved",
            ),
            (
                Refusal::TagExists {
                    tag: "v1.4.1".into(),
                    tag_object: None,
                },
                "tag_exists",
            ),
            (Refusal::PreviewStale, "preview_stale"),
            (Refusal::InProgress, "release_in_progress"),
            (
                Refusal::ReconciliationRequired(reconciliation.clone()),
                "release_manual_reconciliation_required",
            ),
        ] {
            assert_eq!(refusal.code(), code);
            assert!(!refusal.message().is_empty(), "{code}");
        }
        assert_eq!(
            Refusal::ReconciliationRequired(reconciliation.clone()).message(),
            reconciliation.message()
        );
    }

    #[test]
    fn a_moved_latest_tag_says_where_it_moved_from_and_to() {
        let message = Refusal::LatestTagMoved {
            expected: None,
            latest: Some("v1.4.0".into()),
        }
        .message();
        assert_eq!(
            message,
            "The latest tag moved from none to v1.4.0. Review the release again."
        );
    }
}
