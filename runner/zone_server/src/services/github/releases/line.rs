use super::TagLine;
use super::tag::Tag;
use super::tag::lead;

pub(crate) const FIRST_VERSION: &str = "v0.1.0";
const PREVIOUS_TAGS: usize = 10;

pub(crate) struct Line {
    prefix: Option<String>,
    tags: Vec<Tag>,
}

impl Line {
    pub(crate) fn new(prefix: Option<String>, mut tags: Vec<Tag>) -> Self {
        tags.sort_by(|left, right| right.cmp(left));
        tags.dedup();
        Self { prefix, tags }
    }

    pub(crate) fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub(crate) fn count(&self) -> usize {
        self.tags.len()
    }

    pub(crate) fn latest(&self) -> Option<&Tag> {
        self.tags.iter().find(|tag| tag.stable())
    }

    pub(crate) fn next_tag(&self) -> String {
        self.latest().map_or_else(
            || format!("{}{FIRST_VERSION}", lead(self.prefix())),
            Tag::next_patch,
        )
    }

    pub(crate) fn previous_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .take(PREVIOUS_TAGS)
            .map(|tag| tag.name().to_string())
            .collect()
    }

    pub(crate) fn offered(&self) -> TagLine {
        TagLine {
            prefix: self.prefix.clone(),
            latest_tag: self.latest().map(|tag| tag.name().to_string()),
            next_tag: self.next_tag(),
            previous_tags: self.previous_tags(),
        }
    }
}
