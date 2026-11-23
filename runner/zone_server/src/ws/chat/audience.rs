/// Who a turn answers to. It decides whether the chat's auto-approve applies and whether the
/// chat is told when fleet policy refuses this runner the chat's lease.
///
/// Auto-approve is a standing "yes" a person gave while they were there to see what they said
/// yes to. A turn nobody sent over the socket runs without it: its prompt can carry text the model
/// wrote from something it read, so a tool that writes to the host or leaves the workspace is
/// still put to somebody, finds no one, and is declined. Reading tools are below that tier and
/// run either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// A person sent it over the chat's socket.
    Interactive,
    /// Started for a person who isn't on the socket yet, such as an auto project's planner
    /// answering its brief. Nothing else will run it, so a refusal is told to the chat.
    Watched,
    /// Owed by a schedule, such as a reminder's firing. A refusal leaves it owed and another
    /// runner takes it, so the chat is told nothing.
    Owed,
}

impl Audience {
    pub(super) fn approves(self) -> bool {
        self == Self::Interactive
    }

    pub(super) fn announces_refusal(self) -> bool {
        self != Self::Owed
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tokio::sync::broadcast::Receiver;
    use uuid::Uuid;
    use zone_enterprise::Plan;
    use zone_enterprise::fleet::{RUNNER_REVOKED, Refusal};

    use super::Audience;
    use crate::db::fleet::state::State;
    use crate::db::organization_members::OrgRole;
    use crate::db::workspace_members::WorkspaceRole;
    use crate::services::fleet;
    use crate::services::fleet::tests::Server;
    use crate::ws::chat::{ChatStream, ServerMessage, run_turn};

    const BRIEF: &str = "Plan a small landing page.";

    #[tokio::test]
    async fn a_watched_turn_refused_its_chat_tells_the_chat_and_an_owed_one_does_not() {
        let server = Server::new(Some(Plan::Enterprise)).await;
        let pool = server.database.pool.clone();
        let (workspace, user, chat) = member_chat(&pool).await;
        let runner = fleet::claimant(&server.state)
            .await
            .runner
            .expect("the server registers to claim");
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(runner)
            .bind(State::Revoked.as_str())
            .execute(&pool)
            .await
            .expect("the runner is revoked");
        let stream = ChatStream::of(chat);
        let mut frames = stream.events.subscribe();

        let ask = |audience| run_turn(&server.state, chat, workspace, user, BRIEF, None, audience);
        ask(Audience::Watched).await;
        let watched = endings(&mut frames);
        ask(Audience::Owed).await;
        let owed = endings(&mut frames);
        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1")
            .bind(chat)
            .fetch_one(&pool)
            .await
            .expect("the chat's messages");
        server.remove().await;

        let refusal = Refusal::Revoked;
        assert_eq!(
            watched,
            [json!({"type": "denied", "message": refusal.message(), "code": RUNNER_REVOKED})],
            "a watched turn tells the chat why it didn't run"
        );
        assert!(
            owed.is_empty(),
            "an owed turn stays owed for another runner and tells the chat nothing: {owed:?}"
        );
        assert_eq!(messages, 0, "neither refused turn reached the chat");
    }

    /// The frames that end a turn without an answer, published since the last read.
    fn endings(frames: &mut Receiver<ServerMessage>) -> Vec<Value> {
        std::iter::from_fn(|| frames.try_recv().ok())
            .map(|frame| serde_json::to_value(frame).expect("the frame serialises"))
            .filter(|frame| frame["type"] == "denied" || frame["type"] == "error")
            .collect()
    }

    /// A workspace with one member who may write in it, and a chat there.
    async fn member_chat(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
        let organization: Uuid = sqlx::query_scalar(
            "INSERT INTO organizations (name, slug) VALUES ('Fleet', gen_random_uuid()::text) \
             RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("an organization");
        let workspace: Uuid = sqlx::query_scalar(
            "INSERT INTO workspaces (organization_id, name, slug) \
             VALUES ($1, 'Fleet', gen_random_uuid()::text) RETURNING id",
        )
        .bind(organization)
        .fetch_one(pool)
        .await
        .expect("a workspace");
        let user: Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) \
             VALUES (gen_random_uuid()::text || '@zone.test', 'test-only') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("a user");
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) \
             VALUES ($1, $2, $3)",
        )
        .bind(organization)
        .bind(user)
        .bind(OrgRole::Member.as_str())
        .execute(pool)
        .await
        .expect("an organization membership");
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(workspace)
        .bind(user)
        .bind(WorkspaceRole::Member.as_str())
        .execute(pool)
        .await
        .expect("a workspace membership");
        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name) \
             VALUES ($1, 'Fleet', 'test') RETURNING id",
        )
        .bind(workspace)
        .fetch_one(pool)
        .await
        .expect("a chat");
        (workspace, user, chat)
    }
}
