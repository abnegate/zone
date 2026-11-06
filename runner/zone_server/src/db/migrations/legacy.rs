//! Databases migrated before the v1 baseline, moved onto it in place.

use std::collections::BTreeSet;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::{Connection, PgConnection};

use super::unfinished::Unfinished;

pub const BASELINE: i64 = 1;

/// Every version and SHA-384 checksum sqlx recorded for a database migrated
/// through the last migration before the v1 baseline replaced them.
pub const HISTORY: &[(i64, &str)] = &[
    (
        1,
        "a26ca21be31134c909d818e556eaf38d4867c1a77babf265d562ad5fd7565968292808daba452204e7950adec6db72f5",
    ),
    (
        2,
        "e416dd49f83c1289672a41f4dd4a3c48d668d099edc173c574a2d19af889c0138688600bc1a625a291f585fe1901790a",
    ),
    (
        3,
        "c64528e5a9970bf71f0039c824f198383704ea3412941321b709276ed0d3bdfb644e5f723969d8ab577a8ad4b82b0509",
    ),
    (
        4,
        "955c4ae98b6dec280b147797be516182536cf45d6c1f637893a1fdb6069d2163e41aa1fa20c0f01b22eac11a15cc0e27",
    ),
    (
        5,
        "b328884fb65fc1c2c3f93aa6b3ca2d92da8774b335e7a752b5ae53946d2aa89587fe1106a41f703ebd64b7e19b58654f",
    ),
    (
        6,
        "da5956891e3326433791f224c8490fb0dd6c97eae3a7455bf92f701a3c8bc83e92a9b6a94875765006f9c39166778ec8",
    ),
    (
        7,
        "1d9f6c12082e2d3d4d812401b4ca131e7a5f2bb7d4448ca7adef6664788a8e938468f4662b67f6ed97d7d283c53ef131",
    ),
    (
        8,
        "ace23ec8e1f26fcfe45103b0ed7be4ba60e2c86afbbdec4f9233509997651fe7332a7277d04be85a250dfcbbcc2c42e3",
    ),
    (
        9,
        "702c4c63c2f2384848f5b14164c170b174a51cb6fb5467de93124dc9b044fa0907ca03be12026cff94fb0591ffdfd4ec",
    ),
    (
        10,
        "576bc5733bf748d9cf1d0a337cad2619000a2916eaadf9ab036a2355c904c2f2b063c3e3f239956f46182616dc75c4ec",
    ),
    (
        11,
        "b92d6b96cab77887928b8ec50fa2b05b4040dc037e6be47e531f157b383073337823e2344e0e0f19e3a38561d71ca7f0",
    ),
    (
        12,
        "a2c3bf946b0056dde28d0053703fab4d389a60954d777dad80c71fb367fce4922fb9e9ddb2990dd4c421acd2d9bfd201",
    ),
    (
        13,
        "760d6648976128623748119db38ed9108d3ea041b9a9270998c2f788200269bef1be2befbe8e47f56e7c5455e31a08b1",
    ),
    (
        14,
        "dbffd3957da76feb08151c5b77f72d503192c01809fc2ce5fd85f29076c6980d84a57f96c71443ef3a19f41592cf84c2",
    ),
    (
        15,
        "c3812b864113e39b2447ddaf94f1ce0e412a716fd270f7b1b19953a91935501cc9004f941c70232e313177586a8de6b7",
    ),
    (
        17,
        "8dc8a7991e604504354eb347e1133cb0133fda115ca83be26c987a009211deed65a0eb99329027be6d30140d430573b5",
    ),
    (
        18,
        "cd2000c30cb4c4136a0427ee145f5d49264899fb89cd9386c077a22d703dd0de1c6d8e6c930d4b46ca21c022b83279f3",
    ),
    (
        19,
        "06f82753e6335862951182a4726dc611d6c6196685b8dc3aa0fa3d2a17691c980e17688a9b24561767b82d5d4f3e257f",
    ),
    (
        20,
        "2d1cccde8400a2a1576cb7a7ed15482cb31a909d1a6330be707d39c828e000f37a9ea75f0301f0f882a864686a107416",
    ),
    (
        21,
        "fc72adde26ff52e9baea829a56c374aee5c7ecef149de94218bcddaf415ac2b177f69e7347b360ef8e906c27442e43f1",
    ),
    (
        22,
        "bd4b19f1d998d3d75a28a247002810bf0f8da4d3e96bc78c4b61a0aef4079fc942b6eb4fbdc76dbee05c6632a79fdc04",
    ),
    (
        23,
        "839bff8012b162217f00679ab409f4f389e3350491217d582a23016553175a6c88104bfdc398b361d061cfc931b7d107",
    ),
    (
        24,
        "a225002471aad03a7e394d2ba3ea6db8a808278fb0738ea162b469714c0dca75f735a548d21824055716134fda29046e",
    ),
];

const LEDGER_EXISTS: &str = "SELECT to_regclass('_sqlx_migrations') IS NOT NULL";
const SELECT_LEDGER: &str =
    "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version";
const DELETE_SUPERSEDED: &str = "DELETE FROM _sqlx_migrations WHERE version <> $1";
const RECORD_BASELINE: &str = "UPDATE _sqlx_migrations \
     SET description = $2, checksum = $3, execution_time = 0 WHERE version = $1";

