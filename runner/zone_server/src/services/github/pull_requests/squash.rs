use super::Commit;

const CO_AUTHOR: &str = "co-authored-by:";
const BULLET: &str = "* ";
const PARAGRAPH: &str = "\n\n";
const LINE: &str = "\n";

pub fn message(commits: &[Commit]) -> String {
    let mut trailers: Vec<&str> = Vec::new();
    let mut entries = Vec::with_capacity(commits.len());
    for commit in commits {
        let mut kept = Vec::new();
        for line in commit.message.lines() {
            match co_author(line) {
                Some(trailer) => {
                    if !trailers
                        .iter()
                        .any(|held| held.eq_ignore_ascii_case(trailer))
                    {
                        trailers.push(trailer);
                    }
                }
                None => kept.push(line),
            }
        }
        let text = kept.join(LINE);
        let text = text.trim();
        if !text.is_empty() {
            entries.push(format!("{BULLET}{text}"));
        }
    }
    let body = entries.join(PARAGRAPH);
    match (body.is_empty(), trailers.is_empty()) {
        (_, true) => body,
        (true, false) => trailers.join(LINE),
        (false, false) => format!("{body}{PARAGRAPH}{}", trailers.join(LINE)),
    }
}

fn co_author(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    trimmed
        .get(..CO_AUTHOR.len())
        .is_some_and(|key| key.eq_ignore_ascii_case(CO_AUTHOR))
        .then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(message: &str) -> Commit {
        Commit {
            sha: "a".repeat(40),
            message: message.into(),
            author_name: None,
            author_login: None,
            authored_at: None,
            url: "https://github.com/owner/repository/commit/a".into(),
        }
    }

    #[test]
    fn each_commit_is_listed_and_every_co_author_ends_the_message_once() {
        let message = message(&[
            commit("feat: add widgets\n\nWhy widgets.\n\nCo-authored-by: Ada <ada@example.com>"),
            commit(
                "fix: tidy\n\nCo-Authored-By: Claude <noreply@anthropic.com>\nco-authored-by: Ada <ada@example.com>",
            ),
        ]);

        assert_eq!(
            message,
            "* feat: add widgets\n\nWhy widgets.\n\n\
             * fix: tidy\n\n\
             Co-authored-by: Ada <ada@example.com>\n\
             Co-Authored-By: Claude <noreply@anthropic.com>"
        );
    }

    #[test]
    fn commits_without_co_authors_are_only_listed() {
        assert_eq!(
            message(&[commit("first"), commit("second\n")]),
            "* first\n\n* second"
        );
        assert_eq!(message(&[]), "");
    }
}
