//! An image a message carries, read from the URL it was attached by.

const DATA_SCHEME: &str = "data:";
const BASE64_SUFFIX: &str = ";base64";

/// The image types every agent here can be shown.
const MEDIA_TYPES: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

/// Where an attached image's bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Image<'a> {
    /// Carried in the URL itself, base64-encoded.
    Inline { media_type: &'a str, data: &'a str },
    /// At a public address the agent has to fetch.
    Linked(&'a str),
}

impl<'a> Image<'a> {
    /// The image `url` names, or `None` for one no agent can be shown: an
    /// unsupported type, a malformed data URL, or a relative URL that only
    /// Zone's own authenticated routes can serve.
    pub fn parse(url: &'a str) -> Option<Self> {
        if let Some(rest) = url.strip_prefix(DATA_SCHEME) {
            let (header, data) = rest.split_once(',')?;
            let media_type = header.strip_suffix(BASE64_SUFFIX)?;
            return (extension(media_type).is_some() && !data.is_empty())
                .then_some(Self::Inline { media_type, data });
        }
        (url.starts_with("https://") || url.starts_with("http://")).then_some(Self::Linked(url))
    }
}

/// The file extension an image of `media_type` is saved under.
pub fn extension(media_type: &str) -> Option<&'static str> {
    MEDIA_TYPES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(media_type))
        .map(|(_, extension)| *extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base64_data_url_is_an_inline_image() {
        assert_eq!(
            Image::parse("data:image/png;base64,iVBORw0KGgo="),
            Some(Image::Inline {
                media_type: "image/png",
                data: "iVBORw0KGgo=",
            })
        );
    }

    #[test]
    fn a_public_address_is_a_linked_image() {
        assert_eq!(
            Image::parse("https://example.test/shot.png"),
            Some(Image::Linked("https://example.test/shot.png"))
        );
    }

    #[test]
    fn an_image_no_agent_can_be_shown_is_refused() {
        for url in [
            "/api/artifacts/workspace/chat/message/generated.png",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:image/png,not-base64",
            "data:image/png;base64,",
            "data:text/plain;base64,aGk=",
            "ftp://example.test/shot.png",
        ] {
            assert_eq!(Image::parse(url), None, "{url}");
        }
    }
}