/// Whether `ledger` is the complete pre-v1 history, which the baseline's schema
/// matches exactly. A ledger holding none of it is left to sqlx. One holding
/// only part of it, or more besides, is refused rather than guessed at.
pub fn superseded(ledger: &[(i64, Vec<u8>, bool)]) -> Result<bool, Unfinished> {
    let history: BTreeSet<(i64, Vec<u8>)> = HISTORY
        .iter()
        .map(|(version, checksum)| {
            let checksum = hex::decode(checksum).expect("pre-v1 checksums are hex");
            (*version, checksum)
        })
        .collect();
    let (recorded, unexpected): (Vec<_>, Vec<_>) =
        ledger.iter().partition(|(version, checksum, success)| {
            *success && history.contains(&(*version, checksum.clone()))
        });
    if recorded.is_empty() {
        return Ok(false);
    }
    if recorded.len() == history.len() && unexpected.is_empty() {
        return Ok(true);
    }
    let applied: BTreeSet<i64> = recorded.iter().map(|(version, _, _)| *version).collect();
    Err(Unfinished {
        missing: history
            .iter()
            .map(|(version, _)| *version)
            .filter(|version| !applied.contains(version))
            .collect(),
        unexpected: unexpected.iter().map(|(version, _, _)| *version).collect(),
    })
}

/// Collapse a complete pre-v1 ledger into the single baseline row, keeping the
/// schema and every row of data as they are.
pub async fn rebase(
    connection: &mut PgConnection,
    migrator: &Migrator,
) -> Result<(), MigrateError> {
    let exists: bool = sqlx::query_scalar(LEDGER_EXISTS)
        .fetch_one(&mut *connection)
        .await?;
    if !exists {
        return Ok(());
    }
    let ledger: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(SELECT_LEDGER)
        .fetch_all(&mut *connection)
        .await?;
    if !superseded(&ledger).map_err(|error| MigrateError::Source(Box::new(error)))? {
        return Ok(());
    }
    let baseline = migrator
        .iter()
        .find(|migration| migration.version == BASELINE)
        .ok_or(MigrateError::VersionNotPresent(BASELINE))?;
    let mut transaction = connection.begin().await?;
    sqlx::query(DELETE_SUPERSEDED)
        .bind(BASELINE)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(RECORD_BASELINE)
        .bind(BASELINE)
        .bind(baseline.description.as_ref())
        .bind(baseline.checksum.as_ref())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    tracing::info!("moved the pre-v1 migration ledger onto the v1 baseline");
    Ok(())
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha384};

    use super::{HISTORY, Unfinished, superseded};

    const BASELINE_CHECKSUM: &str = "2dfdc59a18685543a92caee401199580da07b3cfb448d54a67c9f2e382f81059d56cd76bac7b9ce214e04b750f6854de";

    fn history() -> Vec<(i64, Vec<u8>, bool)> {
        HISTORY
            .iter()
            .map(|(version, checksum)| (*version, hex::decode(checksum).unwrap(), true))
            .collect()
    }

    #[test]
    fn the_v1_baseline_is_frozen() {
        let baseline = include_bytes!("../../../migrations/0001_v1.sql");
        assert_eq!(
            hex::encode(Sha384::digest(baseline)),
            BASELINE_CHECKSUM,
            "0001_v1.sql changed: every database migrated since v1 records its checksum and \
             would refuse to boot. Add a new migration instead."
        );
    }

    #[test]
    fn the_pre_v1_history_ends_at_24_and_skips_16() {
        let versions: Vec<i64> = HISTORY.iter().map(|(version, _)| *version).collect();
        let expected: Vec<i64> = (1..=24).filter(|version| *version != 16).collect();
        assert_eq!(versions, expected);
        for (version, checksum) in HISTORY {
            assert_eq!(
                hex::decode(checksum).unwrap().len(),
                48,
                "checksum of {version}"
            );
        }
    }

    #[test]
    fn a_fresh_or_baselined_ledger_is_left_to_sqlx() {
        assert_eq!(superseded(&[]), Ok(false));
        let baselined = [(1, hex::decode(BASELINE_CHECKSUM).unwrap(), true)];
        assert_eq!(superseded(&baselined), Ok(false));
    }

    #[test]
    fn the_complete_pre_v1_history_is_superseded() {
        assert_eq!(superseded(&history()), Ok(true));
    }

    #[test]
    fn a_ledger_short_of_the_last_pre_v1_migration_is_refused() {
        let mut ledger = history();
        ledger.pop();
        assert_eq!(
            superseded(&ledger),
            Err(Unfinished {
                missing: vec![24],
                unexpected: vec![],
            })
        );
    }

    #[test]
    fn a_ledger_with_a_migration_from_outside_main_is_refused() {
        let mut ledger = history();
        ledger.push((25, vec![0; 48], true));
        assert_eq!(
            superseded(&ledger),
            Err(Unfinished {
                missing: vec![],
                unexpected: vec![25],
            })
        );
    }

    #[test]
    fn a_ledger_with_a_failed_pre_v1_migration_is_refused() {
        let mut ledger = history();
        ledger[3].2 = false;
        assert_eq!(
            superseded(&ledger),
            Err(Unfinished {
                missing: vec![4],
                unexpected: vec![4],
            })
        );
    }
}
