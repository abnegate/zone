use std::sync::Arc;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::decrypt_value;
use abnegate_secret::encrypt_value;
use abnegate_secret::is_encrypted;
use sqlx::PgConnection;
use sqlx::PgPool;
use uuid::Uuid;

use super::backup::Backup;
use super::classified::Classified;
use super::column::Column;
use super::error::SealingError;
use super::options::Options;
use super::outcome::Outcome;
use super::report::Report;

/// How many rows of a column are read, and written in one transaction, at a time.
pub const BATCH_SIZE: usize = 100;

const CURRENT_DATABASE: &str = "SELECT current_database()";

/// Every table of the public schema whose rows `pg_dump` dumps: ordinary tables, and not those an
/// extension owns.
const PUBLIC_TABLES: &str = "SELECT class.relname::text FROM pg_class class \
     JOIN pg_namespace namespace ON namespace.oid = class.relnamespace \
     WHERE namespace.nspname = 'public' AND class.relkind = 'r' \
     AND NOT EXISTS (SELECT 1 FROM pg_depend dependency \
         WHERE dependency.classid = 'pg_class'::regclass \
         AND dependency.objid = class.oid AND dependency.deptype = 'e') \
     ORDER BY class.relname";

/// Seals every credential column of one database under one key.
pub struct Sealer {
    pool: PgPool,
    key: Arc<MasterKey>,
    options: Options,
}

impl Sealer {
    /// A sealer of the database `pool` connects to, sealing under `key`.
    pub fn new(pool: PgPool, key: Arc<MasterKey>, options: Options) -> Self {
        Self { pool, key, options }
    }

    /// Verify the backup holds every table of the database, count every column, then, unless
    /// this is a dry run, seal.
    ///
    /// A run, dry or not, is refused when no value that looks sealed opens under the key,
    /// since that is what the wrong `ENCRYPTION_KEY` looks like. Each batch commits on its
    /// own, so a run that stops part way resumes where it stopped: what it committed is
    /// skipped as envelopes.
    pub async fn run(&self) -> Result<Report, SealingError> {
        let database: String = sqlx::query_scalar(CURRENT_DATABASE)
            .fetch_one(&self.pool)
            .await?;
        let tables: Vec<String> = sqlx::query_scalar(PUBLIC_TABLES)
            .fetch_all(&self.pool)
            .await?;
        let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
        let backup = Backup::verify(self.options.backup(), &database, &tables).await?;

        let survey = self.pass(backup.clone(), false).await?;
        if let Some(unopened) = survey.wrong_key() {
            return Err(SealingError::KeyMismatch { unopened });
        }
        if survey.key_untested() {
            tracing::warn!(
                "No stored value looks sealed, so nothing shows this ENCRYPTION_KEY is the server's"
            );
        }
        if self.options.dry_run() {
            return Ok(survey);
        }
        self.pass(backup, true).await
    }

    async fn pass(&self, backup: Backup, writing: bool) -> Result<Report, SealingError> {
        let mut report = Report::new(backup, !writing);
        for column in Column::ALL {
            let mut after = None;
            loop {
                let rows = column.page(&self.pool, after, BATCH_SIZE).await?;
                let Some((last, _)) = rows.last() else {
                    break;
                };
                after = Some(*last);
                let complete = rows.len() < BATCH_SIZE;
                if writing {
                    let mut transaction = self.pool.begin().await?;
                    for (id, stored) in &rows {
                        self.settle(*column, &mut transaction, *id, stored, &mut report)
                            .await?;
                    }
                    transaction.commit().await?;
                } else {
                    for (_, stored) in &rows {
                        let classified = Classified::of(stored, &self.key);
                        let planned = Outcome::planned(column.policy(), classified.class());
                        report.record(*column, classified.class(), planned);
                    }
                }
                if complete {
                    break;
                }
            }
        }
        Ok(report)
    }

    async fn settle(
        &self,
        column: Column,
        connection: &mut PgConnection,
        id: Uuid,
        stored: &str,
        report: &mut Report,
    ) -> Result<(), SealingError> {
        let classified = Classified::of(stored, &self.key);
        let planned = Outcome::planned(column.policy(), classified.class());
        let outcome = match classified.credential() {
            Some(credential) if planned.writes() => {
                let sealed = encrypt_value(credential, &self.key)?;
                match verify(sealed, credential, &self.key) {
                    Ok(sealed) => {
                        if column.replace(connection, id, stored, &sealed).await? {
                            planned
                        } else {
                            tracing::warn!(%column, %id, "The row changed after it was read; the next run seals it");
                            Outcome::Raced
                        }
                    }
                    Err(mismatched) => {
                        tracing::error!(%column, %id, "A sealed credential did not reopen as itself; the row is unchanged");
                        mismatched
                    }
                }
            }
            _ => planned,
        };
        report.record(column, classified.class(), outcome);
        Ok(())
    }
}

/// `sealed` when it is an envelope that reopens under `key` as `credential`, otherwise
/// [`Outcome::Mismatched`].
fn verify(sealed: String, credential: &SecretValue, key: &MasterKey) -> Result<String, Outcome> {
    match decrypt_value(&sealed, key) {
        Ok(opened) if is_encrypted(&sealed) && opened == *credential => Ok(sealed),
        _ => Err(Outcome::Mismatched),
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::MasterKey;
    use abnegate_secret::SecretValue;

    use crate::secret::fixture::seal;

    use super::Outcome;
    use super::verify;

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    #[test]
    fn a_seal_that_reopens_as_the_credential_is_written() {
        let key = key();
        let sealed = seal("ghp_token", &key);
        assert_eq!(
            verify(sealed.clone(), &SecretValue::new("ghp_token"), &key),
            Ok(sealed)
        );
    }

    #[test]
    fn a_seal_that_reopens_as_anything_else_is_mismatched() {
        let key = key();
        assert_eq!(
            verify(
                seal("ghp_token", &key),
                &SecretValue::new("ghp_other"),
                &key
            ),
            Err(Outcome::Mismatched)
        );
    }

    #[test]
    fn a_seal_under_another_key_is_mismatched() {
        assert_eq!(
            verify(
                seal("ghp_token", &key()),
                &SecretValue::new("ghp_token"),
                &key()
            ),
            Err(Outcome::Mismatched)
        );
    }

    #[test]
    fn text_that_is_not_an_envelope_is_mismatched() {
        assert_eq!(
            verify(
                "ghp_token".to_string(),
                &SecretValue::new("ghp_token"),
                &key()
            ),
            Err(Outcome::Mismatched)
        );
    }
}
