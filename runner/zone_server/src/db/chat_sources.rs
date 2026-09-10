//! Per-chat registry of retrieved sources, addressed by a stable identifier.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{
    Decode, PgPool, Postgres, Type,
    error::BoxDynError,
    postgres::{PgTypeInfo, PgValueRef},
};
use uuid::Uuid;

use super::DbResult;
use crate::agent::identifier::{self, Kind};

/// A unique violation naming the primary key means a different URI already
/// holds the minted identifier. Every other unique violation is a real
/// conflict, and retrying one would never terminate.
const IDENTIFIER_CONSTRAINT: &str = "chat_sources_pkey";

impl Type<Postgres> for Kind {
    fn type_info() -> PgTypeInfo {
        <&str as Type<Postgres>>::type_info()
    }

    fn compatible(info: &PgTypeInfo) -> bool {
        <&str as Type<Postgres>>::compatible(info)
    }
}

impl<'r> Decode<'r, Postgres> for Kind {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let stored = <&str as Decode<'r, Postgres>>::decode(value)?;
        Self::parse(stored).ok_or_else(|| format!("Unknown source kind: {stored}").into())
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Source {
    pub chat_id: Uuid,
    pub identifier: String,
    pub kind: Kind,
    pub uri: String,
    pub title: String,
    pub first_observed_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
}

pub async fn observe(
    pool: &PgPool,
    chat: Uuid,
    kind: Kind,
    uri: &str,
    title: &str,
) -> DbResult<Source> {
    let mut identifier = identifier::mint(kind, uri);

    loop {
        match insert(pool, chat, kind, &identifier, uri, title).await {
            Ok(source) => return Ok(source),
            Err(error) if collided(&error) => {
                identifier = identifier::extend(&identifier, uri).ok_or_else(|| {
                    sqlx::Error::Protocol(format!(
                        "Exhausted identifiers for {kind} source in chat {chat}"
                    ))
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn insert(
    pool: &PgPool,
    chat: Uuid,
    kind: Kind,
    identifier: &str,
    uri: &str,
    title: &str,
) -> DbResult<Source> {
    sqlx::query_as::<_, Source>(
        r#"
        INSERT INTO chat_sources (chat_id, identifier, kind, uri, title)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (chat_id, kind, uri) DO UPDATE SET last_observed_at = clock_timestamp()
        RETURNING chat_id, identifier, kind, uri, title, first_observed_at, last_observed_at
        "#,
    )
    .bind(chat)
    .bind(identifier)
    .bind(kind.as_str())
    .bind(uri)
    .bind(title)
    .fetch_one(pool)
    .await
}

fn collided(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|error| {
        error.is_unique_violation() && error.constraint() == Some(IDENTIFIER_CONSTRAINT)
    })
}

pub async fn resolve(pool: &PgPool, chat: Uuid, identifiers: &[String]) -> DbResult<Vec<Source>> {
    if identifiers.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, Source>(
        r#"
        SELECT chat_id, identifier, kind, uri, title, first_observed_at, last_observed_at
        FROM chat_sources
        WHERE chat_id = $1 AND identifier = ANY($2)
        "#,
    )
    .bind(chat)
    .bind(identifiers)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::error::{DatabaseError, ErrorKind};
    use std::fmt;

    const URI_CONSTRAINT: &str = "chat_sources_chat_id_kind_uri_key";

    #[derive(Debug)]
    struct Violation {
        constraint: &'static str,
        unique: bool,
    }

    impl fmt::Display for Violation {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.constraint)
        }
    }

    impl std::error::Error for Violation {}

    impl DatabaseError for Violation {
        fn message(&self) -> &str {
            self.constraint
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn constraint(&self) -> Option<&str> {
            Some(self.constraint)
        }

        fn kind(&self) -> ErrorKind {
            if self.unique {
                ErrorKind::UniqueViolation
            } else {
                ErrorKind::CheckViolation
            }
        }
    }

    fn violation(constraint: &'static str, unique: bool) -> sqlx::Error {
        sqlx::Error::Database(Box::new(Violation { constraint, unique }))
    }

    #[test]
    fn only_the_primary_key_counts_as_an_identifier_collision() {
        assert!(
            collided(&violation(IDENTIFIER_CONSTRAINT, true)),
            "a unique violation on the primary key must retry with a longer identifier"
        );
        assert!(
            !collided(&violation(URI_CONSTRAINT, true)),
            "a unique violation on the chat, kind and uri must not retry, or observe would loop forever"
        );
        assert!(
            !collided(&violation(IDENTIFIER_CONSTRAINT, false)),
            "a non-unique violation must not retry"
        );
        assert!(
            !collided(&sqlx::Error::RowNotFound),
            "a non-database error must not retry"
        );
    }
}
