//! Stable per-chat identifiers for retrieved sources.
//!
//! A reply cites a source by identifier rather than by its position in a result
//! list, so a citation survives re-ranking, pagination, and a second search that
//! returns the same page in a different slot. The identifier is derived from the
//! source key, which is what makes a citation checkable: the server re-derives
//! the identifier for everything it actually retrieved and refuses any marker
//! that names something it never saw.
//!
//! # The key is hashed verbatim
//!
//! [`mint`] hashes the source key byte for byte. It does not normalise,
//! lowercase, trim, sort query parameters, strip fragments, or resolve
//! redirects, and it must never begin to. Normalisation is a per-caller
//! judgement, and two callers that judge differently would mint two different
//! identifiers for one source — exactly the failure a content-derived
//! identifier exists to prevent. Settle the key once, before it reaches this
//! module, and hand the same bytes to every call.
//!
//! The key is what names the source, which is not always where the source
//! lives: a knowledge passage is keyed by the entry it came from and addressed
//! by a URL. Only the key reaches this module. The address is the registry's
//! business, and [`crate::db::chat_sources`] stores it alongside.

use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DIGEST_BYTES: usize = 32;
const SEPARATOR: char = ':';

/// Hexadecimal characters a freshly minted digest carries.
pub const MINT_WIDTH: usize = 6;

/// Hexadecimal characters [`extend`] adds each time a prefix collides.
pub const WIDTH_STEP: usize = 2;

/// Widest digest [`extend`] will produce.
///
/// Thirty-two hexadecimal characters is half of SHA-256, so 128 bits of the
/// digest, putting the birthday bound at roughly 2^64 distinct sources in one
/// chat — unreachable, while a chat's real source count makes even the six of
/// [`MINT_WIDTH`] collide only rarely. The ceiling exists so a registry
/// resolving a collision terminates rather than lengthening forever, and it
/// doubles as the upper bound of the marker grammar, which keeps every
/// identifier this module can mint scannable by [`markers`].
pub const MAX_WIDTH: usize = 32;

static MARKER: OnceLock<Regex> = OnceLock::new();

/// What a source is, which the model reads as the first half of a marker.
///
/// A closed set: the variants are baked into the marker grammar shown to the
/// model, so an unrecognised kind is not a marker at all.
#[derive(Debug, Clone, Copy, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Web,
    Doc,
    Kb,
    Chat,
}

impl Kind {
    /// Every variant, in the order the marker pattern alternates over them.
    pub const ALL: [Self; 4] = [Self::Web, Self::Doc, Self::Kb, Self::Chat];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Doc => "doc",
            Self::Kb => "kb",
            Self::Chat => "chat",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Derive the bare identifier for a source, as `kind:digest`.
///
/// The key is hashed verbatim. Read the module documentation before changing
/// anything about what reaches the hasher.
pub fn mint(kind: Kind, key: &str) -> String {
    token(kind, &prefix(key, MINT_WIDTH))
}

/// Assemble a bare identifier out of the parts [`markers`] hands back.
pub fn token(kind: Kind, digest: &str) -> String {
    format!("{kind}{SEPARATOR}{digest}")
}

/// Re-derive an identifier from the same key, [`WIDTH_STEP`] characters wider.
///
/// Two keys can share a six-character prefix, and the registry settles that by
/// lengthening both until they differ. Nothing comes back once the digest is at
/// [`MAX_WIDTH`], or when `existing` is not a well-formed identifier, so a
/// caller lengthening in a loop always terminates.
pub fn extend(existing: &str, key: &str) -> Option<String> {
    let (kind, digest) = split(existing)?;
    let width = digest.len() + WIDTH_STEP;
    (width <= MAX_WIDTH).then(|| token(kind, &prefix(key, width)))
}

/// Every distinct `[kind:digest]` marker in a reply, in the order it first
/// appears.
pub fn markers(text: &str) -> Vec<(Kind, String)> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (_, [kind, digest]) in pattern()
        .captures_iter(text)
        .map(|capture| capture.extract())
    {
        let Some(kind) = Kind::parse(kind) else {
            continue;
        };
        if seen.insert((kind, digest)) {
            found.push((kind, digest.to_string()));
        }
    }
    found
}

/// The bracketed inline form of an identifier, which is what the model is shown
/// and what it is asked to emit.
pub fn render(identifier: &str) -> String {
    format!("[{identifier}]")
}

fn prefix(key: &str, width: usize) -> String {
    let digest: [u8; DIGEST_BYTES] = Sha256::digest(key.as_bytes()).into();
    let mut encoded = hex::encode(digest);
    encoded.truncate(width);
    encoded
}

fn split(identifier: &str) -> Option<(Kind, &str)> {
    let (kind, digest) = identifier.split_once(SEPARATOR)?;
    let kind = Kind::parse(kind)?;
    if !(MINT_WIDTH..=MAX_WIDTH).contains(&digest.len()) {
        return None;
    }
    digest
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        .then_some((kind, digest))
}

