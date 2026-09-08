//! SHA-384 digest of migration SQL, byte-compatible with the sqlx migrator.

use std::fmt;

use sha2::{Digest, Sha384};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checksum(Vec<u8>);

impl Checksum {
    /// SHA-384 over the raw SQL bytes is not a free choice: `zone_server` writes this
    /// exact digest into `_sqlx_migrations`, so any other algorithm would read every
    /// row it wrote as a modified migration and refuse to start.
    pub fn of(sql: &str) -> Self {
        Self(Sha384::digest(sql).to_vec())
    }

    pub fn from_stored(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Checksum;

    const SHA384_OF_ABC: &str = "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7";

    #[test]
    fn digest_matches_the_published_sha384_vector() {
        assert_eq!(
            Checksum::of("abc").to_string(),
            SHA384_OF_ABC,
            "checksum must stay SHA-384 to stay compatible with the sqlx ledger"
        );
    }

    #[test]
    fn digest_is_forty_eight_bytes() {
        assert_eq!(Checksum::of("select 1").as_bytes().len(), 48);
    }

    #[test]
    fn differing_sql_produces_differing_digests() {
        assert_ne!(Checksum::of("select 1"), Checksum::of("select 2"));
    }

    #[test]
    fn stored_bytes_round_trip() {
        let computed = Checksum::of("select 1");
        assert_eq!(
            Checksum::from_stored(computed.as_bytes().to_vec()),
            computed
        );
    }
}
