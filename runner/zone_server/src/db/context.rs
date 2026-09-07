//! Fenced, transactional conversation persistence. No transaction spans model inference.

use chrono::NaiveDateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;
use zone_core::llm::{Message, Role};

use zone_chat::history::{self, Entry, Evidence, History, NewEntry, ReplayMessage, Summary};
use zone_chat::store;

pub use zone_chat::store::{Guard, Lease, StoredMessage};

/// Postgres failures the conversation store can hit.
///
/// Richer than [`store::Error`] on purpose: the queries below keep `?` and the
/// sqlx detail, and the [`store::ContextStore`] implementation flattens it at
/// the boundary so callers of the trait never see a database type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("This chat already has an active response")]
    Busy,
    #[error("Chat generation ownership expired or changed; the response was stopped")]
    LeaseLost,
    #[error("Conversation checkpoint changed; retry from current history")]
    Conflict,
    #[error("Conversation integrity error: {0}")]
    Integrity(String),
    #[error("Conversation evidence was not found in this chat")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<Error> for store::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Busy => store::Error::Busy,
            Error::LeaseLost => store::Error::LeaseLost,
            Error::Conflict => store::Error::Conflict,
            Error::Integrity(detail) => store::Error::Integrity(detail),
            Error::NotFound => store::Error::NotFound,
            Error::Database(error) => store::Error::Backend(error.to_string()),
            Error::Json(error) => store::Error::Backend(error.to_string()),
        }
    }
}

const PAGE_CHARS: u64 = 8_000;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
    chat_id: Uuid,
    workspace_id: Option<Uuid>,
}

impl Store {
    /// The scope must come from an authorized chat, never from model tool arguments.
    pub fn new(pool: PgPool, chat_id: Uuid, workspace_id: Option<Uuid>) -> Self {
        Self {
            pool,
            chat_id,
            workspace_id,
        }
    }

