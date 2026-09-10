//! Per-chat registry of retrieved sources, addressed by a stable identifier.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{
    Decode, PgPool, Postgres, Type,
    error::BoxDynError,
    postgres::{PgTypeInfo, PgValueRef},
};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use super::DbResult;

/// Replaced by `use crate::agent::identifier;` when that module lands; the call
/// sites below are already written against its surface.
mod identifier {
    use sha2::{Digest, Sha256};

    const MINTED: usize = 6;
    const GROWTH: usize = 1;
    const SEPARATOR: u8 = 0x1f;

    pub fn mint(kind: &str, uri: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        hasher.update([SEPARATOR]);
        hasher.update(uri.as_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        hex::encode(digest)[..MINTED].to_owned()
    }

    pub fn extend(existing: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(existing.as_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        format!("{existing}{}", &hex::encode(digest)[..GROWTH])
    }
}

/// A unique violation naming the primary key means a different URI already
/// holds the minted identifier. Every other unique violation is a real
/// conflict, and retrying one would never terminate.
const IDENTIFIER_CONSTRAINT: &str = "chat_sources_pkey";

const ATTEMPTS: u8 = 8;

#[derive(Debug, thiserror::Error)]
#[error("Unknown source kind: {0}")]
pub struct UnknownKind(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Web,
    Doc,
    Kb,
    Chat,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Doc => "doc",
            Self::Kb => "kb",
            Self::Chat => "chat",
        }
    }
}

impl FromStr for Kind {
    type Err = UnknownKind;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "web" => Ok(Self::Web),
            "doc" => Ok(Self::Doc),
            "kb" => Ok(Self::Kb),
            "chat" => Ok(Self::Chat),
            other => Err(UnknownKind(other.to_owned())),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

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
        <&str as Decode<'r, Postgres>>::decode(value)?
            .parse()
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Source {
    pub chat_id: Uuid,
    pub identifier: String,
    pub kind: Kind,
    pub uri: String,
    pub title: Option<String>,
    pub first_observed_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
}

pub async fn observe(
    pool: &PgPool,
    chat: Uuid,
    kind: Kind,
    uri: &str,
    title: Option<&str>,
) -> DbResult<Source> {
    let mut identifier = identifier::mint(kind.as_str(), uri);

    for _ in 0..ATTEMPTS {
        match insert(pool, chat, kind, &identifier, uri, title).await {
            Ok(source) => return Ok(source),
            Err(error) if collided(&error) => identifier = identifier::extend(&identifier),
            Err(error) => return Err(error),
        }
    }

    Err(sqlx::Error::Protocol(format!(
        "Exhausted {ATTEMPTS} identifiers for {kind} source in chat {chat}"
    )))
}

async fn insert(
    pool: &PgPool,
    chat: Uuid,
    kind: Kind,
    identifier: &str,
    uri: &str,
    title: Option<&str>,
) -> DbResult<Source> {
    sqlx::query_as::<_, Source>(
        r#"
        INSERT INTO chat_sources
            (chat_id, identifier, kind, uri, title, first_observed_at, last_observed_at)
        VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
        ON CONFLICT (chat_id, kind, uri) DO UPDATE SET last_observed_at = NOW()
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

    const URI_CONSTRAINT: &str = "chat_sources_chat_id_kind_uri_key";
    const KINDS: [Kind; 4] = [Kind::Web, Kind::Doc, Kind::Kb, Kind::Chat];

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

    fn hexadecimal(value: &str) -> bool {
        value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    }

    #[test]
    fn mints_six_hexadecimal_characters() {
        for kind in KINDS {
            let minted = identifier::mint(kind.as_str(), "https://example.com/a");
            assert_eq!(minted.len(), 6, "{kind} identifier was not six characters");
            assert!(
                hexadecimal(&minted),
                "{minted} is not lowercase hexadecimal"
            );
        }
    }

    #[test]
    fn minting_is_a_pure_function_of_kind_and_uri() {
        let uri = "https://example.com/a";
        assert_eq!(
            identifier::mint(Kind::Web.as_str(), uri),
            identifier::mint(Kind::Web.as_str(), uri),
            "minting the same source twice produced different identifiers"
        );
        assert_ne!(
            identifier::mint(Kind::Web.as_str(), uri),
            identifier::mint(Kind::Doc.as_str(), uri),
            "kind is not part of the minted identifier"
        );
        assert_ne!(
            identifier::mint(Kind::Web.as_str(), uri),
            identifier::mint(Kind::Web.as_str(), "https://example.com/b"),
            "uri is not part of the minted identifier"
        );
    }

    #[test]
    fn extending_grows_one_character_and_keeps_the_prefix() {
        let mut identifier = identifier::mint(Kind::Web.as_str(), "https://example.com/a");

        for attempt in 1..=usize::from(ATTEMPTS) {
            let extended = identifier::extend(&identifier);
            assert!(
                extended.starts_with(&identifier),
                "extension {extended} dropped the prefix {identifier}"
            );
            assert_eq!(
                extended.len(),
                6 + attempt,
                "extension {extended} did not grow by exactly one character"
            );
            assert!(hexadecimal(&extended), "{extended} is not hexadecimal");
            identifier = extended;
        }
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

    #[test]
    fn kinds_round_trip_through_their_stored_representation() {
        for kind in KINDS {
            assert_eq!(
                kind.as_str().parse::<Kind>().expect("kind should parse"),
                kind,
                "{kind} did not survive a round trip"
            );
            assert_eq!(kind.to_string(), kind.as_str());
        }

        assert!(
            "video".parse::<Kind>().is_err(),
            "a kind outside the check constraint must not parse"
        );
    }
}
