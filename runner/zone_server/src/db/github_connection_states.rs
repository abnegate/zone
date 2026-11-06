//! Single-use `state` values that tie a GitHub redirect back to the Zone user who started it.

use abnegate_secret::SecretValue;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use crate::utils::crypto::hash_token;

pub const LIFETIME: TimeDelta = TimeDelta::hours(1);

const STATE_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Registration,
    Installation,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::Installation => "installation",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Issued {
    pub state: SecretValue,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ConsumedState {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub github_app_id: Option<Uuid>,
}

/// Only the SHA-256 of the state is stored. Expired states are pruned in the
/// same statement.
pub async fn issue(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    purpose: Purpose,
    github_app_id: Option<Uuid>,
) -> DbResult<Issued> {
    let mut bytes = [0u8; STATE_BYTES];
    rand::fill(&mut bytes);
    let state = SecretValue::new(URL_SAFE_NO_PAD.encode(bytes));

    let expires_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        "WITH pruned AS (DELETE FROM github_connection_states WHERE expires_at <= NOW()) \
         INSERT INTO github_connection_states \
           (nonce_hash, organization_id, user_id, github_app_id, purpose, expires_at) \
         VALUES ($1, $2, $3, $4, $5, NOW() + make_interval(secs => $6)) \
         RETURNING expires_at",
    )
    .bind(hash_token(state.expose()))
    .bind(organization_id)
    .bind(user_id)
    .bind(github_app_id)
    .bind(purpose.as_str())
    .bind(LIFETIME.as_seconds_f64())
    .fetch_one(pool)
    .await?;

    Ok(Issued { state, expires_at })
}

/// Deletes the matching live state, so a second redirect with the same value finds nothing.
pub async fn consume(
    pool: &PgPool,
    state: &str,
    purpose: Purpose,
) -> DbResult<Option<ConsumedState>> {
    sqlx::query_as::<_, ConsumedState>(
        "DELETE FROM github_connection_states \
         WHERE nonce_hash = $1 AND purpose = $2 AND expires_at > NOW() \
         RETURNING organization_id, user_id, github_app_id",
    )
    .bind(hash_token(state))
    .bind(purpose.as_str())
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::github_apps::testing::{pool, seed_app, seed_organization, seed_user};

    async fn stored(pool: &PgPool, state: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM github_connection_states WHERE nonce_hash = $1")
            .bind(hash_token(state))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_state_is_consumed_once() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;
        let app = seed_app(&pool, organization, "acme").await;

        let issued = issue(
            &pool,
            organization,
            user,
            Purpose::Installation,
            Some(app.id),
        )
        .await
        .unwrap();
        let state = issued.state.expose();
        assert_eq!(state.len(), 43);
        assert!(
            URL_SAFE_NO_PAD
                .decode(state)
                .is_ok_and(|bytes| bytes.len() == STATE_BYTES)
        );
        let lifetime = issued.expires_at - Utc::now();
        assert!(lifetime > LIFETIME - TimeDelta::minutes(1), "{lifetime}");
        assert!(lifetime <= LIFETIME, "{lifetime}");

        let plaintext: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM github_connection_states WHERE nonce_hash = $1",
        )
        .bind(state)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(plaintext, 0, "the state itself must never be stored");

        let consumed = consume(&pool, state, Purpose::Installation).await.unwrap();
        assert_eq!(
            consumed,
            Some(ConsumedState {
                organization_id: organization,
                user_id: user,
                github_app_id: Some(app.id),
            })
        );
        assert_eq!(
            consume(&pool, state, Purpose::Installation).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_state_for_another_purpose_is_refused_and_kept() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;

        let issued = issue(&pool, organization, user, Purpose::Registration, None)
            .await
            .unwrap();
        let state = issued.state.expose();

        assert_eq!(
            consume(&pool, state, Purpose::Installation).await.unwrap(),
            None
        );
        let consumed = consume(&pool, state, Purpose::Registration)
            .await
            .unwrap()
            .expect("the right purpose still consumes it");
        assert_eq!(consumed.github_app_id, None);
    }

    #[tokio::test]
    async fn an_expired_state_is_refused() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;

        let issued = issue(&pool, organization, user, Purpose::Registration, None)
            .await
            .unwrap();
        let state = issued.state.expose();
        sqlx::query(
            "UPDATE github_connection_states SET expires_at = NOW() - INTERVAL '1 second' \
             WHERE nonce_hash = $1",
        )
        .bind(hash_token(state))
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            consume(&pool, state, Purpose::Registration).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn an_unknown_state_is_refused() {
        let pool = pool().await;
        assert_eq!(
            consume(&pool, "never-issued", Purpose::Registration)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn issuing_prunes_expired_states() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;

        let expired = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO github_connection_states \
               (nonce_hash, organization_id, user_id, purpose, expires_at) \
             VALUES ($1, $2, $3, 'registration', NOW() - INTERVAL '1 minute')",
        )
        .bind(hash_token(&expired))
        .bind(organization)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(stored(&pool, &expired).await, 1);

        let issued = issue(&pool, organization, user, Purpose::Registration, None)
            .await
            .unwrap();

        assert_eq!(stored(&pool, &expired).await, 0);
        assert_eq!(stored(&pool, issued.state.expose()).await, 1);
    }

    #[test]
    fn debug_never_shows_the_state() {
        let issued = Issued {
            state: SecretValue::new("plaintext-state"),
            expires_at: Utc::now(),
        };
        let rendered = format!("{issued:?}");
        assert!(!rendered.contains("plaintext-state"), "{rendered}");
    }
}
