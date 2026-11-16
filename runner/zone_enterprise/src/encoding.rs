use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::SIGNATURE_LENGTH;

use crate::error::Error;

pub const SEPARATOR: char = '.';

pub fn encode(payload: &[u8], signature: &[u8; SIGNATURE_LENGTH]) -> String {
    format!(
        "{}{SEPARATOR}{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(signature)
    )
}

pub(crate) fn decode(text: &str) -> Result<(Vec<u8>, [u8; SIGNATURE_LENGTH]), Error> {
    let normalized: String = text
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    let (payload, signature) = normalized.split_once(SEPARATOR).ok_or(Error::Malformed)?;
    if payload.is_empty() || signature.contains(SEPARATOR) {
        return Err(Error::Malformed);
    }
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::Malformed)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Error::Malformed)?;
    let signature = <[u8; SIGNATURE_LENGTH]>::try_from(signature).map_err(|_| Error::Malformed)?;
    Ok((payload, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNATURE: [u8; SIGNATURE_LENGTH] = [0xfb; SIGNATURE_LENGTH];

    fn payload_bytes() -> Vec<u8> {
        (0..=u8::MAX).collect()
    }

    #[test]
    fn round_trips() {
        let text = encode(&payload_bytes(), &SIGNATURE);
        assert_eq!(decode(&text), Ok((payload_bytes(), SIGNATURE)));
    }

    #[test]
    fn output_is_url_safe_without_padding() {
        let text = encode(&payload_bytes(), &SIGNATURE);
        for forbidden in ['+', '/', '='] {
            assert!(!text.contains(forbidden), "{text} contains {forbidden}");
        }
        assert!(
            text.contains('-') && text.contains('_'),
            "the bytes should exercise both url-safe characters: {text}"
        );
    }

    #[test]
    fn output_has_one_separator() {
        let text = encode(b"{}", &SIGNATURE);
        assert_eq!(text.matches(SEPARATOR).count(), 1);
    }

    #[test]
    fn strips_every_ascii_whitespace_character() {
        let text = encode(&payload_bytes(), &SIGNATURE);
        let spaced: String = text
            .chars()
            .flat_map(|character| [character, ' ', '\t', '\n', '\r', '\x0c'])
            .collect();
        assert_eq!(decode(&spaced), Ok((payload_bytes(), SIGNATURE)));
    }

    #[test]
    fn refuses_empty_input() {
        assert_eq!(decode(""), Err(Error::Malformed));
        assert_eq!(decode(" \n\t\r\n "), Err(Error::Malformed));
    }

    #[test]
    fn refuses_input_without_a_separator() {
        assert_eq!(
            decode(&URL_SAFE_NO_PAD.encode(payload_bytes())),
            Err(Error::Malformed)
        );
    }

    #[test]
    fn refuses_input_with_three_parts() {
        let text = format!(
            "{}{SEPARATOR}{}",
            encode(&payload_bytes(), &SIGNATURE),
            URL_SAFE_NO_PAD.encode(SIGNATURE)
        );
        assert_eq!(decode(&text), Err(Error::Malformed));
    }

    #[test]
    fn refuses_empty_parts() {
        let signature = URL_SAFE_NO_PAD.encode(SIGNATURE);
        assert_eq!(
            decode(&format!("{SEPARATOR}{signature}")),
            Err(Error::Malformed)
        );
        assert_eq!(decode(&format!("e30{SEPARATOR}")), Err(Error::Malformed));
        assert_eq!(decode(&SEPARATOR.to_string()), Err(Error::Malformed));
    }

    #[test]
    fn refuses_a_short_or_long_signature() {
        for length in [SIGNATURE_LENGTH - 1, SIGNATURE_LENGTH + 1] {
            let text = format!(
                "e30{SEPARATOR}{}",
                URL_SAFE_NO_PAD.encode(vec![0xfb; length])
            );
            assert_eq!(
                decode(&text),
                Err(Error::Malformed),
                "accepted a {length}-byte signature"
            );
        }
    }

    #[test]
    fn refuses_padding() {
        let text = encode(&payload_bytes(), &SIGNATURE);
        assert_eq!(decode(&format!("{text}==")), Err(Error::Malformed));
        let padded = format!(
            "{}{SEPARATOR}{}",
            base64::engine::general_purpose::URL_SAFE.encode(b"{}"),
            URL_SAFE_NO_PAD.encode(SIGNATURE)
        );
        assert!(padded.contains('='));
        assert_eq!(decode(&padded), Err(Error::Malformed));
    }

    #[test]
    fn refuses_the_standard_alphabet() {
        let text = encode(&payload_bytes(), &SIGNATURE)
            .replace('-', "+")
            .replace('_', "/");
        assert!(text.contains('+') && text.contains('/'));
        assert_eq!(decode(&text), Err(Error::Malformed));
    }

    #[test]
    fn refuses_non_canonical_trailing_bits() {
        let text = encode(b"{}", &SIGNATURE);
        let (payload, signature) = text.split_once(SEPARATOR).unwrap();
        assert_eq!(
            decode(&format!(
                "{payload}{SEPARATOR}{}",
                with_trailing_bits(signature)
            )),
            Err(Error::Malformed)
        );
        assert_eq!(
            decode(&format!(
                "{}{SEPARATOR}{signature}",
                with_trailing_bits(payload)
            )),
            Err(Error::Malformed)
        );
    }

    fn with_trailing_bits(part: &str) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let (head, last) = part.split_at(part.len() - 1);
        let index = ALPHABET
            .iter()
            .position(|&character| character == last.as_bytes()[0])
            .unwrap();
        assert!(URL_SAFE_NO_PAD.decode(part).is_ok());
        format!("{head}{}", char::from(ALPHABET[index | 1]))
    }
}
