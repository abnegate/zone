use sqlx::PgExecutor;
use uuid::Uuid;

use crate::db::DbResult;

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