fn pattern() -> &'static Regex {
    MARKER.get_or_init(|| {
        let kinds = Kind::ALL.map(Kind::as_str).join("|");
        Regex::new(&format!(
            r"\[({kinds}){SEPARATOR}([0-9a-f]{{{MINT_WIDTH},{MAX_WIDTH}}})\]"
        ))
        .expect("identifier marker pattern is a valid regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "https://example.com/Docs/Guide?b=2&a=1#top";
    const DIGEST: &str = "7b0f74a12dbf90394bc3cf93fbe39ccf9aef33e384a3cfb406f36014a94e2551";

    #[test]
    fn the_wire_form_of_a_kind_is_its_own_string() {
        for kind in Kind::ALL {
            assert_eq!(
                serde_json::to_value(kind).expect("a kind serializes"),
                serde_json::json!(kind.as_str()),
                "{kind:?} serializes to something other than its own string"
            );
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("Web"), None);
        assert_eq!(Kind::parse("page"), None);
        assert_eq!(Kind::parse(""), None);
    }

    #[test]
    fn minting_the_same_uri_twice_yields_the_same_identifier() {
        let first = mint(Kind::Web, URI);

        for _ in 0..8 {
            assert_eq!(mint(Kind::Web, URI), first);
        }
        assert_eq!(first, format!("web{SEPARATOR}{}", &DIGEST[..MINT_WIDTH]));
        assert_eq!(first.len(), "web".len() + 1 + MINT_WIDTH);
    }

    #[test]
    fn the_uri_is_hashed_verbatim_and_never_normalised() {
        let minted = mint(Kind::Web, URI);

        for variant in [
            "https://example.com/docs/guide?b=2&a=1#top",
            "https://example.com/Docs/Guide?b=2&a=1",
            "https://example.com/Docs/Guide?a=1&b=2#top",
            "https://example.com/Docs/Guide?b=2&a=1#top ",
            " https://example.com/Docs/Guide?b=2&a=1#top",
            "https://example.com/Docs/Guide/?b=2&a=1#top",
        ] {
            assert_ne!(
                mint(Kind::Web, variant),
                minted,
                "{variant} minted the same identifier as the canonical URI, so something \
                 normalised the bytes before they were hashed"
            );
        }
    }

    #[test]
    fn the_kind_is_part_of_the_identifier() {
        let identifiers: HashSet<String> = Kind::ALL.iter().map(|kind| mint(*kind, URI)).collect();

        assert_eq!(identifiers.len(), Kind::ALL.len());
    }

    #[test]
    fn extending_lengthens_the_digest_by_exactly_two() {
        let minted = mint(Kind::Doc, URI);
        let wider = extend(&minted, URI).expect("a minted identifier extends");

        assert_eq!(
            wider,
            format!("doc{SEPARATOR}{}", &DIGEST[..MINT_WIDTH + WIDTH_STEP])
        );
        assert_eq!(wider.len(), minted.len() + WIDTH_STEP);
        assert!(
            wider.starts_with(&minted),
            "{wider} does not extend {minted}, so the digest was re-derived from other bytes"
        );
    }

    #[test]
    fn extending_stops_at_the_ceiling_so_a_caller_cannot_loop_forever() {
        let mut identifier = mint(Kind::Kb, URI);
        let mut widths = vec![identifier.len()];

        while let Some(wider) = extend(&identifier, URI) {
            assert!(
                widths.len() < MAX_WIDTH,
                "extending never stopped: {widths:?}"
            );
            widths.push(wider.len());
            identifier = wider;
        }

        assert_eq!(identifier, format!("kb{SEPARATOR}{}", &DIGEST[..MAX_WIDTH]));
        assert_eq!(widths.len(), (MAX_WIDTH - MINT_WIDTH) / WIDTH_STEP + 1);
        assert_eq!(extend(&identifier, URI), None);
    }

    #[test]
    fn an_identifier_that_is_not_well_formed_never_extends() {
        for existing in [
            "",
            "web",
            "web:",
            "page:7b0f74",
            "web:7b0f7",
            "web:zzzzzz",
            "web:7B0F74",
            &format!("web{SEPARATOR}{DIGEST}"),
        ] {
            assert_eq!(extend(existing, URI), None, "{existing}");
        }
    }

    #[test]
    fn a_marker_is_found_after_terminal_punctuation() {
        let reply = "The guide covers it.[web:7b0f74] The changelog agrees![doc:abc123] \
                     Anything else?[kb:0f9e8d7c]";

        assert_eq!(
            markers(reply),
            vec![
                (Kind::Web, "7b0f74".to_string()),
                (Kind::Doc, "abc123".to_string()),
                (Kind::Kb, "0f9e8d7c".to_string()),
            ]
        );
    }

    #[test]
    fn a_non_hexadecimal_or_too_short_digest_is_not_a_marker() {
        let reply = "Sources: [web:zzzzzz] [doc:12345] [kb:7b0f74g1] [chat:7B0F74] \
                     [page:7b0f74] [web:7b0f7] [web:] [web7b0f74] \
                     [web:7b0f74a12dbf90394bc3cf93fbe39ccf9a]";

        assert!(
            markers(reply).is_empty(),
            "{:?} was scanned out of a reply that has no well-formed marker",
            markers(reply)
        );
    }

    #[test]
    fn a_marker_repeated_in_one_reply_is_returned_once() {
        let reply = "The page says so [web:7b0f74], and it repeats it [web:7b0f74]. \
                     A doc with the same digest is a different source [doc:7b0f74], \
                     and so is a wider digest of the same page [web:7b0f74a1]. \
                     [web:7b0f74]";

        assert_eq!(
            markers(reply),
            vec![
                (Kind::Web, "7b0f74".to_string()),
                (Kind::Doc, "7b0f74".to_string()),
                (Kind::Web, "7b0f74a1".to_string()),
            ]
        );
    }

    #[test]
    fn rendered_identifiers_scan_back_out_of_a_reply() {
        let mut identifier = mint(Kind::Chat, URI);
        loop {
            let reply = format!("As established {}.", render(&identifier));

            let (kind, digest) = split(&identifier).expect("a minted identifier splits");
            assert_eq!(markers(&reply), vec![(kind, digest.to_string())]);
            assert_eq!(token(kind, digest), identifier);

            let Some(wider) = extend(&identifier, URI) else {
                break;
            };
            identifier = wider;
        }
    }
}
