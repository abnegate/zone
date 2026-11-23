use abnegate_secret::MasterKey;
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Key, Tag, XChaCha20Poly1305, XNonce};

use super::subkey::subkey;

pub const NONCE_BYTES: usize = 24;
pub const TAG_BYTES: usize = 16;

const PURPOSE: &str = "zone-bus:sealing";

/// Encrypts and authenticates bus envelopes with XChaCha20-Poly1305 under a
/// subkey of the master key, so only replicas that share the master key can
/// read the bus or speak on it.
#[derive(Clone)]
pub struct Sealer {
    cipher: XChaCha20Poly1305,
}

impl Sealer {
    pub fn derive(master: &MasterKey) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new(&Key::from(subkey(master, PURPOSE))),
        }
    }

    /// A random nonce. At 192 bits, random nonces do not repeat however many
    /// envelopes one key seals.
    pub fn nonce() -> [u8; NONCE_BYTES] {
        let mut nonce = [0u8; NONCE_BYTES];
        rand::fill(&mut nonce);
        nonce
    }

    /// Encrypts `body` in place and returns the tag that authenticates it
    /// with `framing` and the channel it is published on, so a sealed
    /// envelope cannot be replayed onto another topic.
    pub fn seal(
        &self,
        nonce: &[u8; NONCE_BYTES],
        channel: &str,
        framing: &[u8],
        body: &mut [u8],
    ) -> [u8; TAG_BYTES] {
        self.cipher
            .encrypt_inout_detached(
                &XNonce::from(*nonce),
                &associated(channel, framing),
                body.into(),
            )
            .expect("an envelope is far shorter than the cipher's limit")
            .into()
    }

    /// Decrypts `body` in place when `tag` authenticates it with `framing` and
    /// `channel`. False otherwise, and `body` is then garbage.
    pub fn open(
        &self,
        nonce: &[u8; NONCE_BYTES],
        channel: &str,
        framing: &[u8],
        body: &mut [u8],
        tag: &[u8; TAG_BYTES],
    ) -> bool {
        self.cipher
            .decrypt_inout_detached(
                &XNonce::from(*nonce),
                &associated(channel, framing),
                body.into(),
                &Tag::from(*tag),
            )
            .is_ok()
    }
}

/// The channel, prefixed by its length so no byte can shift between it and
/// the framing, then the framing.
fn associated(channel: &str, framing: &[u8]) -> Vec<u8> {
    let mut associated = Vec::with_capacity(size_of::<u64>() + channel.len() + framing.len());
    associated.extend_from_slice(&(channel.len() as u64).to_be_bytes());
    associated.extend_from_slice(channel.as_bytes());
    associated.extend_from_slice(framing);
    associated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";
    const CHANNEL: &str = "zone:0123456789abcdef:titles";
    const OTHER_CHANNEL: &str = "zone:0123456789abcdef:actions";
    const FRAMING: &[u8] = b"framing";
    const BODY: &[u8] = b"a body nobody else may read";

    fn sealer() -> Sealer {
        Sealer::derive(&secret::derive(PASSPHRASE).unwrap())
    }

    fn sealed(sealer: &Sealer, nonce: &[u8; NONCE_BYTES]) -> (Vec<u8>, [u8; TAG_BYTES]) {
        let mut body = BODY.to_vec();
        let tag = sealer.seal(nonce, CHANNEL, FRAMING, &mut body);
        (body, tag)
    }

    #[test]
    fn a_sealed_body_opens_to_itself_and_hides_its_plaintext() {
        let sealer = sealer();
        let nonce = Sealer::nonce();
        let (mut body, tag) = sealed(&sealer, &nonce);

        assert_ne!(body, BODY);
        assert!(sealer.open(&nonce, CHANNEL, FRAMING, &mut body, &tag));
        assert_eq!(body, BODY);
    }

    #[test]
    fn a_body_opens_only_with_its_own_channel_framing_nonce_and_tag() {
        let sealer = sealer();
        let nonce = Sealer::nonce();
        let (body, tag) = sealed(&sealer, &nonce);
        let mut flipped = tag;
        flipped[0] ^= 0x01;
        let opens = |nonce: &[u8; NONCE_BYTES], channel: &str, framing: &[u8], tag| {
            sealer.open(nonce, channel, framing, &mut body.clone(), tag)
        };

        assert!(!opens(&nonce, OTHER_CHANNEL, FRAMING, &tag));
        assert!(!opens(&nonce, CHANNEL, b"framinG", &tag));
        assert!(!opens(&Sealer::nonce(), CHANNEL, FRAMING, &tag));
        assert!(!opens(&nonce, CHANNEL, FRAMING, &flipped));
        assert!(opens(&nonce, CHANNEL, FRAMING, &tag));
    }

    #[test]
    fn the_channel_length_is_bound_so_bytes_cannot_shift_into_the_framing() {
        let sealer = sealer();
        let nonce = Sealer::nonce();
        let mut body = BODY.to_vec();
        let tag = sealer.seal(&nonce, "zone:a", b"bc", &mut body);

        assert!(!sealer.open(&nonce, "zone:ab", b"c", &mut body, &tag));
    }

    #[test]
    fn another_master_key_cannot_open() {
        let nonce = Sealer::nonce();
        let (mut body, tag) = sealed(&sealer(), &nonce);
        let other = Sealer::derive(&secret::derive(OTHER_PASSPHRASE).unwrap());

        assert!(!other.open(&nonce, CHANNEL, FRAMING, &mut body, &tag));
    }

    #[test]
    fn nonces_are_random() {
        assert_ne!(Sealer::nonce(), Sealer::nonce());
    }
}