    pub async fn acquire(&self, owner: Uuid, lifetime: Duration) -> Result<Lease, Error> {
        let milliseconds = milliseconds(lifetime)?;
        let row = sqlx::query("INSERT INTO chat_leases (chat_id, owner, fence, expires_at)
            SELECT id, $3, 1, clock_timestamp() + $4 * interval '1 millisecond'
            FROM chats WHERE id = $1 AND workspace_id IS NOT DISTINCT FROM $2
            ON CONFLICT (chat_id) DO UPDATE SET owner = EXCLUDED.owner,
            fence = chat_leases.fence + 1, expires_at = clock_timestamp() + $4 * interval '1 millisecond'
            WHERE chat_leases.expires_at <= clock_timestamp()
            RETURNING owner, fence, expires_at")
            .bind(self.chat_id).bind(self.workspace_id).bind(owner).bind(milliseconds)
            .fetch_optional(&self.pool).await?;
        match row {
            Some(row) => Ok(Lease {
                chat_id: self.chat_id,
                owner: row.get("owner"),
                fence: row.get("fence"),
                expires_at: row.get("expires_at"),
            }),
            None => {
                self.scope(&mut *self.pool.acquire().await?).await?;
                Err(Error::Busy)
            }
        }
    }

    pub async fn renew(&self, lease: &Lease, lifetime: Duration) -> Result<Lease, Error> {
        self.identity(lease)?;
        let row = sqlx::query("UPDATE chat_leases l SET expires_at = clock_timestamp() + $5 * interval '1 millisecond'
            FROM chats c WHERE l.chat_id = $1 AND l.owner = $2 AND l.fence = $3
            AND l.expires_at > clock_timestamp() AND c.id = l.chat_id AND c.workspace_id IS NOT DISTINCT FROM $4
            RETURNING l.expires_at")
            .bind(self.chat_id).bind(lease.owner).bind(lease.fence).bind(self.workspace_id).bind(milliseconds(lifetime)?)
            .fetch_optional(&self.pool).await?.ok_or(Error::LeaseLost)?;
        Ok(Lease {
            expires_at: row.get("expires_at"),
            ..lease.clone()
        })
    }

    pub fn keep_alive(&self, lease: Lease, lifetime: Duration) -> Result<Guard, Error> {
        store::keep_alive(std::sync::Arc::new(self.clone()), lease, lifetime)
            .map_err(|error| Error::Integrity(error.to_string()))
    }

    pub async fn assert_current(&self, lease: &Lease) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn release(&self, lease: &Lease) -> Result<bool, Error> {
        self.identity(lease)?;
        // Retain the row so the fencing token is never reset by release/reacquire.
        Ok(sqlx::query(
            "UPDATE chat_leases l SET expires_at = clock_timestamp() FROM chats c
            WHERE l.chat_id = $1 AND l.owner = $2 AND l.fence = $3
            AND c.id = l.chat_id AND c.workspace_id IS NOT DISTINCT FROM $4",
        )
        .bind(self.chat_id)
        .bind(lease.owner)
        .bind(lease.fence)
        .bind(self.workspace_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }

    /// Save the trigger and parent turn atomically, before any external operation starts.
    pub async fn begin(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        user_message_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        message: ReplayMessage,
    ) -> Result<StoredMessage, Error> {
        if message.role != Role::User
            || message.content.as_deref() != Some(content)
            || message.tool_calls.is_some()
            || message.tool_call_id.is_some()
        {
            return Err(Error::Integrity("Invalid triggering user message".into()));
        }
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.materialize(&mut transaction).await?;
        self.recover_in(&mut transaction).await?;
        sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
            .bind(self.chat_id)
            .execute(&mut *transaction)
            .await?;
        let title_claimed = sqlx::query("UPDATE chats SET title_message_id = $2 WHERE id = $1 AND automatic_title
            AND title_message_id IS NULL AND NOT EXISTS (SELECT 1 FROM messages WHERE chat_id = $1 AND role = 'user')")
            .bind(self.chat_id).bind(user_message_id).execute(&mut *transaction).await?.rows_affected() == 1;
        let row = self
            .visible(
                &mut transaction,
                user_message_id,
                "user",
                content,
                metadata,
                title_claimed,
            )
            .await?;
        sqlx::query(
            "INSERT INTO chat_turns (id, chat_id, user_message_id, fence) VALUES ($1,$2,$3,$4)",
        )
        .bind(turn_id)
        .bind(self.chat_id)
        .bind(user_message_id)
        .bind(lease.fence)
        .execute(&mut *transaction)
        .await?;
        self.insert(
            &mut transaction,
            Some(turn_id),
            &user_message_id.to_string(),
            &message,
            true,
            false,
        )
        .await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(row)
    }

    pub async fn append(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        entries: &[NewEntry],
    ) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.turn(&mut transaction, lease, turn_id).await?;
        for entry in entries {
            self.append_in(&mut transaction, turn_id, entry).await?;
        }
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn create_message(
        &self,
        lease: &Lease,
        role: &str,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error> {
        let mut message = match role {
            "user" => Message::user(content),
            "assistant" => Message::assistant(content),
            "system" => Message::system(content),
            _ => return Err(Error::Integrity("Invalid visible message role".into())),
        };
        message.images = crate::services::chat::session::images(metadata.as_ref());
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.materialize(&mut transaction).await?;
        self.recover_in(&mut transaction).await?;
        let id = Uuid::new_v4();
        let title_claimed = if role == "user" {
            sqlx::query("UPDATE chats SET title_message_id=$2 WHERE id=$1 AND automatic_title AND title_message_id IS NULL AND NOT EXISTS(SELECT 1 FROM messages WHERE chat_id=$1 AND role='user')").bind(self.chat_id).bind(id).execute(&mut *transaction).await?.rows_affected()==1
        } else {
            false
        };
        let row = self
            .visible(&mut transaction, id, role, content, metadata, title_claimed)
            .await?;
        self.insert(
            &mut transaction,
            None,
            &id.to_string(),
            &ReplayMessage::from(&message),
            true,
            false,
        )
        .await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(row)
    }

    /// Explicit deletion removes private evidence with its visible owner and invalidates
    /// checkpoints. The remaining visible companion is retained at its original position.
    pub async fn delete_message(&self, lease: &Lease, id: Uuid) -> Result<bool, Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.materialize(&mut transaction).await?;
        self.recover_in(&mut transaction).await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE chat_id=$1 AND id=$2)")
                .bind(self.chat_id)
                .bind(id)
                .fetch_one(&mut *transaction)
                .await?;
        if !exists {
            return Ok(false);
        }
        let turn=sqlx::query("SELECT id,user_message_id FROM chat_turns WHERE chat_id=$1 AND (id=$2 OR user_message_id=$2)").bind(self.chat_id).bind(id).fetch_optional(&mut *transaction).await?;
        if let Some(turn) = turn {
            let owner: Uuid = turn.get("id");
            let user: Uuid = turn.get("user_message_id");
            let companion = if id == user {
                sqlx::query("SELECT m.content,m.metadata,(SELECT max(position) FROM chat_entries WHERE chat_id=$1 AND turn_id=$2) AS position FROM messages m WHERE m.chat_id=$1 AND m.id=$2").bind(self.chat_id).bind(owner).fetch_optional(&mut *transaction).await?
            } else {
                None
            };
            if id == owner {
                sqlx::query("UPDATE chat_entries SET turn_id=NULL WHERE chat_id=$1 AND id=$2")
                    .bind(self.chat_id)
                    .bind(user.to_string())
                    .execute(&mut *transaction)
                    .await?;
            }
            sqlx::query("DELETE FROM chat_turns WHERE chat_id=$1 AND id=$2")
                .bind(self.chat_id)
                .bind(owner)
                .execute(&mut *transaction)
                .await?;
            if let Some(companion) = companion {
                let mut message = Message::assistant(companion.get::<String, _>("content"));
                message.images = crate::services::chat::session::images(
                    companion.get::<Option<Value>, _>("metadata").as_ref(),
                );
                let position: Option<i64> = companion.get("position");
                if let Some(position) = position {
                    sqlx::query("INSERT INTO chat_entries(chat_id,id,position,message,consumed) OVERRIDING SYSTEM VALUE VALUES($1,$2,$3,$4,TRUE)").bind(self.chat_id).bind(owner.to_string()).bind(position).bind(serde_json::to_value(ReplayMessage::from(&message))?).execute(&mut *transaction).await?;
                }
            }
        }
        sqlx::query("DELETE FROM chat_checkpoints WHERE chat_id=$1")
            .bind(self.chat_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM chat_entries WHERE chat_id=$1 AND id=$2")
            .bind(self.chat_id)
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM messages WHERE chat_id=$1 AND id=$2")
            .bind(self.chat_id)
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn consumed(&self, lease: &Lease, ids: &[String]) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        let unique: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let count = sqlx::query(
            "UPDATE chat_entries SET consumed = TRUE WHERE chat_id = $1 AND id = ANY($2)",
        )
        .bind(self.chat_id)
        .bind(ids)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if count as usize != unique.len() {
            return Err(Error::Integrity(
                "Cannot mark missing evidence consumed".into(),
            ));
        }
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.turn(&mut transaction, lease, turn_id).await?;
        let pending: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chat_calls WHERE chat_id = $1 AND turn_id = $2 AND result_id IS NULL)")
            .bind(self.chat_id).bind(turn_id).fetch_one(&mut *transaction).await?;
        if pending {
            return Err(Error::Integrity(
                "Cannot complete a turn with pending tool outcomes".into(),
            ));
        }
        let row = self
            .visible(
                &mut transaction,
                turn_id,
                "assistant",
                content,
                metadata,
                false,
            )
            .await?;
        sqlx::query("UPDATE chat_turns SET status = 'completed', completed_at = clock_timestamp() WHERE chat_id = $1 AND id = $2")
            .bind(self.chat_id).bind(turn_id).execute(&mut *transaction).await?;
        self.consume_turn_in(&mut transaction, turn_id, &[]).await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(row)
    }

    /// Write the visible assistant row for a running turn so a refresh sees
    /// streamed chunks before the turn finishes.
    pub async fn publish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.turn(&mut transaction, lease, turn_id).await?;
        let row = self
            .visible(
                &mut transaction,
                turn_id,
                "assistant",
                content,
                metadata,
                false,
            )
            .await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(row)
    }

    /// Persist partial visible prose and uncertain outcomes in one fenced transaction.
    pub async fn finish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        interrupted: bool,
        partial: Option<&ReplayMessage>,
    ) -> Result<StoredMessage, Error> {
        if !interrupted && partial.is_none() {
            return self.complete(lease, turn_id, content, metadata).await;
        }
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.turn(&mut transaction, lease, turn_id).await?;
        let recovery = if interrupted {
            self.interrupt_in(&mut transaction, turn_id).await?
        } else {
            let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_calls WHERE chat_id=$1 AND turn_id=$2 AND result_id IS NULL)").bind(self.chat_id).bind(turn_id).fetch_one(&mut *transaction).await?;
            if pending {
                return Err(Error::Integrity(
                    "Cannot complete with pending tool outcomes".into(),
                ));
            }
            sqlx::query("UPDATE chat_turns SET status='completed',completed_at=clock_timestamp() WHERE chat_id=$1 AND id=$2").bind(self.chat_id).bind(turn_id).execute(&mut *transaction).await?;
            Vec::new()
        };
        if let Some(partial) = partial {
            self.append_in(
                &mut transaction,
                turn_id,
                &NewEntry {
                    id: Uuid::new_v4().to_string(),
                    message: partial.clone(),
                    mutations: Vec::new(),
                },
            )
            .await?;
        }
        let row = self
            .visible(
                &mut transaction,
                turn_id,
                "assistant",
                content,
                metadata,
                false,
            )
            .await?;
        self.consume_turn_in(&mut transaction, turn_id, &recovery)
            .await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(row)
    }

    pub async fn interrupt(&self, lease: &Lease, turn_id: Uuid) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.turn(&mut transaction, lease, turn_id).await?;
        self.interrupt_in(&mut transaction, turn_id).await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn recover(&self, lease: &Lease) -> Result<usize, Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        let count = self.recover_in(&mut transaction).await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(count)
    }

