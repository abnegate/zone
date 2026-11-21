use std::borrow::Cow;

const SPECIAL: [char; 5] = ['&', '<', '>', '"', '\''];

pub fn html(text: &str) -> Cow<'_, str> {
    if !text.contains(SPECIAL) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + text.len() / 2);
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_special_character_is_escaped() {
        assert_eq!(
            html(r#"<a href="x" title='y'>Tom & Jerry</a>"#),
            "&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;Tom &amp; Jerry&lt;/a&gt;"
        );
    }

    #[test]
    fn plain_text_is_borrowed_untouched() {
        let escaped = html("Acme Ltd — Zürich");
        assert!(matches!(escaped, Cow::Borrowed("Acme Ltd — Zürich")));
    }

    #[test]
    fn an_escaped_entity_is_escaped_again() {
        assert_eq!(html("&amp;"), "&amp;amp;");
    }
}
