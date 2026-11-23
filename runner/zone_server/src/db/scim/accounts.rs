use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::users;
use crate::db::DbResult;

pub async fn renamable(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    let administrator: Option<bool> =
        sqlx::query_scalar("SELECT is_admin IS TRUE FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *connection)
            .await?;
    if administrator != Some(false) {
        return Ok(false);
    }
    Ok(users::pristine(&mut *connection, user_id).await?
        && users::sole_organization(&mut *connection, organization_id, user_id).await?)
}

pub async fn update(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    email: Option<&str>,
    display_name: Option<Option<&str>>,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE users
        SET email = COALESCE(lower($2), email),
            email_verified = CASE
                WHEN lower($2) IS DISTINCT FROM email AND $2 IS NOT NULL THEN FALSE
                ELSE email_verified
            END,
            display_name = CASE WHEN $3 THEN $4 ELSE display_name END,
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .bind(email)
    .bind(display_name.is_some())
    .bind(display_name.flatten())
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}
