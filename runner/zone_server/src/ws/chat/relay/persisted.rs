use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::db::context::{COMPLETED, INTERRUPTED};

use super::super::ServerMessage;

/// Told to the readers of a turn whose replica stopped without ending it.
pub const STOPPED: &str =
    "The server running this reply stopped; any queued messages were dropped. Send again.";

/// The turn a lease was taken for, as it was saved, and its answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Persisted {
    id: Uuid,
    status: String,
    content: Option<String>,
    metadata: Option<Value>,
}

impl Persisted {
    pub async fn at(pool: &PgPool, chat: Uuid, fence: i64) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT t.id, t.status, m.content, m.metadata FROM chat_turns t
             LEFT JOIN messages m ON m.id = t.id AND m.chat_id = t.chat_id
             WHERE t.chat_id = $1 AND t.fence = $2",
        )
        .bind(chat)
        .bind(fence)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|row| Self {
            id: row.get("id"),
            status: row.get("status"),
            content: row.get("content"),
            metadata: row.get("metadata"),
        }))
    }

    /// The frame that ends the turn: what a reload would show for one that
    /// finished, and the error for one nobody finished.
    pub fn closing(turn: Option<Self>) -> ServerMessage {
        match turn {
            Some(Self {
                id,
                status,
                content: Some(content),
                metadata,
            }) if status == COMPLETED => ServerMessage::MessageEnd {
                message_id: id,
                content,
                metadata: metadata.filter(|metadata| !metadata.is_null()),
                error: None,
            },
            Some(Self { id, status, .. }) if status == INTERRUPTED => ServerMessage::Cancelled {
                message_id: Some(id),
            },
            _ => ServerMessage::Error {
                message: STOPPED.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(status: &str, content: Option<&str>) -> Persisted {
        Persisted {
            id: Uuid::new_v4(),
            status: status.to_string(),
            content: content.map(str::to_string),
            metadata: Some(serde_json::json!({"usage": {"total": 3}})),
        }
    }

    #[test]
    fn a_finished_turn_closes_with_its_saved_answer_and_no_error() {
        let finished = turn(COMPLETED, Some("Saved answer"));
        let id = finished.id;

        match Persisted::closing(Some(finished)) {
            ServerMessage::MessageEnd {
                message_id,
                content,
                metadata,
                error,
            } => {
                assert_eq!(message_id, id);
                assert_eq!(content, "Saved answer");
                assert_eq!(metadata, Some(serde_json::json!({"usage": {"total": 3}})));
                assert_eq!(error, None);
            }
            other => panic!("expected the saved answer, got {other:?}"),
        }
    }

    #[test]
    fn an_interrupted_turn_closes_as_cancelled() {
        let interrupted = turn(INTERRUPTED, Some("Half"));
        let id = interrupted.id;

        assert!(matches!(
            Persisted::closing(Some(interrupted)),
            ServerMessage::Cancelled { message_id: Some(message_id) } if message_id == id
        ));
    }

    #[test]
    fn a_turn_still_running_or_never_saved_closes_with_the_stopped_error() {
        for turn in [Some(turn(crate::db::context::RUNNING, Some("Half"))), None] {
            assert!(matches!(
                Persisted::closing(turn),
                ServerMessage::Error { message } if message == STOPPED
            ));
        }
    }
}