    pub async fn load(&self) -> Result<History, Error> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await?;
        self.scope(&mut transaction).await?;
        let history = self.load_in(&mut transaction).await?;
        if let Some(summary) = &history.summary {
            history::validate(&history, summary).map_err(Error::Integrity)?;
        }
        transaction.commit().await?;
        Ok(history)
    }

    pub async fn checkpoint(
        &self,
        lease: &Lease,
        expected: Option<&Summary>,
        proposed: &Summary,
    ) -> Result<(), Error> {
        let mut transaction = self.pool.begin().await?;
        self.lock(&mut transaction, lease).await?;
        self.materialize(&mut transaction).await?;
        let history = self.load_in(&mut transaction).await?;
        if history.summary.as_ref() != expected {
            return Err(Error::Conflict);
        }
        history::validate(&history, proposed).map_err(Error::Integrity)?;
        let previous = expected.map_or(0, |summary| summary.revision);
        if proposed.revision
            != previous
                .checked_add(1)
                .ok_or_else(|| Error::Integrity("Checkpoint revision overflow".into()))?
        {
            return Err(Error::Conflict);
        }
        if let Some(expected) = expected {
            let selected: HashSet<&str> = proposed.entries.iter().map(String::as_str).collect();
            if expected
                .entries
                .iter()
                .any(|id| !selected.contains(id.as_str()))
                || expected.entries == proposed.entries
            {
                return Err(Error::Integrity(
                    "Checkpoint must retain prior coverage and add new evidence".into(),
                ));
            }
        }
        let revision = i64::try_from(proposed.revision)
            .map_err(|_| Error::Integrity("Checkpoint revision overflow".into()))?;
        sqlx::query("INSERT INTO chat_checkpoints (chat_id, revision, content, entries, fingerprint) VALUES ($1,$2,$3,$4,$5)
            ON CONFLICT (chat_id) DO UPDATE SET revision = EXCLUDED.revision, content = EXCLUDED.content,
            entries = EXCLUDED.entries, fingerprint = EXCLUDED.fingerprint, updated_at = clock_timestamp()")
            .bind(self.chat_id).bind(revision).bind(&proposed.content).bind(serde_json::to_value(&proposed.entries)?).bind(&proposed.fingerprint)
            .execute(&mut *transaction).await?;
        self.lock(&mut transaction, lease).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Unicode scalar offsets, with bounds derived from actual persisted content length.
    pub async fn evidence(&self, id: &str, offset: u64, limit: u64) -> Result<Evidence, Error> {
        if let Some(cursor) = id.strip_prefix("catalog:") {
            let (position, fingerprint) = cursor.split_once(':').ok_or(Error::NotFound)?;
            let position = position
                .parse::<i64>()
                .ok()
                .filter(|value| *value >= 0)
                .ok_or(Error::NotFound)?;
            return self
                .catalog_at(Some((position, fingerprint)), offset, limit)
                .await;
        }
        if self.workspace_id.is_none() {
            return Err(Error::NotFound);
        }
        let row = sqlx::query("SELECT e.message FROM chat_entries e JOIN chats c ON c.id = e.chat_id
            WHERE e.chat_id = $1 AND e.id = $2 AND c.workspace_id IS NOT DISTINCT FROM $3 AND e.message->>'role' = 'tool'")
            .bind(self.chat_id).bind(id).bind(self.workspace_id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound)?;
        let message: ReplayMessage = serde_json::from_value(row.get("message"))?;
        let content = message.content.unwrap_or_default();
        Self::page(id, &content, offset, limit)
    }

    /// Open a bounded catalog snapshot. Continuations use its returned id, so evidence
    /// reader results cannot make their own pagination grow indefinitely.
    pub async fn catalog(&self, offset: u64, limit: u64) -> Result<Evidence, Error> {
        self.catalog_at(None, offset, limit).await
    }

    async fn catalog_at(
        &self,
        cursor: Option<(i64, &str)>,
        offset: u64,
        limit: u64,
    ) -> Result<Evidence, Error> {
        if self.workspace_id.is_none() {
            return Err(Error::NotFound);
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE id=$1 AND workspace_id=$2)",
        )
        .bind(self.chat_id)
        .bind(self.workspace_id)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Err(Error::NotFound);
        }
        let position=match cursor {
            Some((position,_))=>position,
            None=>sqlx::query_scalar::<_,i64>("SELECT COALESCE(max(e.position),0) FROM chat_entries e JOIN chats c ON c.id=e.chat_id WHERE e.chat_id=$1 AND c.workspace_id=$2").bind(self.chat_id).bind(self.workspace_id).fetch_one(&self.pool).await?,
        };
        let rows=sqlx::query("SELECT e.id, call.value->'function'->>'name' AS name,
            CASE WHEN e.message->>'content' LIKE 'Error: execution was interrupted. Outcome unknown:%' THEN 'unknown'
                 WHEN e.message->>'content' LIKE 'Error:%' THEN 'error' ELSE 'recorded' END AS outcome
            FROM chat_entries e JOIN chats c ON c.id=e.chat_id
            JOIN chat_calls owner ON owner.chat_id=e.chat_id AND owner.result_id=e.id
            JOIN chat_entries envelope ON envelope.chat_id=owner.chat_id AND envelope.id=owner.envelope_id
            CROSS JOIN LATERAL jsonb_array_elements(CASE WHEN jsonb_typeof(envelope.message->'tool_calls')='array' THEN envelope.message->'tool_calls' ELSE '[]'::jsonb END) call(value)
            WHERE e.chat_id=$1 AND c.workspace_id=$2 AND e.position<=$3 AND call.value->>'id'=owner.id
            ORDER BY e.position")
            .bind(self.chat_id).bind(self.workspace_id).bind(position).fetch_all(&self.pool).await?;
        let mut content = String::new();
        for row in rows {
            let item = serde_json::json!({"id":row.get::<String,_>("id"),"name":row.get::<Option<String>,_>("name"),"outcome":row.get::<String,_>("outcome")});
            content.push_str(&serde_json::to_string(&item)?);
            content.push('\n');
        }
        let fingerprint = hex::encode(Sha256::digest(content.as_bytes()));
        if cursor.is_some_and(|(_, expected)| expected != fingerprint) {
            return Err(Error::Integrity(
                "Evidence catalog changed after a deletion; restart with id omitted".into(),
            ));
        }
        Self::page(
            &format!("catalog:{position}:{fingerprint}"),
            &content,
            offset,
            limit,
        )
    }

    fn page(id: &str, content: &str, offset: u64, limit: u64) -> Result<Evidence, Error> {
        let total = content.chars().count() as u64;
        if limit == 0 || offset > total {
            return Err(Error::Integrity(
                "Evidence page offset or length is invalid".into(),
            ));
        }
        let count = limit.min(PAGE_CHARS).min(total - offset);
        let page: String = content
            .chars()
            .skip(offset as usize)
            .take(count as usize)
            .collect();
        let end = offset + count;
        Ok(Evidence {
            id: id.into(),
            content: page,
            offset,
            next: (end < total).then_some(end),
            total,
        })
    }

    fn identity(&self, lease: &Lease) -> Result<(), Error> {
        if self.chat_id != lease.chat_id {
            Err(Error::LeaseLost)
        } else {
            Ok(())
        }
    }

    async fn scope(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chats WHERE id = $1 AND workspace_id IS NOT DISTINCT FROM $2)")
            .bind(self.chat_id).bind(self.workspace_id).fetch_one(connection).await?;
        if exists { Ok(()) } else { Err(Error::NotFound) }
    }

    async fn lock(&self, connection: &mut PgConnection, lease: &Lease) -> Result<(), Error> {
        self.identity(lease)?;
        let current = sqlx::query("SELECT l.fence FROM chat_leases l JOIN chats c ON c.id = l.chat_id
            WHERE l.chat_id = $1 AND l.owner = $2 AND l.fence = $3 AND l.expires_at > clock_timestamp()
            AND c.workspace_id IS NOT DISTINCT FROM $4 FOR UPDATE OF l")
            .bind(self.chat_id).bind(lease.owner).bind(lease.fence).bind(self.workspace_id).fetch_optional(connection).await?;
        current.ok_or(Error::LeaseLost)?;
        Ok(())
    }

    async fn turn(
        &self,
        connection: &mut PgConnection,
        lease: &Lease,
        turn_id: Uuid,
    ) -> Result<(), Error> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chat_turns WHERE chat_id = $1 AND id = $2 AND fence = $3 AND status = 'running')")
            .bind(self.chat_id).bind(turn_id).bind(lease.fence).fetch_one(connection).await?;
        if exists {
            Ok(())
        } else {
            Err(Error::LeaseLost)
        }
    }

    async fn insert(
        &self,
        connection: &mut PgConnection,
        turn: Option<Uuid>,
        id: &str,
        message: &ReplayMessage,
        consumed: bool,
        legacy: bool,
    ) -> Result<(), Error> {
        if id.is_empty() || message.version != history::VERSION {
            return Err(Error::Integrity(
                "Invalid canonical entry version or identity".into(),
            ));
        }
        sqlx::query("INSERT INTO chat_entries (chat_id,id,turn_id,message,consumed,legacy) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(self.chat_id).bind(id).bind(turn).bind(serde_json::to_value(message)?).bind(consumed).bind(legacy).execute(connection).await?;
        Ok(())
    }

    async fn append_in(
        &self,
        connection: &mut PgConnection,
        turn_id: Uuid,
        entry: &NewEntry,
    ) -> Result<(), Error> {
        let message = &entry.message;
        if !matches!(message.role, Role::Assistant | Role::Tool) {
            return Err(Error::Integrity(
                "Only assistant envelopes and tool outcomes can be appended to a turn".into(),
            ));
        }
        let mutations: HashSet<&str> = entry.mutations.iter().map(String::as_str).collect();
        let calls: HashSet<&str> = message
            .tool_calls
            .iter()
            .flatten()
            .map(|call| call.id.as_str())
            .collect();
        if !mutations.is_subset(&calls)
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.is_empty())
        {
            return Err(Error::Integrity(
                "Invalid tool envelope mutation identities".into(),
            ));
        }
        if (message.role == Role::Tool
            && (message.tool_calls.is_some() || message.tool_call_id.is_none()))
            || (message.role == Role::Assistant && message.tool_call_id.is_some())
        {
            return Err(Error::Integrity(
                "Invalid tool envelope or result role".into(),
            ));
        }
        self.insert(connection, Some(turn_id), &entry.id, message, false, false)
            .await?;
        for call in message.tool_calls.iter().flatten() {
            if call.id.is_empty() {
                return Err(Error::Integrity("Tool call has no identity".into()));
            }
            sqlx::query("INSERT INTO chat_calls (chat_id,id,turn_id,envelope_id,mutating) VALUES ($1,$2,$3,$4,$5)")
                .bind(self.chat_id).bind(&call.id).bind(turn_id).bind(&entry.id).bind(mutations.contains(call.id.as_str())).execute(&mut *connection).await?;
        }
        if let Some(id) = &message.tool_call_id {
            let updated = sqlx::query("UPDATE chat_calls SET result_id = $4 WHERE chat_id = $1 AND id = $2 AND turn_id = $3 AND result_id IS NULL")
                .bind(self.chat_id).bind(id).bind(turn_id).bind(&entry.id).execute(connection).await?.rows_affected();
            if updated != 1 {
                return Err(Error::Integrity(
                    "Tool result is orphaned, duplicated or belongs to another turn".into(),
                ));
            }
        }
        Ok(())
    }

    async fn recover_in(&self, connection: &mut PgConnection) -> Result<usize, Error> {
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM chat_turns WHERE chat_id = $1 AND status = 'running' ORDER BY created_at, id")
            .bind(self.chat_id).fetch_all(&mut *connection).await?;
        for id in &ids {
            self.interrupt_in(connection, *id).await?;
        }
        Ok(ids.len())
    }

    /// Returns the recovery entries it appended, which the model has not seen yet.
    async fn interrupt_in(
        &self,
        connection: &mut PgConnection,
        turn_id: Uuid,
    ) -> Result<Vec<String>, Error> {
        let pending = sqlx::query("SELECT id, mutating FROM chat_calls WHERE chat_id = $1 AND turn_id = $2 AND result_id IS NULL ORDER BY envelope_id, id")
            .bind(self.chat_id).bind(turn_id).fetch_all(&mut *connection).await?;
        let mut recovery: Vec<String> = Vec::with_capacity(pending.len());
        for row in pending {
            let call: String = row.get("id");
            let content = if row.get::<bool, _>("mutating") {
                "Error: execution was interrupted. Outcome unknown: this operation may have changed external state. Inspect the resulting state before taking further action; do not automatically repeat this mutation."
            } else {
                "Error: execution was interrupted before a result was durably recorded. No result is available."
            };
            let entry = NewEntry {
                id: Uuid::new_v4().to_string(),
                message: ReplayMessage::from(&Message::tool_result(call, content)),
                mutations: Vec::new(),
            };
            self.append_in(connection, turn_id, &entry).await?;
            recovery.push(entry.id);
        }
        sqlx::query("UPDATE chat_turns SET status = 'interrupted', completed_at = clock_timestamp() WHERE chat_id = $1 AND id = $2")
            .bind(self.chat_id).bind(turn_id).execute(&mut *connection).await?;
        self.consume_turn_in(connection, turn_id, &recovery).await?;
        Ok(recovery)
    }

    /// Retained entries stay unconsumed so the next run replays them verbatim and
    /// compaction cannot summarize them away before the model has read them.
    async fn consume_turn_in(
        &self,
        connection: &mut PgConnection,
        turn_id: Uuid,
        retain: &[String],
    ) -> Result<(), Error> {
        sqlx::query(
            "UPDATE chat_entries SET consumed = TRUE WHERE chat_id = $1 AND turn_id = $2 AND consumed = FALSE AND id <> ALL($3)",
        )
        .bind(self.chat_id)
        .bind(turn_id)
        .bind(retain)
        .execute(connection)
        .await?;
        Ok(())
    }

    async fn visible(
        &self,
        connection: &mut PgConnection,
        id: Uuid,
        role: &str,
        content: &str,
        metadata: Option<Value>,
        title_claimed: bool,
    ) -> Result<StoredMessage, Error> {
        let created_at: Option<NaiveDateTime> = sqlx::query_scalar(
            "INSERT INTO messages (id, chat_id, role, content, metadata) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (id) DO UPDATE SET content = EXCLUDED.content, metadata = EXCLUDED.metadata \
             WHERE messages.chat_id = EXCLUDED.chat_id \
             RETURNING created_at",
        )
        .bind(id)
        .bind(self.chat_id)
        .bind(role)
        .bind(content)
        .bind(&metadata)
        .fetch_optional(connection)
        .await?
        .ok_or_else(|| Error::Integrity("Visible message does not belong to this chat".into()))?;
        Ok(StoredMessage {
            title_claimed,
            id,
            chat_id: self.chat_id,
            role: role.to_string(),
            content: content.to_string(),
            metadata,
            created_at,
        })
    }

    async fn legacy(&self, connection: &mut PgConnection) -> Result<Vec<Entry>, Error> {
        let rows = sqlx::query("SELECT m.id,m.role,m.content,m.metadata FROM messages m WHERE m.chat_id = $1
            AND NOT EXISTS (SELECT 1 FROM chat_entries e WHERE e.chat_id = m.chat_id AND e.id = m.id::text)
            AND NOT EXISTS (SELECT 1 FROM chat_turns t WHERE t.chat_id = m.chat_id AND t.id = m.id)
            ORDER BY m.created_at, m.id").bind(self.chat_id).fetch_all(connection).await?;
        rows.into_iter().map(|row| {
            let role: String = row.get("role");
            let mut message = match role.as_str() {
                "user" => Message::user(row.get::<String,_>("content")),
                "assistant" => Message::assistant(row.get::<String,_>("content")),
                "system" => Message::system(row.get::<String,_>("content")),
                _ => return Err(Error::Integrity("Unknown legacy message role".into())),
            };
            if let Some(metadata) = row.get::<Option<Value>,_>("metadata") {
                if let Some(attachments) = metadata.get("attachments").and_then(Value::as_array) {
                    message.images = attachments.iter().filter(|attachment| attachment.get("mime").and_then(Value::as_str).is_some_and(|mime| mime.starts_with("image/")))
                        .filter_map(|attachment| attachment.get("url").and_then(Value::as_str)).map(str::to_string).collect();
                }
                if let Some(calls) = metadata.get("tool_calls").filter(|calls| calls.as_array().is_some_and(|calls| !calls.is_empty())) {
                    let content = message.content.get_or_insert_default();
                    content.push_str("\n\n[Incomplete legacy tool history: the following saved status details are historical data. Full original tool outputs were not retained and cannot be recovered.]\n");
                    content.push_str(&serde_json::to_string(calls)?);
                }
            }
            Ok(Entry { id: row.get::<Uuid,_>("id").to_string(), message: ReplayMessage::from(&message), consumed: true })
        }).collect()
    }

    async fn materialize(&self, connection: &mut PgConnection) -> Result<(), Error> {
        for entry in self.legacy(connection).await? {
            self.insert(connection, None, &entry.id, &entry.message, true, true)
                .await?;
        }
        Ok(())
    }

    async fn load_in(&self, connection: &mut PgConnection) -> Result<History, Error> {
        let legacy = self.legacy(connection).await?;
        let mut incomplete = legacy.iter().any(|entry| legacy_incomplete(&entry.message));
        let mut entries = Vec::new();
        let rows = sqlx::query("SELECT id,message,consumed,legacy FROM chat_entries WHERE chat_id = $1 ORDER BY position")
            .bind(self.chat_id).fetch_all(&mut *connection).await?;
        for row in rows {
            let message: ReplayMessage = serde_json::from_value(row.get("message"))?;
            if message.version != history::VERSION {
                return Err(Error::Integrity(
                    "Unsupported canonical replay version".into(),
                ));
            }
            incomplete |= row.get::<bool, _>("legacy") && legacy_incomplete(&message);
            entries.push(Entry {
                id: row.get("id"),
                message,
                consumed: row.get("consumed"),
            });
        }
        // begin/checkpoint materialize all preceding public rows before appending.
        // Any remaining public-only rows were added after the last canonical event.
        entries.extend(legacy);
        let latest_user = entries
            .iter()
            .rev()
            .find(|entry| entry.message.role == Role::User)
            .map(|entry| entry.id.clone());
        let row = sqlx::query(
            "SELECT revision,content,entries,fingerprint FROM chat_checkpoints WHERE chat_id = $1",
        )
        .bind(self.chat_id)
        .fetch_optional(connection)
        .await?;
        let summary = row
            .map(|row| {
                Ok::<_, Error>(Summary {
                    content: row.get("content"),
                    entries: serde_json::from_value(row.get("entries"))?,
                    fingerprint: row.get("fingerprint"),
                    revision: u64::try_from(row.get::<i64, _>("revision"))
                        .map_err(|_| Error::Integrity("Invalid checkpoint revision".into()))?,
                })
            })
            .transpose()?;
        Ok(History {
            entries,
            summary,
            latest_user,
            incomplete,
        })
    }
}

