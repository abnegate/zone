use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reconciliation {
    pub(crate) tag: String,
    pub(crate) tag_object: String,
    pub(crate) reference_created: Option<bool>,
    pub(crate) release_id: Option<u64>,
    pub(crate) release_exists: bool,
    pub(crate) marker: Uuid,
}

impl Reconciliation {
    pub(crate) fn message(&self) -> String {
        let reference = match self.reference_created {
            Some(true) => format!("refs/tags/{} points at it", self.tag),
            Some(false) => format!("refs/tags/{} was not created", self.tag),
            None => format!("whether refs/tags/{} points at it is unknown", self.tag),
        };
        let release = match (self.release_id, self.release_exists) {
            (Some(id), _) => format!("draft release {id} exists but was not confirmed published"),
            (None, true) => format!("GitHub already holds a release for {}", self.tag),
            (None, false) => "no release was confirmed created".to_string(),
        };
        format!(
            "The release was only partly created and needs reconciling on GitHub: tag object {} for {} was created, {reference}, and {release}. Look for the marker zone-release:{}. Nothing was deleted.",
            self.tag_object, self.tag, self.marker
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJECT: &str = "cccccccccccccccccccccccccccccccccccccccc";

    fn reconciliation(reference_created: Option<bool>, release_id: Option<u64>) -> Reconciliation {
        Reconciliation {
            tag: "v1.4.1".into(),
            tag_object: OBJECT.into(),
            reference_created,
            release_id,
            release_exists: false,
            marker: Uuid::nil(),
        }
    }

    #[test]
    fn the_message_names_exactly_what_was_written() {
        let written = reconciliation(Some(true), Some(91)).message();
        assert!(written.contains(OBJECT), "{written}");
        assert!(
            written.contains("refs/tags/v1.4.1 points at it"),
            "{written}"
        );
        assert!(written.contains("draft release 91"), "{written}");
        assert!(written.contains(&Uuid::nil().to_string()), "{written}");

        let tagged = reconciliation(Some(false), None).message();
        assert!(tagged.contains("was not created"), "{tagged}");
        assert!(
            tagged.contains("no release was confirmed created"),
            "{tagged}"
        );

        let unknown = reconciliation(None, None).message();
        assert!(unknown.contains("is unknown"), "{unknown}");

        let taken = Reconciliation {
            release_exists: true,
            ..reconciliation(Some(true), None)
        }
        .message();
        assert!(taken.contains("already holds a release"), "{taken}");
    }
}
