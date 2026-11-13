#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reconciliation {
    pub(crate) tag: String,
    pub(crate) tag_object: String,
    pub(crate) reference_created: bool,
    pub(crate) release_id: Option<u64>,
}

impl Reconciliation {
    pub(crate) fn message(&self) -> String {
        let reference = if self.reference_created {
            format!("refs/tags/{} points at it", self.tag)
        } else {
            format!("refs/tags/{} was not confirmed created", self.tag)
        };
        let release = match self.release_id {
            Some(id) => format!("draft release {id} exists but was not confirmed published"),
            None => "no release was confirmed created".to_string(),
        };
        format!(
            "The release was only partly created and needs reconciling on GitHub: tag object {} for {} was created, {reference}, and {release}. Nothing was deleted.",
            self.tag_object, self.tag
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJECT: &str = "cccccccccccccccccccccccccccccccccccccccc";

    fn reconciliation(reference_created: bool, release_id: Option<u64>) -> Reconciliation {
        Reconciliation {
            tag: "v1.4.1".into(),
            tag_object: OBJECT.into(),
            reference_created,
            release_id,
        }
    }

    #[test]
    fn the_message_names_exactly_what_was_written() {
        let written = reconciliation(true, Some(91)).message();
        assert!(written.contains(OBJECT), "{written}");
        assert!(
            written.contains("refs/tags/v1.4.1 points at it"),
            "{written}"
        );
        assert!(written.contains("draft release 91"), "{written}");

        let tagged = reconciliation(false, None).message();
        assert!(tagged.contains("was not confirmed created"), "{tagged}");
        assert!(
            tagged.contains("no release was confirmed created"),
            "{tagged}"
        );
    }
}
