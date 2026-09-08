//! The pure decision of which embedded migrations a database still needs.

use std::collections::{HashMap, HashSet};

use super::error::MigrationError;
use super::ledger::AppliedMigration;
use super::resolve::Migration;

#[derive(Debug)]
pub struct Plan<'a> {
    pending: Vec<&'a Migration>,
}

impl<'a> Plan<'a> {
    /// Decide what to apply, refusing anything that would leave the two migration paths
    /// disagreeing: a half-applied migration, a database ahead of this binary, a file
    /// edited after it was applied, or a version that would land behind an applied one.
    pub fn new(
        migrations: &'a [Migration],
        applied: &[AppliedMigration],
    ) -> Result<Self, MigrationError> {
        if let Some(partial) = applied.iter().find(|entry| !entry.success) {
            return Err(MigrationError::PartiallyApplied {
                version: partial.version,
            });
        }

        let embedded_versions: HashSet<i64> = migrations
            .iter()
            .map(|migration| migration.version)
            .collect();

        if let Some(unknown) = applied
            .iter()
            .find(|entry| !embedded_versions.contains(&entry.version))
        {
            return Err(MigrationError::MissingFromBinary {
                version: unknown.version,
            });
        }

        let applied_by_version: HashMap<i64, &AppliedMigration> =
            applied.iter().map(|entry| (entry.version, entry)).collect();
        let highest_applied = applied.iter().map(|entry| entry.version).max();

        let mut pending = Vec::new();
        for migration in migrations {
            match applied_by_version.get(&migration.version) {
                Some(entry) if entry.checksum != migration.checksum => {
                    return Err(MigrationError::ChecksumMismatch {
                        version: migration.version,
                        applied: entry.checksum.clone(),
                        embedded: migration.checksum.clone(),
                    });
                }
                Some(_) => continue,
                None => {
                    if let Some(highest_applied) = highest_applied
                        && migration.version < highest_applied
                    {
                        return Err(MigrationError::OutOfOrder {
                            version: migration.version,
                            highest_applied,
                        });
                    }

                    pending.push(migration);
                }
            }
        }

        Ok(Self { pending })
    }

    pub fn pending(&self) -> &[&'a Migration] {
        &self.pending
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Plan;
    use crate::migration::checksum::Checksum;
    use crate::migration::embedded::Source;
    use crate::migration::error::MigrationError;
    use crate::migration::ledger::AppliedMigration;
    use crate::migration::resolve::{Migration, resolve};

    const SOURCES: &[Source] = &[
        Source {
            file_name: "001_one.sql",
            sql: "CREATE TABLE one ();",
        },
        Source {
            file_name: "002_two.sql",
            sql: "CREATE TABLE two ();",
        },
        Source {
            file_name: "004_four.sql",
            sql: "CREATE TABLE four ();",
        },
    ];

    fn migrations() -> Vec<Migration> {
        resolve(SOURCES).unwrap()
    }

    fn applied(migrations: &[Migration], versions: &[i64]) -> Vec<AppliedMigration> {
        migrations
            .iter()
            .filter(|migration| versions.contains(&migration.version))
            .map(|migration| AppliedMigration {
                version: migration.version,
                checksum: migration.checksum.clone(),
                success: true,
            })
            .collect()
    }

    fn pending_versions(plan: &Plan<'_>) -> Vec<i64> {
        plan.pending()
            .iter()
            .map(|migration| migration.version)
            .collect()
    }

    #[test]
    fn an_empty_applied_set_applies_everything_in_order() {
        let migrations = migrations();
        let plan = Plan::new(&migrations, &[]).unwrap();

        assert_eq!(pending_versions(&plan), vec![1, 2, 4]);
        assert!(!plan.is_empty());
    }

    #[test]
    fn only_the_unapplied_tail_is_pending() {
        let migrations = migrations();
        let applied = applied(&migrations, &[1, 2]);
        let plan = Plan::new(&migrations, &applied).unwrap();

        assert_eq!(pending_versions(&plan), vec![4]);
    }

    #[test]
    fn a_fully_migrated_database_has_nothing_pending() {
        let migrations = migrations();
        let applied = applied(&migrations, &[1, 2, 4]);
        let plan = Plan::new(&migrations, &applied).unwrap();

        assert!(plan.is_empty(), "re-running must be a no-op");
    }

    #[test]
    fn a_changed_migration_file_is_detected() {
        let migrations = migrations();
        let mut applied = applied(&migrations, &[1, 2]);
        applied[1].checksum = Checksum::of("CREATE TABLE two_but_edited ();");

        let error = Plan::new(&migrations, &applied).unwrap_err();

        assert!(
            matches!(error, MigrationError::ChecksumMismatch { version: 2, .. }),
            "an edited migration must not be silently re-applied, got {error}"
        );
    }

    #[test]
    fn an_out_of_order_migration_is_refused() {
        let migrations = migrations();
        let applied = applied(&migrations, &[1, 4]);

        let error = Plan::new(&migrations, &applied).unwrap_err();

        assert!(
            matches!(
                error,
                MigrationError::OutOfOrder {
                    version: 2,
                    highest_applied: 4
                }
            ),
            "a migration numbered below the applied high-water mark must be refused, got {error}"
        );
    }

    #[test]
    fn a_database_ahead_of_the_binary_is_refused() {
        let migrations = migrations();
        let mut applied = applied(&migrations, &[1, 2, 4]);
        applied.push(AppliedMigration {
            version: 9,
            checksum: Checksum::of("CREATE TABLE nine ();"),
            success: true,
        });

        let error = Plan::new(&migrations, &applied).unwrap_err();

        assert!(
            matches!(error, MigrationError::MissingFromBinary { version: 9 }),
            "an older binary must not run against a newer schema, got {error}"
        );
    }

    #[test]
    fn a_partially_applied_migration_is_refused() {
        let migrations = migrations();
        let mut applied = applied(&migrations, &[1, 2]);
        applied[1].success = false;

        let error = Plan::new(&migrations, &applied).unwrap_err();

        assert!(matches!(
            error,
            MigrationError::PartiallyApplied { version: 2 }
        ));
    }

    #[test]
    fn a_gap_in_versions_does_not_look_out_of_order() {
        let migrations = migrations();
        let applied = applied(&migrations, &[1, 2]);
        let plan = Plan::new(&migrations, &applied).unwrap();

        assert_eq!(
            pending_versions(&plan),
            vec![4],
            "version 3 never existing is not a reason to refuse version 4"
        );
    }

    #[test]
    fn an_empty_binary_against_an_empty_database_plans_nothing() {
        let plan = Plan::new(&[], &[]).unwrap();
        assert!(plan.is_empty());
    }
}