fn milliseconds(lifetime: Duration) -> Result<i64, Error> {
    let value = i64::try_from(lifetime.as_millis())
        .map_err(|_| Error::Integrity("Lease lifetime exceeds database duration range".into()))?;
    if value < 3 {
        return Err(Error::Integrity(
            "Lease lifetime requires at least three millisecond scheduling intervals".into(),
        ));
    }
    Ok(value)
}

fn legacy_incomplete(message: &ReplayMessage) -> bool {
    message
        .content
        .as_deref()
        .is_some_and(|content| content.contains("[Incomplete legacy tool history:"))
}

/// Postgres behind the storage-agnostic conversation interface. Every method
/// is the inherent one with its database error flattened at the boundary.
#[async_trait::async_trait]
impl store::ContextStore for Store {
    async fn acquire(&self, owner: Uuid, lifetime: Duration) -> Result<Lease, store::Error> {
        Store::acquire(self, owner, lifetime)
            .await
            .map_err(Into::into)
    }

    async fn renew(&self, lease: &Lease, lifetime: Duration) -> Result<Lease, store::Error> {
        Store::renew(self, lease, lifetime)
            .await
            .map_err(Into::into)
    }

    async fn assert_current(&self, lease: &Lease) -> Result<(), store::Error> {
        Store::assert_current(self, lease).await.map_err(Into::into)
    }

