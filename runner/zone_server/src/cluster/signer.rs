use abnegate_secret::MasterKey;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub const TAG_BYTES: usize = 32;

const PURPOSE: &str = "zone-bus:signing";

type Keyed = Hmac<Sha256>;

/// Signs and checks bus envelopes with a subkey of the master key, so only
/// replicas that share the master key can speak on the bus.
#[derive(Clone)]
pub struct Signer {
    keyed: Keyed,
}

impl Signer {
    pub fn derive(master: &MasterKey) -> Self {
        Self {
            keyed: Keyed::new_from_slice(&subkey(master, PURPOSE)).expect("HMAC takes any key"),
        }
    }

    /// The tag binds the body to the channel it was published on, so a
    /// signed envelope cannot be replayed onto another topic.
    pub fn sign(&self, channel: &str, body: &[u8]) -> [u8; TAG_BYTES] {
        self.bound(channel, body).finalize().into_bytes().into()
    }

    pub fn verify(&self, channel: &str, body: &[u8], tag: &[u8]) -> bool {
        self.bound(channel, body).verify_slice(tag).is_ok()
    }

    fn bound(&self, channel: &str, body: &[u8]) -> Keyed {
        let mut keyed = self.keyed.clone();
        keyed.update(&(channel.len() as u64).to_be_bytes());
        keyed.update(channel.as_bytes());
        keyed.update(body);
        keyed
    }
}

/// HMAC-SHA256 keyed with the master key's hexadecimal over a purpose, the
/// same derivation the spend keys and planner grants use.
pub fn subkey(master: &MasterKey, purpose: &str) -> [u8; TAG_BYTES] {
    let mut keyed =
        Keyed::new_from_slice(master.to_hexadecimal().as_bytes()).expect("HMAC takes any key");
    keyed.update(purpose.as_bytes());
    keyed.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";
    const CHANNEL: &str = "zone:0123456789abcdef:titles";

    #[test]
    fn a_signed_body_verifies_on_its_own_channel_only() {
        let signer = Signer::derive(&secret::derive(PASSPHRASE).unwrap());
        let tag = signer.sign(CHANNEL, b"body");

        assert!(signer.verify(CHANNEL, b"body", &tag));
        assert!(!signer.verify("zone:0123456789abcdef:actions", b"body", &tag));
        assert!(!signer.verify(CHANNEL, b"bodY", &tag));
        assert!(!signer.verify(CHANNEL, b"body", &tag[..TAG_BYTES - 1]));
    }

    #[test]
    fn another_master_key_cannot_verify() {
        let signer = Signer::derive(&secret::derive(PASSPHRASE).unwrap());
        let other = Signer::derive(&secret::derive(OTHER_PASSPHRASE).unwrap());

        assert!(!other.verify(CHANNEL, b"body", &signer.sign(CHANNEL, b"body")));
    }

    #[test]
    fn the_channel_length_is_bound_so_bytes_cannot_shift_between_channel_and_body() {
        let signer = Signer::derive(&secret::derive(PASSPHRASE).unwrap());
        let tag = signer.sign("zone:a", b"bc");

        assert!(!signer.verify("zone:ab", b"c", &tag));
    }

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
