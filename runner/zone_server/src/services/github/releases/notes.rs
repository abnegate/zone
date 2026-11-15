use std::collections::HashSet;

use regex::Regex;

use super::IncludedPull;

const LIST_MARKERS: [char; 2] = ['*', '-'];
const AUTHORED: &str = " by @";
const LINKED: &str = " in ";
const SCHEME: &str = "://";

pub(crate) struct Notes<'a> {
    body: &'a str,
}

impl<'a> Notes<'a> {
    pub(crate) fn new(body: &'a str) -> Self {
        Self { body }
    }

    pub(crate) fn pulls(&self, repository_url: &str) -> Vec<IncludedPull> {
        let repository_url = repository_url.trim_end_matches('/');
        let Ok(pattern) = Regex::new(&format!(
            r"(?i){}/pull/(\d+)",
            regex::escape(repository_url)
        )) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut pulls = Vec::new();
        for line in self.body.lines() {
            for captures in pattern.captures_iter(line) {
                let Some(number) = captures
                    .get(1)
                    .and_then(|number| number.as_str().parse::<u64>().ok())
                    .filter(|number| *number > 0)
                else {
                    continue;
                };
                if seen.insert(number) {
                    pulls.push(IncludedPull {
                        number,
                        url: format!("{repository_url}/pull/{number}"),
                        title: title(line, number),
                    });
                }
            }
        }
        pulls
    }
}

fn title(line: &str, number: u64) -> String {
    let item = line.trim().trim_start_matches(LIST_MARKERS).trim();
    let title = item
        .rsplit_once(AUTHORED)
        .or_else(|| item.rsplit_once(LINKED))
        .map_or("", |(title, _)| title.trim());
    if title.is_empty() || title.contains(SCHEME) {
        format!("#{number}")
    } else {
        title.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPOSITORY_URL: &str = "https://github.com/acme/widgets";

    fn pulls(body: &str) -> Vec<(u64, String, String)> {
        Notes::new(body)
            .pulls(REPOSITORY_URL)
            .into_iter()
            .map(|pull| (pull.number, pull.title, pull.url))
            .collect()
    }

    fn pull(number: u64, title: &str) -> (u64, String, String) {
        (
            number,
            title.to_string(),
            format!("{REPOSITORY_URL}/pull/{number}"),
        )
    }

    #[test]
    fn generated_notes_name_each_pull_request_once_with_its_title() {
        let body = "## What's Changed\n\
            * Add widgets by @octo in https://github.com/acme/widgets/pull/12\n\
            * Fix the by-line in titles by @cat in https://github.com/acme/widgets/pull/13\n\
            \n\
            ## New Contributors\n\
            * @cat made their first contribution in https://github.com/acme/widgets/pull/13\n\
            \n\
            **Full Changelog**: https://github.com/acme/widgets/compare/v1.3.0...v1.4.0";
        assert_eq!(
            pulls(body),
            [
                pull(12, "Add widgets"),
                pull(13, "Fix the by-line in titles")
            ]
        );
    }

    #[test]
    fn a_pull_request_of_another_repository_is_not_this_releases() {
        let body = "* Borrowed by @octo in https://github.com/acme/gadgets/pull/7\n\
            * Mine by @octo in https://github.com/acme/widgets-extra/pull/8\n\
            * Listed by @octo in https://github.com/acme/widgets/pulls";
        assert!(pulls(body).is_empty());
    }

    #[test]
    fn a_bare_link_is_titled_by_its_number_and_case_does_not_hide_it() {
        let body =
            "https://GitHub.com/Acme/Widgets/pull/21 and https://github.com/acme/widgets/pull/0";
        assert_eq!(pulls(body), [pull(21, "#21")]);
    }

    #[test]
    fn a_line_naming_two_pull_requests_keeps_both() {
        let body = "- Squash https://github.com/acme/widgets/pull/3 and https://github.com/acme/widgets/pull/4";
        let numbers: Vec<u64> = pulls(body).into_iter().map(|(number, ..)| number).collect();
        assert_eq!(numbers, [3, 4]);
    }
}
