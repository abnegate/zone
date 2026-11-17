//! Schema upgrades.

use sqlx::PgPool;
use sqlx::migrate::{Migrate, MigrateError};

pub async fn run(pool: &PgPool) -> Result<(), MigrateError> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let timeout: String = sqlx::query_scalar("SHOW lock_timeout")
        .fetch_one(&mut *connection)
        .await?;
    // A blocking advisory-lock SELECT holds a virtual transaction. End each
    // wait promptly so two boots cannot wedge each other.
    sqlx::query("SET lock_timeout = '100ms'")
        .execute(&mut *connection)
        .await?;
    loop {
        match connection.lock().await {
            Ok(()) => break,
            Err(MigrateError::Execute(error))
                if error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref()
                    .is_some_and(|code| matches!(code, "55P03" | "40P01")) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(error) => return Err(error),
        }
    }
    sqlx::query("SELECT set_config('lock_timeout', $1, false)")
        .bind(timeout)
        .execute(&mut *connection)
        .await?;
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_locking(false);
    let result = migrator.run_direct(None, &mut *connection, false).await;
    let unlocked = connection.unlock().await;
    result.and(unlocked)?;
    connection.close().await?;
    super::recovery::reconcile(pool).await?;
    Ok(())
}
