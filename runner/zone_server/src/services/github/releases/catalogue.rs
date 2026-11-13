use super::tag::Tag;

pub(crate) const FIRST_TAG: &str = "v0.1.0";
const PREVIOUS_TAGS: usize = 10;

pub(crate) struct Catalogue {
    tags: Vec<Tag>,
}

impl Catalogue {
    pub(crate) fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut tags: Vec<Tag> = names.into_iter().filter_map(Tag::parse).collect();
        tags.sort_by(|left, right| right.cmp(left));
        tags.dedup();
        Self { tags }
    }

    pub(crate) fn latest(&self) -> Option<&Tag> {
        self.tags.iter().find(|tag| tag.stable())
    }

    pub(crate) fn latest_tag(&self) -> Option<String> {
        self.latest().map(|tag| tag.name().to_string())
    }

    pub(crate) fn next_tag(&self) -> String {
        self.latest()
            .map_or_else(|| FIRST_TAG.to_string(), Tag::next_patch)
    }

    pub(crate) fn previous_tags(&self) -> Vec<String> {
        self.tags
            .iter()
            .take(PREVIOUS_TAGS)
            .map(|tag| tag.name().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_without_a_version_tag_starts_at_the_first() {
        let catalogue = Catalogue::new(["nightly", "release-candidate"]);
        assert_eq!(catalogue.latest_tag(), None);
        assert_eq!(catalogue.next_tag(), FIRST_TAG);
        assert!(catalogue.previous_tags().is_empty());
    }

    #[test]
    fn the_next_tag_bumps_the_highest_stable_version() {
        let catalogue = Catalogue::new(["v1.2.3", "v1.10.0", "v1.11.0-rc.1", "v1.9.9", "nightly"]);
        assert_eq!(catalogue.latest_tag().as_deref(), Some("v1.10.0"));
        assert_eq!(catalogue.next_tag(), "v1.10.1");
    }

    #[test]
    fn an_unprefixed_line_keeps_its_bare_versions() {
        let catalogue = Catalogue::new(["2.0.0", "1.0.0"]);
        assert_eq!(catalogue.next_tag(), "2.0.1");
    }

    #[test]
    fn the_previous_tags_are_the_ten_highest_versions_newest_first() {
        let names: Vec<String> = (0..12).map(|patch| format!("v1.0.{patch}")).collect();
        let mut listed: Vec<&str> = names.iter().map(String::as_str).collect();
        listed.push("v1.0.11");
        listed.push("v1.1.0-rc.1");
        let catalogue = Catalogue::new(listed);
        let expected: Vec<String> = std::iter::once("v1.1.0-rc.1".to_string())
            .chain((3..12).rev().map(|patch| format!("v1.0.{patch}")))
            .collect();
        assert_eq!(catalogue.previous_tags(), expected);
    }
}
