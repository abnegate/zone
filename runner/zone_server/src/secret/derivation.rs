use std::sync::Arc;
use std::sync::LazyLock;

use abnegate_secret::Error;
use abnegate_secret::KeyDerivation;
use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use sha2::Digest;
use sha2::Sha256;

const DOMAIN: &[u8] = b"zone-encryption-salt-v1";
const FINGERPRINT_BYTES: usize = 32;

static DERIVED: LazyLock<DashMap<[u8; FINGERPRINT_BYTES], Arc<MasterKey>>> =
    LazyLock::new(DashMap::new);

/// The key `passphrase` derives, memoised because Argon2id is memory-hard on purpose and every
/// `AppState` derives from the same configured passphrase.
pub fn derive(passphrase: &str) -> Result<Arc<MasterKey>, Error> {
    let slot = match DERIVED.entry(fingerprint(passphrase)) {
        Entry::Occupied(derived) => return Ok(Arc::clone(derived.get())),
        Entry::Vacant(slot) => slot,
    };
    let derivation = KeyDerivation::new(DOMAIN);
    let key = Arc::new(MasterKey::derive(
        &SecretValue::new(passphrase),
        &derivation,
    )?);
    slot.insert(Arc::clone(&key));
    Ok(key)
}

fn fingerprint(passphrase: &str) -> [u8; FINGERPRINT_BYTES] {
    Sha256::new()
        .chain_update(DOMAIN)
        .chain_update(passphrase.as_bytes())
        .finalize()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passphrase_derives_the_key_zone_has_always_derived_from_it() {
        let key = derive("a-high-entropy-passphrase-of-at-least-32-bytes").unwrap();
        assert_eq!(
            key.to_hexadecimal().as_str(),
            "8c6aa1bf541236b467250a20850eb50455008cb49b369358cfeeda94b30ca621"
        );
    }

    #[test]
    fn repeated_derivation_reuses_the_first_result() {
        let passphrase = "a-different-high-entropy-key-for-the-memoisation-test";
        let first = derive(passphrase).unwrap();

        assert!(
            DERIVED
                .get(&fingerprint(passphrase))
                .is_some_and(|derived| Arc::ptr_eq(derived.value(), &first)),
            "the derivation is kept so that Argon2id, which is memory-hard on purpose, runs \
             once per key rather than once per caller"
        );
        assert!(
            Arc::ptr_eq(&derive(passphrase).unwrap(), &first),
            "a memoised derivation still returns the derived key"
        );
    }

    #[test]
    fn an_empty_passphrase_derives_no_key() {
        assert!(matches!(derive(""), Err(Error::EmptyPassphrase)));
    }
}
