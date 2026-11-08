//! The line a chat search shows for each message it found.

/// What a line break inside a snippet reads as.
pub(crate) const LINE_BREAK: &str = " ⏎ ";

/// Collapse `text` onto one line and cut it to `max_chars`.
///
/// A message body arrives with newlines and runs of whitespace that would push
/// the part worth reading off the result. Runs of blank space within a line
/// go; a line break becomes [`LINE_BREAK`], so the lines stay apart.
pub(crate) fn excerpt(text: &str, max_chars: usize) -> String {
    let collapsed = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<&str>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>()
        .join(LINE_BREAK);
    match collapsed.char_indices().nth(max_chars) {
        Some((byte_index, _)) => format!("{}…", &collapsed[..byte_index]),
        None => collapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blank space inside one line is noise that pushes the readable part off
    /// the card, so it still collapses.
    #[test]
    fn blank_space_inside_a_line_still_collapses() {
        assert_eq!(excerpt("cargo    test   --all", 400), "cargo test --all");
        assert_eq!(
            excerpt("  one\n\n\ntwo  ", 400),
            format!("one{LINE_BREAK}two")
        );
    }

    #[test]
    fn two_lines_stay_apart() {
        assert_eq!(
            excerpt("cat config.toml\nrm -rf /srv/zone", 400),
            format!("cat config.toml{LINE_BREAK}rm -rf /srv/zone")
        );
    }

    #[test]
    fn a_long_snippet_is_cut_on_a_character_boundary() {
        assert_eq!(excerpt("ééééé", 3), "ééé…");
        assert_eq!(excerpt("short", 5), "short");
    }
}