    async fn release(&self, lease: &Lease) -> Result<bool, store::Error> {
        Store::release(self, lease).await.map_err(Into::into)
    }

    async fn begin(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        user_message_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        message: ReplayMessage,
    ) -> Result<StoredMessage, store::Error> {
        Store::begin(
            self,
            lease,
            turn_id,
            user_message_id,
            content,
            metadata,
            message,
        )
        .await
        .map_err(Into::into)
    }

    async fn append(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        entries: &[NewEntry],
    ) -> Result<(), store::Error> {
        Store::append(self, lease, turn_id, entries)
            .await
            .map_err(Into::into)
    }

    async fn create_message(
        &self,
        lease: &Lease,
        role: &str,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, store::Error> {
        Store::create_message(self, lease, role, content, metadata)
            .await
            .map_err(Into::into)
    }

    async fn delete_message(&self, lease: &Lease, id: Uuid) -> Result<bool, store::Error> {
        Store::delete_message(self, lease, id)
            .await
            .map_err(Into::into)
    }

    async fn consumed(&self, lease: &Lease, ids: &[String]) -> Result<(), store::Error> {
        Store::consumed(self, lease, ids).await.map_err(Into::into)
    }

    async fn complete(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, store::Error> {
        Store::complete(self, lease, turn_id, content, metadata)
            .await
            .map_err(Into::into)
    }

    async fn publish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<StoredMessage, store::Error> {
        Store::publish(self, lease, turn_id, content, metadata)
            .await
            .map_err(Into::into)
    }

    async fn finish(
        &self,
        lease: &Lease,
        turn_id: Uuid,
        content: &str,
        metadata: Option<Value>,
        interrupted: bool,
        partial: Option<&ReplayMessage>,
    ) -> Result<StoredMessage, store::Error> {
        Store::finish(
            self,
            lease,
            turn_id,
            content,
            metadata,
            interrupted,
            partial,
        )
        .await
        .map_err(Into::into)
    }

    async fn interrupt(&self, lease: &Lease, turn_id: Uuid) -> Result<(), store::Error> {
        Store::interrupt(self, lease, turn_id)
            .await
            .map_err(Into::into)
    }

    async fn recover(&self, lease: &Lease) -> Result<usize, store::Error> {
        Store::recover(self, lease).await.map_err(Into::into)
    }

    async fn load(&self) -> Result<History, store::Error> {
        Store::load(self).await.map_err(Into::into)
    }

    async fn checkpoint(
        &self,
        lease: &Lease,
        expected: Option<&Summary>,
        proposed: &Summary,
    ) -> Result<(), store::Error> {
        Store::checkpoint(self, lease, expected, proposed)
            .await
            .map_err(Into::into)
    }

    async fn evidence(&self, id: &str, offset: u64, limit: u64) -> Result<Evidence, store::Error> {
        Store::evidence(self, id, offset, limit)
            .await
            .map_err(Into::into)
    }

    async fn catalog(&self, offset: u64, limit: u64) -> Result<Evidence, store::Error> {
        Store::catalog(self, offset, limit)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_caps_requested_limit_to_context_budget() {
        let content = "α".repeat(20_000);
        let evidence = Store::page("entry", &content, 0, 1_000_000).unwrap();
        assert_eq!(evidence.content.chars().count() as u64, PAGE_CHARS);
        assert_eq!(evidence.content, "α".repeat(PAGE_CHARS as usize));
        assert_eq!(evidence.offset, 0);
        assert_eq!(evidence.next, Some(PAGE_CHARS));
        assert_eq!(evidence.total, 20_000);
        let rest = Store::page("entry", &content, evidence.next.unwrap(), 1_000_000).unwrap();
        assert_eq!(rest.content.chars().count() as u64, PAGE_CHARS);
        assert_eq!(rest.offset, PAGE_CHARS);
        assert_eq!(rest.next, Some(PAGE_CHARS * 2));
    }
}
