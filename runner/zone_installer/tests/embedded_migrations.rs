//! Guards the embedded migration set against the files the server migrates from.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use zone_installer::migration::{EMBEDDED, MIGRATIONS_DIRECTORY, embedded};

const SQL_EXTENSION: &str = "sql";

fn migrations_on_disk() -> BTreeMap<String, String> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(MIGRATIONS_DIRECTORY);

    let entries = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));

    entries
        .map(|entry| entry.expect("cannot read migration directory entry").path())
        .filter(|path| path.extension().is_some_and(|kind| kind == SQL_EXTENSION))
        .map(|path| {
            let file_name = path
                .file_name()
                .expect("migration path has no file name")
                .to_string_lossy()
                .into_owned();
            let sql = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            (file_name, sql)
        })
        .collect()
}

#[test]
fn the_embedded_set_matches_the_files_on_disk() {
    let on_disk = migrations_on_disk();
    let compiled_in: BTreeMap<&str, &str> = EMBEDDED
        .iter()
        .map(|source| (source.file_name, source.sql))
        .collect();

    let missing: Vec<&String> = on_disk
        .keys()
        .filter(|file_name| !compiled_in.contains_key(file_name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "these migrations exist on disk but are not embedded, so the client would start on an \
         older schema than the server: {missing:?}. Add them to src/migration/embedded.rs."
    );

    let extra: Vec<&&str> = compiled_in
        .keys()
        .filter(|file_name| !on_disk.contains_key(**file_name))
        .collect();
    assert!(
        extra.is_empty(),
        "these migrations are embedded but no longer exist on disk: {extra:?}"
    );

    for (file_name, sql) in &on_disk {
        assert_eq!(
            compiled_in[file_name.as_str()],
            sql.as_str(),
            "embedded copy of {file_name} differs from the file on disk"
        );
    }
}

#[test]
fn the_embedded_set_resolves_to_a_contiguous_ordered_history() {
    let migrations = embedded().expect("embedded migrations must resolve");

    assert!(!migrations.is_empty());

    let versions: Vec<i64> = migrations.iter().map(|m| m.version).collect();
    let mut sorted = versions.clone();
    sorted.sort_unstable();
    assert_eq!(versions, sorted, "migrations must resolve in version order");

    assert_eq!(migrations[0].version, 1, "history must start at version 1");
    assert_eq!(
        migrations.len(),
        EMBEDDED.len(),
        "every embedded source must resolve to exactly one migration"
    );
}

#[test]
fn every_embedded_migration_carries_sql_and_a_description() {
    for migration in embedded().expect("embedded migrations must resolve") {
        assert!(
            !migration.sql.trim().is_empty(),
            "migration {} is empty",
            migration.version
        );
        assert!(
            !migration.description.is_empty(),
            "migration {} has no description",
            migration.version
        );
        assert_eq!(migration.checksum.as_bytes().len(), 48);
    }
}

/// The strongest guarantee available without a database: resolve the same directory with
/// sqlx's own resolver — the one `zone_server` uses — and require it to agree with the
/// embedded set on every version, description, checksum and byte of SQL. If these ever
/// disagree the two migration paths have become two schemas.
#[tokio::test]
async fn the_embedded_set_resolves_identically_to_the_sqlx_migrator() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(MIGRATIONS_DIRECTORY);
    let migrator = sqlx::migrate::Migrator::new(directory.as_path())
        .await
        .expect("sqlx must resolve the server's migration directory");

    let ours = embedded().expect("embedded migrations must resolve");
    let theirs: Vec<&sqlx::migrate::Migration> = migrator.iter().collect();

    assert_eq!(
        ours.len(),
        theirs.len(),
        "embedded set and sqlx resolved a different number of migrations"
    );

    for (ours, theirs) in ours.iter().zip(theirs) {
        assert_eq!(ours.version, theirs.version, "versions must agree");
        assert_eq!(
            ours.description,
            theirs.description.as_ref(),
            "description of migration {} must agree",
            ours.version
        );
        assert_eq!(
            ours.checksum.as_bytes(),
            theirs.checksum.as_ref(),
            "checksum of migration {} must agree, or the ledgers cannot be shared",
            ours.version
        );
        assert_eq!(
            ours.sql,
            theirs.sql.as_str(),
            "SQL of migration {} must agree byte for byte",
            ours.version
        );
        assert_eq!(
            ours.no_transaction, theirs.no_tx,
            "transaction handling of migration {} must agree",
            ours.version
        );
    }
}
