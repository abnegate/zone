//! An image a message carries, read from the URL it was attached by.

mod rejection;

use std::borrow::Cow;

use base64::Engine as _;
use base64::alphabet::STANDARD;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

pub use rejection::Rejection;

const DATA_SCHEME: &str = "data:";
const BASE64_SUFFIX: &str = ";base64";

/// The media types every agent here opens, with the extension each is saved
/// under.
const MEDIA_TYPES: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

/// Browsers pad base64 and other encoders do not, so either is read.
const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image<'a> {
    Inline {
        media_type: &'static str,
        extension: &'static str,
        bytes: Vec<u8>,
    },
    Linked(&'a str),
}

impl<'a> Image<'a> {
    pub fn parse(url: &'a str) -> Result<Self, Rejection> {
        if let Some(rest) = url.strip_prefix(DATA_SCHEME) {
            let (header, data) = rest.split_once(',').ok_or(Rejection::Malformed)?;
            let declared = header
                .strip_suffix(BASE64_SUFFIX)
                .ok_or(Rejection::Malformed)?;
            let (media_type, extension) = MEDIA_TYPES
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(declared))
                .ok_or(Rejection::Unsupported)?;
            let bytes = LENIENT
                .decode(compact(data).as_bytes())
                .map_err(|_| Rejection::Malformed)?;
            if bytes.is_empty() {
                return Err(Rejection::Malformed);
            }
            return Ok(Self::Inline {
                media_type,
                extension,
                bytes,
            });
        }
        if url.starts_with("https://") || url.starts_with("http://") {
            return Ok(Self::Linked(url));
        }
        Err(Rejection::Unreachable)
    }
}

fn compact(data: &str) -> Cow<'_, str> {
    if data.bytes().any(|byte| byte.is_ascii_whitespace()) {
        Cow::Owned(data.chars().filter(|c| !c.is_ascii_whitespace()).collect())
    } else {
        Cow::Borrowed(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_BYTES: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    fn inline(media_type: &'static str, extension: &'static str) -> Image<'static> {
        Image::Inline {
            media_type,
            extension,
            bytes: PNG_BYTES.to_vec(),
        }
    }

    #[test]
    fn a_base64_data_url_is_an_inline_image() {
        assert_eq!(
            Image::parse("data:image/png;base64,iVBORw0KGgo="),
            Ok(inline("image/png", "png"))
        );
    }

    #[test]
    fn a_media_type_in_any_case_is_read_as_the_lowercase_one_agents_take() {
        assert_eq!(
            Image::parse("data:IMAGE/PNG;base64,iVBORw0KGgo="),
            Ok(inline("image/png", "png"))
        );
    }

    #[test]
    fn wrapped_or_unpadded_base64_still_decodes() {
        for url in [
            "data:image/png;base64,iVBORw0K\nGgo=",
            "data:image/png;base64,iVBORw0KGgo",
        ] {
            assert_eq!(Image::parse(url), Ok(inline("image/png", "png")), "{url}");
        }
    }

    #[test]
    fn a_public_address_is_a_linked_image() {
        assert_eq!(
            Image::parse("https://example.test/shot.png"),
            Ok(Image::Linked("https://example.test/shot.png"))
        );
    }

    #[test]
    fn an_image_no_agent_can_be_shown_says_why() {
        for (url, rejection) in [
            (
                "/api/artifacts/workspace/chat/message/generated.png",
                Rejection::Unreachable,
            ),
            ("ftp://example.test/shot.png", Rejection::Unreachable),
            ("data:image/svg+xml;base64,PHN2Zz4=", Rejection::Unsupported),
            ("data:text/plain;base64,aGk=", Rejection::Unsupported),
            ("data:image/png,not-base64", Rejection::Malformed),
            ("data:image/png;base64,", Rejection::Malformed),
            ("data:image/png;base64,!!!!", Rejection::Malformed),
        ] {
            assert_eq!(Image::parse(url), Err(rejection), "{url}");
        }
    }
}
