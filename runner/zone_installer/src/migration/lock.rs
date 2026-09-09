//! The advisory lock key that serialises every migrator against one database.

use crc::{CRC_32_ISO_HDLC, Crc};

const CRC_32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

/// Salt and algorithm are copied from sqlx's Postgres migrator. Sharing the key is the
/// point: a client starting while `zone_server` migrates must block on the same lock
/// rather than take a private one and apply the same migration alongside it.
const LOCK_KEY_SALT: i64 = 0x3d32_ad9e;

pub fn identifier(database_name: &str) -> i64 {
    LOCK_KEY_SALT * i64::from(CRC_32.checksum(database_name.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::identifier;

    #[test]
    fn the_key_is_stable_for_a_database_name() {
        assert_eq!(
            identifier("zone"),
            identifier("zone"),
            "two processes must derive the same key"
        );
    }

    #[test]
    fn separate_databases_get_separate_keys() {
        assert_ne!(identifier("zone"), identifier("zone_test"));
    }

    #[test]
    fn the_key_matches_the_sqlx_derivation() {
        for (database_name, expected) in [
            ("zone", 2_771_974_297_191_923_538_i64),
            ("zone_test", 1_283_835_657_316_171_598_i64),
        ] {
            assert_eq!(
                identifier(database_name),
                expected,
                "expected 0x3d32ad9e * crc32({database_name:?}); changing this key would stop \
                 excluding the sqlx migrator"
            );
        }
    }

    #[test]
    fn the_longest_plausible_name_does_not_overflow() {
        let name = "z".repeat(63);
        assert!(identifier(&name) >= 0);
    }
}
