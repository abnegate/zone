use ed25519_dalek::PUBLIC_KEY_LENGTH;

pub(crate) const KEYS: [(&str, [u8; PUBLIC_KEY_LENGTH]); 1] = [(
    "2026-1",
    [
        0x84, 0xb4, 0x53, 0x12, 0x85, 0x14, 0x02, 0x8f, 0xae, 0x0c, 0xc3, 0xcf, 0xbd, 0xff, 0x5a,
        0xc0, 0xa3, 0xc0, 0xd6, 0x1e, 0xb0, 0x3c, 0x1b, 0xbc, 0x4b, 0x50, 0xba, 0x02, 0xb6, 0x7c,
        0x5c, 0x04,
    ],
)];

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    use crate::error::Error;
    use crate::fixtures;
    use crate::keyring::Keyring;
    use crate::payload::Payload;
    use crate::signing::sign;
    use crate::trusted_key::TrustedKey;

    const ID: &str = "2026-1";
    const SOURCE: &str = include_str!("production.rs");

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_compiled_in_keys_build_a_keyring() {
        assert_eq!(Keyring::production().ids().count(), super::KEYS.len());
    }

    #[test]
    fn holds_exactly_the_2026_key() {
        let keyring = Keyring::production();
        assert_eq!(keyring.ids().collect::<Vec<_>>(), [ID]);
    }

    #[test]
    fn key_matches_the_published_public_key() {
        let published = STANDARD
            .decode("hLRTEoUUAo+uDMPPvf9awKPA1h6wPBu8S1C6ArZ8XAQ=")
            .unwrap();
        assert_eq!(
            Keyring::production().get(ID).unwrap().bytes().as_slice(),
            published.as_slice()
        );
    }

    #[test]
    fn every_listed_key_is_a_valid_strong_point() {
        for (id, public) in super::KEYS {
            assert!(
                TrustedKey::new(id, public).is_ok(),
                "{id} is invalid or weak"
            );
        }
    }

    #[test]
    fn source_holds_no_test_key() {
        let listed: String = SOURCE
            .split("0x")
            .skip(1)
            .filter_map(|token| token.get(..2))
            .collect::<String>()
            .to_lowercase();
        let flattened = SOURCE.to_lowercase();
        for key in [fixtures::first(), fixtures::second()] {
            let public = key.verifying_key().to_bytes();
            let hex = hex(&public);
            assert!(!listed.contains(&hex), "a test key is listed as bytes");
            assert!(!flattened.contains(&hex), "a test key appears as hex");
            for engine in [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD] {
                let encoded = engine.encode(public);
                assert!(
                    !SOURCE.contains(&encoded),
                    "a test key appears as base64 {encoded}"
                );
            }
        }
    }

    #[test]
    fn source_holds_the_production_key_as_bytes() {
        let listed: String = SOURCE
            .split("0x")
            .skip(1)
            .filter_map(|token| token.get(..2))
            .collect();
        assert!(listed.contains(&hex(&super::KEYS[0].1)));
    }

    #[test]
    fn refuses_a_test_signed_license_claiming_the_production_key() {
        let text = sign(
            &Payload {
                key_id: ID.into(),
                ..fixtures::payload()
            },
            &fixtures::first(),
        );
        assert_eq!(Keyring::production().verify(&text), Err(Error::Signature));
    }

    #[test]
    fn refuses_a_test_key_license() {
        let text = sign(&fixtures::payload(), &fixtures::first());
        assert_eq!(
            Keyring::production().verify(&text),
            Err(Error::UnknownKey {
                id: fixtures::FIRST.into()
            })
        );
    }
}
