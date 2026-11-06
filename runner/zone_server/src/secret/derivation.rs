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
