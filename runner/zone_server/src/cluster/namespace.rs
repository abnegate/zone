use std::fmt;

use abnegate_secret::MasterKey;

use super::subkey::subkey;

const PURPOSE: &str = "zone-bus:namespace";
const HEXADECIMAL_LENGTH: usize = 16;

/// The prefix every channel of one cluster shares. Replicas with another
/// master key, or tests sharing one Valkey, land in a different namespace and
/// never see each other's channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace(String);

impl Namespace {
    pub fn derive(master: &MasterKey) -> Self {
        let mut hexadecimal = hex::encode(subkey(master, PURPOSE));
        hexadecimal.truncate(HEXADECIMAL_LENGTH);
        Self(hexadecimal)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::subkey::SUBKEY_BYTES;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const OTHER_PASSPHRASE: &str = "another-cluster-passphrase-of-32-bytes!";

    #[test]
    fn the_namespace_is_sixteen_hexadecimal_characters() {
        let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());

        assert_eq!(namespace.as_str().len(), HEXADECIMAL_LENGTH);
        assert!(
            namespace
                .as_str()
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        assert_eq!(
            namespace,
            Namespace::derive(&secret::derive(PASSPHRASE).unwrap())
        );
    }

    #[test]
    fn another_master_key_has_another_namespace() {
        assert_ne!(
            Namespace::derive(&secret::derive(PASSPHRASE).unwrap()),
            Namespace::derive(&secret::derive(OTHER_PASSPHRASE).unwrap())
        );
    }

    #[test]
    fn the_namespace_is_not_a_prefix_of_the_sealing_key() {
        let master = secret::derive(PASSPHRASE).unwrap();
        let sealing = hex::encode::<[u8; SUBKEY_BYTES]>(subkey(&master, "zone-bus:sealing"));

        assert!(!sealing.starts_with(Namespace::derive(&master).as_str()));
    }
}
