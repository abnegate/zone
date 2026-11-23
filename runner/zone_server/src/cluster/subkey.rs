use abnegate_secret::MasterKey;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub const SUBKEY_BYTES: usize = 32;

/// HMAC-SHA256 keyed with the master key's hexadecimal over a purpose, the
/// same derivation the spend keys and planner grants use.
pub fn subkey(master: &MasterKey, purpose: &str) -> [u8; SUBKEY_BYTES] {
    let mut keyed = Hmac::<Sha256>::new_from_slice(master.to_hexadecimal().as_bytes())
        .expect("HMAC takes any key");
    keyed.update(purpose.as_bytes());
    keyed.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";
    const PURPOSE: &str = "zone-bus:sealing";

    #[test]
    fn subkeys_differ_by_purpose_and_master_key() {
        let master = secret::derive(PASSPHRASE).unwrap();
        let other = secret::derive(OTHER_PASSPHRASE).unwrap();

        assert_eq!(subkey(&master, PURPOSE), subkey(&master, PURPOSE));
        assert_ne!(
            subkey(&master, PURPOSE),
            subkey(&master, "zone-bus:namespace")
        );
        assert_ne!(subkey(&master, PURPOSE), subkey(&other, PURPOSE));
    }
}
