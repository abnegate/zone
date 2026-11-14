use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::TagLine;
use super::line::FIRST_VERSION;
use super::line::Line;
use super::tag::Tag;

pub(crate) struct Catalogue {
    lines: Vec<Line>,
}

impl Catalogue {
    pub(crate) fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut grouped: BTreeMap<Option<String>, Vec<Tag>> = BTreeMap::new();
        for tag in names.into_iter().filter_map(Tag::parse) {
            grouped
                .entry(tag.prefix().map(str::to_string))
                .or_default()
                .push(tag);
        }
        let mut lines: Vec<Line> = grouped
            .into_iter()
            .map(|(prefix, tags)| Line::new(prefix, tags))
            .collect();
        lines.sort_by_key(|line| (Reverse(line.count()), line.prefix().map(str::to_string)));
        Self { lines }
    }

    pub(crate) fn latest(&self, prefix: Option<&str>) -> Option<&Tag> {
        self.line(prefix)?.latest()
    }

    pub(crate) fn latest_stable(&self) -> Option<&Tag> {
        self.lines
            .iter()
            .filter_map(Line::latest)
            .max_by(|left, right| left.version(right))
    }

    pub(crate) fn latest_tag(&self) -> Option<String> {
        self.dominant()?.latest().map(|tag| tag.name().to_string())
    }

    pub(crate) fn next_tag(&self) -> String {
        self.dominant()
            .map_or_else(|| FIRST_VERSION.to_string(), Line::next_tag)
    }

    pub(crate) fn previous_tags(&self) -> Vec<String> {
        self.dominant().map(Line::previous_tags).unwrap_or_default()
    }

    pub(crate) fn lines(&self) -> Vec<TagLine> {
        self.lines.iter().map(Line::offered).collect()
    }

    fn dominant(&self) -> Option<&Line> {
        self.lines.first()
    }

    fn line(&self, prefix: Option<&str>) -> Option<&Line> {
        self.lines.iter().find(|line| line.prefix() == prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_without_a_version_tag_starts_at_the_first() {
        let catalogue = Catalogue::new(["nightly", "release-candidate"]);
        assert_eq!(catalogue.latest_tag(), None);
        assert_eq!(catalogue.next_tag(), FIRST_VERSION);
        assert!(catalogue.previous_tags().is_empty());
        assert!(catalogue.lines().is_empty());
        assert!(catalogue.latest_stable().is_none());
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

    #[test]
    fn a_prefixed_repository_offers_each_line_and_its_busiest_as_the_default() {
        let catalogue = Catalogue::new([
            "abnegate-vcs/v0.2.1",
            "abnegate-vcs/v0.2.0",
            "abnegate-vcs/v0.3.0-rc.1",
            "abnegate-llm/v1.0.0",
        ]);

        assert_eq!(
            catalogue.latest_tag().as_deref(),
            Some("abnegate-vcs/v0.2.1")
        );
        assert_eq!(catalogue.next_tag(), "abnegate-vcs/v0.2.2");
        assert_eq!(
            catalogue.lines(),
            [
                TagLine {
                    prefix: Some("abnegate-vcs".into()),
                    latest_tag: Some("abnegate-vcs/v0.2.1".into()),
                    next_tag: "abnegate-vcs/v0.2.2".into(),
                    previous_tags: vec![
                        "abnegate-vcs/v0.3.0-rc.1".into(),
                        "abnegate-vcs/v0.2.1".into(),
                        "abnegate-vcs/v0.2.0".into(),
                    ],
                },
                TagLine {
                    prefix: Some("abnegate-llm".into()),
                    latest_tag: Some("abnegate-llm/v1.0.0".into()),
                    next_tag: "abnegate-llm/v1.0.1".into(),
                    previous_tags: vec!["abnegate-llm/v1.0.0".into()],
                },
            ]
        );
        assert_eq!(
            catalogue.latest(Some("abnegate-llm")).map(Tag::name),
            Some("abnegate-llm/v1.0.0")
        );
        assert_eq!(catalogue.latest(None).map(Tag::name), None);
        assert_eq!(
            catalogue.latest_stable().map(Tag::name),
            Some("abnegate-llm/v1.0.0")
        );
    }

    #[test]
    fn a_tie_between_lines_prefers_the_unprefixed_one() {
        let catalogue = Catalogue::new(["pkg/v3.0.0", "v1.0.0"]);

        assert_eq!(catalogue.latest_tag().as_deref(), Some("v1.0.0"));
        assert_eq!(catalogue.next_tag(), "v1.0.1");
        assert_eq!(
            catalogue.latest(Some("pkg")).map(Tag::name),
            Some("pkg/v3.0.0")
        );
    }
}
