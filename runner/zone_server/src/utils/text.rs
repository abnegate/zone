/// Truncate on a character boundary, marking that we cut.
pub(crate) fn truncate(text: &str, maximum_characters: usize) -> String {
    match text.char_indices().nth(maximum_characters) {
        Some((byte_index, _)) => format!("{}…", &text[..byte_index]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_marks_cut_text() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 3), "hel…");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        // Multi-byte characters must not be split, which byte slicing would do.
        let text = "αβγδε";
        assert_eq!(truncate(text, 3), "αβγ…");
        assert_eq!(truncate(text, 99), text);
    }
}
