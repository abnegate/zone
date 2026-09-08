//! Parsing of embedded sources into version-ordered, checksummed migrations.

use super::checksum::Checksum;
use super::embedded::Source;
use super::error::MigrationError;

const SQL_SUFFIX: &str = ".sql";
const REVERSIBLE_UP_SUFFIX: &str = ".up.sql";
const REVERSIBLE_DOWN_SUFFIX: &str = ".down.sql";
const NO_TRANSACTION_PREFIX: &str = "-- no-transaction";
const VERSION_SEPARATOR: char = '_';
const DESCRIPTION_SEPARATOR: &str = " ";

#[derive(Clone, Debug)]
pub struct Migration {
    pub version: i64,
    pub description: String,
    pub checksum: Checksum,
    pub sql: &'static str,
    pub no_transaction: bool,
}

/// Parse every source and return them in version order.
///
/// Version parsing, the description transform and the `-- no-transaction` opt-out all
/// mirror `sqlx`'s directory resolver, so a migration resolved here and the same file
/// resolved by `zone_server` produce the same version, description and checksum.
pub fn resolve(sources: &[Source]) -> Result<Vec<Migration>, MigrationError> {
    let mut migrations = sources
        .iter()
        .map(parse)
        .collect::<Result<Vec<Migration>, MigrationError>>()?;

    migrations.sort_by_key(|migration| migration.version);

    if let Some(duplicate) = migrations
        .windows(2)
        .find(|pair| pair[0].version == pair[1].version)
    {
        return Err(MigrationError::DuplicateVersion {
            version: duplicate[0].version,
        });
    }

    Ok(migrations)
}

fn parse(source: &Source) -> Result<Migration, MigrationError> {
    let malformed = || MigrationError::MalformedFileName {
        file_name: source.file_name.to_owned(),
    };

    let (version, remainder) = source
        .file_name
        .split_once(VERSION_SEPARATOR)
        .ok_or_else(malformed)?;

    if !remainder.ends_with(SQL_SUFFIX) {
        return Err(malformed());
    }

    if remainder.ends_with(REVERSIBLE_UP_SUFFIX) || remainder.ends_with(REVERSIBLE_DOWN_SUFFIX) {
        return Err(MigrationError::ReversibleUnsupported {
            file_name: source.file_name.to_owned(),
        });
    }

    let version = version
        .parse::<i64>()
        .ok()
        .filter(|version| *version > 0)
        .ok_or_else(|| MigrationError::InvalidVersion {
            file_name: source.file_name.to_owned(),
        })?;

    Ok(Migration {
        version,
        description: remainder
            .trim_end_matches(SQL_SUFFIX)
            .replace(VERSION_SEPARATOR, DESCRIPTION_SEPARATOR),
        checksum: Checksum::of(source.sql),
        sql: source.sql,
        no_transaction: source.sql.starts_with(NO_TRANSACTION_PREFIX),
    })
}

#[cfg(test)]
mod tests {
    use super::{Migration, Source, resolve};
    use crate::migration::error::MigrationError;

    fn source(file_name: &'static str) -> Source {
        Source {
            file_name,
            sql: "SELECT 1;",
        }
    }

    fn parse_one(file_name: &'static str) -> Result<Migration, MigrationError> {
        resolve(&[source(file_name)]).map(|mut migrations| migrations.remove(0))
    }

    #[test]
    fn parses_version_and_description_like_sqlx() {
        let migration = parse_one("001_initial_schema.sql").unwrap();
        assert_eq!(migration.version, 1);
        assert_eq!(migration.description, "initial schema");
    }

    #[test]
    fn parses_a_multi_word_description() {
        let migration = parse_one("008_embedding_dimension_1024.sql").unwrap();
        assert_eq!(migration.version, 8);
        assert_eq!(migration.description, "embedding dimension 1024");
    }

