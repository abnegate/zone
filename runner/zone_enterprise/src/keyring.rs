use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ed25519_dalek::{PUBLIC_KEY_LENGTH, Signature};

use crate::duplicate_key::DuplicateKey;
use crate::encoding;
use crate::error::Error;
use crate::header::Header;
use crate::license::License;
use crate::payload::Payload;
use crate::production;
use crate::trusted_key::TrustedKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyring {
    keys: HashMap<String, TrustedKey>,
}

impl Keyring {
    pub fn new(keys: impl IntoIterator<Item = TrustedKey>) -> Result<Self, DuplicateKey> {
        let mut trusted = HashMap::new();
        for key in keys {
            match trusted.entry(key.id().to_owned()) {
                Entry::Occupied(entry) => {
                    return Err(DuplicateKey {
                        id: entry.key().clone(),
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(key);
                }
            }
        }
        Ok(Self { keys: trusted })
    }

    /// Panics if a compiled-in key is invalid or listed twice.
    pub fn production() -> Self {
        Self::trusting(&production::KEYS)
    }

    fn trusting(keys: &[(&str, [u8; PUBLIC_KEY_LENGTH])]) -> Self {
        let keys = keys.iter().map(|(id, public)| {
            TrustedKey::new(*id, *public)
                .unwrap_or_else(|error| panic!("The production key {id} is unusable: {error}"))
        });
        Self::new(keys).unwrap_or_else(|error| panic!("The production keyring is invalid: {error}"))
    }

    pub fn get(&self, id: &str) -> Option<&TrustedKey> {
        self.keys.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(String::as_str)
    }

    pub fn verify(&self, text: &str) -> Result<License, Error> {
        let (payload, signature) = encoding::decode(text)?;
        let header: Header = serde_json::from_slice(&payload).map_err(|_| Error::Malformed)?;
        let trusted = self
            .keys
            .get(&header.key_id)
            .ok_or(Error::UnknownKey { id: header.key_id })?;
        trusted
            .key()
            .verify_strict(&payload, &Signature::from_bytes(&signature))
            .map_err(|_| Error::Signature)?;
        let parsed: Payload =
            serde_json::from_slice(&payload).map_err(|error| Error::Payload(error.to_string()))?;
        parsed.validate()?;
        Ok(License::new(
            parsed,
            encoding::encode(&payload, &signature),
            trusted.id().to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use chrono::TimeDelta;
    use serde_json::{Value, json};

    use super::*;
    use crate::encoding::SEPARATOR;
    use crate::fixtures;
    use crate::signing::{sign, sign_bytes};
    use crate::tier::Tier;

    fn signed() -> String {
        sign(&fixtures::payload(), &fixtures::first())
    }

    fn wrap(text: &str, width: usize, line_break: &str) -> String {
        text.as_bytes()
            .chunks(width)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join(line_break)
    }

    fn payload_value() -> Value {
        serde_json::to_value(fixtures::payload()).unwrap()
    }

    #[test]
    fn round_trips_a_signed_payload() {
        let text = signed();
        let license = fixtures::keyring().verify(&text).unwrap();
        assert_eq!(license.payload(), &fixtures::payload());
        assert_eq!(license.key_id(), fixtures::FIRST);
        assert_eq!(license.text(), text);
    }

    #[test]
    fn records_the_key_that_verified() {
        let payload = Payload {
            key_id: fixtures::SECOND.into(),
            tier: Tier::Enterprise,
            ..fixtures::payload()
        };
        let license = fixtures::keyring()
            .verify(&sign(&payload, &fixtures::second()))
            .unwrap();
        assert_eq!(license.key_id(), fixtures::SECOND);
        assert_eq!(license.payload(), &payload);
    }

    #[test]
    fn verifies_a_key_wrapped_with_line_feeds() {
        let text = signed();
        let license = fixtures::keyring().verify(&wrap(&text, 64, "\n")).unwrap();
        assert_eq!(license.text(), text);
    }

    #[test]
    fn verifies_a_key_wrapped_with_carriage_returns() {
        let text = signed();
        let license = fixtures::keyring()
            .verify(&wrap(&text, 64, "\r\n"))
            .unwrap();
        assert_eq!(license.text(), text);
    }

    #[test]
    fn verifies_a_key_with_interior_spaces_and_tabs() {
        let text = signed();
        let license = fixtures::keyring().verify(&wrap(&text, 7, " \t ")).unwrap();
        assert_eq!(license.text(), text);
    }

    #[test]
    fn verifies_a_key_with_surrounding_whitespace() {
        let text = signed();
        let license = fixtures::keyring()
            .verify(&format!("\n\t  {text} \r\n\n"))
            .unwrap();
        assert_eq!(license.text(), text);
    }

    #[test]
    fn verifies_a_key_with_non_breaking_and_zero_width_spaces() {
        let text = signed();
        let license = fixtures::keyring()
            .verify(&format!(
                "\u{FEFF}{}\u{00A0}",
                wrap(&text, 16, "\u{200B}\u{00A0}")
            ))
            .unwrap();
        assert_eq!(license.text(), text);
    }

    #[test]
    fn whitespace_does_not_change_the_result() {
        let text = signed();
        let keyring = fixtures::keyring();
        let plain = keyring.verify(&text);
        assert_eq!(keyring.verify(&wrap(&text, 1, "\n \t\r")), plain);

        let tampered = flip_signature(&text);
        let plain = keyring.verify(&tampered);
        assert_eq!(plain, Err(Error::Signature));
        assert_eq!(keyring.verify(&wrap(&tampered, 3, "\r\n")), plain);
    }

    #[test]
    fn refuses_the_standard_alphabet() {
        let text = (1..u32::MAX)
            .map(|seats| {
                sign(
                    &Payload {
                        seats,
                        ..fixtures::payload()
                    },
                    &fixtures::first(),
                )
            })
            .find(|text| text.contains('-') && text.contains('_'))
            .unwrap();
        let standard = text.replace('-', "+").replace('_', "/");
        assert_eq!(fixtures::keyring().verify(&standard), Err(Error::Malformed));
    }

    #[test]
    fn refuses_padding() {
        let text = signed();
        assert_eq!(
            fixtures::keyring().verify(&format!("{text}==")),
            Err(Error::Malformed)
        );
    }

    #[test]
    fn refuses_non_canonical_trailing_bits() {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let text = signed();
        let last = *text.as_bytes().last().unwrap();
        let index = ALPHABET
            .iter()
            .position(|&character| character == last)
            .unwrap();
        let altered = format!(
            "{}{}",
            &text[..text.len() - 1],
            char::from(ALPHABET[index | 1])
        );
        assert_ne!(altered, text);
        assert_eq!(fixtures::keyring().verify(&altered), Err(Error::Malformed));
    }

    #[test]
    fn refuses_empty_input() {
        assert_eq!(fixtures::keyring().verify(""), Err(Error::Malformed));
    }

    #[test]
    fn refuses_whitespace_only_input() {
        assert_eq!(
            fixtures::keyring().verify(" \r\n\t \n"),
            Err(Error::Malformed)
        );
    }

    #[test]
    fn refuses_a_key_without_a_separator() {
        let text = signed().replace(SEPARATOR, "");
        assert_eq!(fixtures::keyring().verify(&text), Err(Error::Malformed));
    }

    #[test]
    fn refuses_a_key_with_three_parts() {
        let text = signed();
        let signature = text.split_once(SEPARATOR).unwrap().1;
        assert_eq!(
            fixtures::keyring().verify(&format!("{text}{SEPARATOR}{signature}")),
            Err(Error::Malformed)
        );
    }

    #[test]
    fn refuses_a_short_or_long_signature() {
        let text = signed();
        let (payload, signature) = text.split_once(SEPARATOR).unwrap();
        let signature = URL_SAFE_NO_PAD.decode(signature).unwrap();
        let short = format!(
            "{payload}{SEPARATOR}{}",
            URL_SAFE_NO_PAD.encode(&signature[..63])
        );
        let long = format!(
            "{payload}{SEPARATOR}{}",
            URL_SAFE_NO_PAD.encode([signature.as_slice(), &[0]].concat())
        );
        assert_eq!(fixtures::keyring().verify(&short), Err(Error::Malformed));
        assert_eq!(fixtures::keyring().verify(&long), Err(Error::Malformed));
    }

    #[test]
    fn refuses_a_payload_without_a_key_id() {
        let mut value = payload_value();
        value.as_object_mut().unwrap().remove("key_id");
        let text = sign_bytes(&serde_json::to_vec(&value).unwrap(), &fixtures::first());
        assert_eq!(fixtures::keyring().verify(&text), Err(Error::Malformed));
    }

    #[test]
    fn refuses_signed_bytes_that_are_not_json() {
        let text = sign_bytes(b"\x00not json at all\xff", &fixtures::first());
        assert_eq!(fixtures::keyring().verify(&text), Err(Error::Malformed));
    }

    #[test]
    fn refuses_a_tampered_payload() {
        let text = signed();
        let (payload, signature) = text.split_once(SEPARATOR).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        let customer = bytes
            .windows(4)
            .position(|window| window == b"Acme")
            .unwrap();
        bytes[customer] ^= 0x01;
        let tampered = format!("{}{SEPARATOR}{signature}", URL_SAFE_NO_PAD.encode(&bytes));
        assert_eq!(fixtures::keyring().verify(&tampered), Err(Error::Signature));
    }

    #[test]
    fn refuses_a_tampered_signature() {
        assert_eq!(
            fixtures::keyring().verify(&flip_signature(&signed())),
            Err(Error::Signature)
        );
    }

    fn flip_signature(text: &str) -> String {
        let (payload, signature) = text.split_once(SEPARATOR).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(signature).unwrap();
        bytes[40] ^= 0x01;
        format!("{payload}{SEPARATOR}{}", URL_SAFE_NO_PAD.encode(&bytes))
    }

    #[test]
    fn refuses_an_unknown_key_and_names_it() {
        let payload = Payload {
            key_id: "2031-4".into(),
            ..fixtures::payload()
        };
        let text = sign(&payload, &fixtures::first());
        assert_eq!(
            fixtures::keyring().verify(&text),
            Err(Error::UnknownKey {
                id: "2031-4".into()
            })
        );
    }

    #[test]
    fn refuses_a_key_id_claiming_another_trusted_key() {
        let payload = Payload {
            key_id: fixtures::SECOND.into(),
            ..fixtures::payload()
        };
        let text = sign(&payload, &fixtures::first());
        assert_eq!(fixtures::keyring().verify(&text), Err(Error::Signature));
    }

    #[test]
    fn rotation_verifies_licenses_from_either_key() {
        let keyring = fixtures::keyring();
        let first = sign(&fixtures::payload(), &fixtures::first());
        let second = sign(
            &Payload {
                key_id: fixtures::SECOND.into(),
                ..fixtures::payload()
            },
            &fixtures::second(),
        );
        assert_eq!(keyring.verify(&first).unwrap().key_id(), fixtures::FIRST);
        assert_eq!(keyring.verify(&second).unwrap().key_id(), fixtures::SECOND);
    }

    #[test]
    fn rotation_drops_licenses_from_a_retired_key() {
        let keyring =
            Keyring::new([fixtures::trusted(fixtures::SECOND, &fixtures::second())]).unwrap();
        assert_eq!(
            keyring.verify(&signed()),
            Err(Error::UnknownKey {
                id: fixtures::FIRST.into()
            })
        );
    }

    #[test]
    fn refuses_signed_json_that_is_not_a_payload() {
        let text = sign_bytes(
            &serde_json::to_vec(&json!({ "key_id": fixtures::FIRST })).unwrap(),
            &fixtures::first(),
        );
        assert!(matches!(
            fixtures::keyring().verify(&text),
            Err(Error::Payload(_))
        ));
    }

    #[test]
    fn refuses_signed_json_with_wrong_types() {
        let mut value = payload_value();
        value["seats"] = Value::from("many");
        let text = sign_bytes(&serde_json::to_vec(&value).unwrap(), &fixtures::first());
        assert!(matches!(
            fixtures::keyring().verify(&text),
            Err(Error::Payload(_))
        ));
    }

    #[test]
    fn refuses_zero_seats() {
        let text = sign(
            &Payload {
                seats: 0,
                ..fixtures::payload()
            },
            &fixtures::first(),
        );
        assert!(matches!(
            fixtures::keyring().verify(&text),
            Err(Error::Payload(_))
        ));
    }

    #[test]
    fn refuses_expiry_not_after_issue() {
        let payload = fixtures::payload();
        for expires_at in [payload.issued_at, payload.issued_at - TimeDelta::days(1)] {
            let text = sign(
                &Payload {
                    expires_at,
                    ..payload.clone()
                },
                &fixtures::first(),
            );
            assert!(
                matches!(fixtures::keyring().verify(&text), Err(Error::Payload(_))),
                "accepted {expires_at}"
            );
        }
    }

    #[test]
    fn refuses_a_blank_customer() {
        let text = sign(
            &Payload {
                customer: "  \t".into(),
                ..fixtures::payload()
            },
            &fixtures::first(),
        );
        assert!(matches!(
            fixtures::keyring().verify(&text),
            Err(Error::Payload(_))
        ));
    }

    #[test]
    fn accepts_an_unknown_extra_field() {
        let mut value = payload_value();
        value
            .as_object_mut()
            .unwrap()
            .insert("support".into(), json!({ "level": "priority" }));
        let text = sign_bytes(&serde_json::to_vec(&value).unwrap(), &fixtures::first());
        assert_eq!(
            fixtures::keyring().verify(&text).unwrap().payload(),
            &fixtures::payload()
        );
    }

    #[test]
    fn an_empty_keyring_trusts_nothing() {
        let keyring = Keyring::new([]).unwrap();
        assert_eq!(keyring.ids().count(), 0);
        assert_eq!(
            keyring.verify(&signed()),
            Err(Error::UnknownKey {
                id: fixtures::FIRST.into()
            })
        );
    }

    #[test]
    fn looks_up_keys_by_id() {
        let keyring = fixtures::keyring();
        let mut ids: Vec<&str> = keyring.ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, [fixtures::FIRST, fixtures::SECOND]);
        assert_eq!(
            keyring.get(fixtures::FIRST).unwrap().bytes(),
            fixtures::first().verifying_key().to_bytes()
        );
        assert!(keyring.get("2026-1").is_none());
    }

    #[test]
    fn refuses_a_duplicate_key_id() {
        let error = Keyring::new([
            fixtures::trusted(fixtures::FIRST, &fixtures::first()),
            fixtures::trusted(fixtures::FIRST, &fixtures::second()),
        ])
        .unwrap_err();
        assert_eq!(
            error,
            DuplicateKey {
                id: fixtures::FIRST.into()
            }
        );
    }

    #[test]
    #[should_panic(expected = "The production key broken is unusable")]
    fn an_invalid_compiled_in_key_panics() {
        let mut not_a_point = [0; PUBLIC_KEY_LENGTH];
        not_a_point[0] = 2;
        Keyring::trusting(&[("broken", not_a_point)]);
    }

    #[test]
    #[should_panic(expected = "The production keyring is invalid")]
    fn a_compiled_in_key_listed_twice_panics() {
        let public = fixtures::first().verifying_key().to_bytes();
        Keyring::trusting(&[(fixtures::FIRST, public), (fixtures::FIRST, public)]);
    }
}