    #[test]
    fn leading_zeroes_do_not_change_the_version() {
        assert_eq!(parse_one("0007_seven.sql").unwrap().version, 7);
    }

    #[test]
    fn rejects_a_file_name_without_a_version_separator() {
        assert!(matches!(
            parse_one("initial-schema.sql"),
            Err(MigrationError::MalformedFileName { .. })
        ));
    }

    #[test]
    fn rejects_a_file_name_that_is_not_sql() {
        assert!(matches!(
            parse_one("001_notes.md"),
            Err(MigrationError::MalformedFileName { .. })
        ));
    }

    #[test]
    fn rejects_a_non_numeric_version() {
        assert!(matches!(
            parse_one("first_schema.sql"),
            Err(MigrationError::InvalidVersion { .. })
        ));
    }

    #[test]
    fn rejects_an_empty_version() {
        assert!(matches!(
            parse_one("_schema.sql"),
            Err(MigrationError::InvalidVersion { .. })
        ));
    }

    #[test]
    fn rejects_a_non_positive_version() {
        assert!(matches!(
            parse_one("0_schema.sql"),
            Err(MigrationError::InvalidVersion { .. })
        ));
        assert!(matches!(
            parse_one("-3_schema.sql"),
            Err(MigrationError::InvalidVersion { .. })
        ));
    }

    #[test]
    fn rejects_reversible_migrations() {
        assert!(matches!(
            parse_one("001_schema.up.sql"),
            Err(MigrationError::ReversibleUnsupported { .. })
        ));
        assert!(matches!(
            parse_one("001_schema.down.sql"),
            Err(MigrationError::ReversibleUnsupported { .. })
        ));
    }

    #[test]
    fn orders_by_version_regardless_of_declaration_order() {
        let migrations = resolve(&[
            source("010_ten.sql"),
            source("002_two.sql"),
            source("001_one.sql"),
        ])
        .unwrap();

        let versions: Vec<i64> = migrations.iter().map(|m| m.version).collect();
        assert_eq!(versions, vec![1, 2, 10]);
    }

    #[test]
    fn a_gap_in_versions_is_allowed() {
        let migrations = resolve(&[
            source("001_one.sql"),
            source("005_five.sql"),
            source("009_nine.sql"),
        ])
        .unwrap();

        let versions: Vec<i64> = migrations.iter().map(|m| m.version).collect();
        assert_eq!(
            versions,
            vec![1, 5, 9],
            "squashed histories leave gaps; ordering must still hold"
        );
    }

    #[test]
    fn a_duplicate_version_is_refused() {
        let error = resolve(&[
            source("001_one.sql"),
            source("002_two.sql"),
            source("002_two_again.sql"),
        ])
        .unwrap_err();

        assert!(matches!(
            error,
            MigrationError::DuplicateVersion { version: 2 }
        ));
    }

    #[test]
    fn a_duplicate_version_is_refused_across_padding_differences() {
        let error = resolve(&[source("01_one.sql"), source("001_one.sql")]).unwrap_err();
        assert!(matches!(
            error,
            MigrationError::DuplicateVersion { version: 1 }
        ));
    }

    #[test]
    fn identical_sql_produces_identical_checksums() {
        let migrations = resolve(&[source("001_one.sql"), source("002_two.sql")]).unwrap();
        assert_eq!(migrations[0].checksum, migrations[1].checksum);
    }

    #[test]
    fn the_no_transaction_opt_out_is_detected() {
        let plain = resolve(&[source("001_one.sql")]).unwrap();
        assert!(!plain[0].no_transaction);

        let opted_out = resolve(&[Source {
            file_name: "001_one.sql",
            sql: "-- no-transaction\nCREATE INDEX CONCURRENTLY one ON two (three);",
        }])
        .unwrap();
        assert!(opted_out[0].no_transaction);
    }

    #[test]
    fn an_empty_source_set_resolves_to_nothing() {
        assert!(resolve(&[]).unwrap().is_empty());
    }
}
