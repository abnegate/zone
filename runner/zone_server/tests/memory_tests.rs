//! Memory end to end: what a turn remembers, what the next turn is told, who
//! cannot see it, and what a background run is given.
//!
//! The store, the tools, the renderer and the two append points each have
//! their own tests. None of them can answer the question the feature was built
//! for — does something a chat wrote reach the next turn's system prompt — so
//! every test here drives the real socket, the real agent loop and the real
//! provider request, and reads the answer out of what the model was actually
//! sent.
//!
//! Each test makes its own organization, workspace and people and takes them
//! out again: `knowledge_entries` is shared with every other suite on this
//! database, and a leaked memory row is indistinguishable from a real one.

mod common;

use std::time::Duration;

use common::context::{
    Harness, MODEL, Script, Socket, answer, calls, finish, next, ordinary, send, successful, tool,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::connect_async;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_server::agent::ActionTarget;
use zone_server::agent::memory::render;
use zone_server::agent::memory::render::Recall;
use zone_server::agent::memory::rules::Refusal;
use zone_server::agent::memory::{
    MEMORY_APPEND, MEMORY_DELETE, MEMORY_LIST, MEMORY_READ, MEMORY_WRITE, version_required,
};
use zone_server::db::memory::{
    self, MEMORY_CATEGORY_PREFIX, MemoryCategory, MemoryWrite, PREFERENCES_TITLE, PROFILE_TITLE,
};
use zone_server::db::workspace_members::{WorkspaceRole, add_member};
use zone_server::db::{chats, tasks, users};
use zone_server::services::chat::session::{self, Mode};

/// Wide enough that nothing here compacts by accident. The one test that wants
/// a compaction asks for a budget instead.
const WINDOW: u64 = 200_000;

/// Keys `ws::chat` writes into a message's metadata. Both are private
/// constants there — the console pins `memory_used` by reading the Rust
/// source — so a test on the far side of the socket has to spell them out.
const MEMORY_USED: &str = "memory_used";
const RECEIPTS: &str = "action_receipts";

/// What the model is scripted to remember, and what a prompt must then carry.
const PROFILE_CONTENT: &str =
    "Reviews pull requests first thing in the morning, before anything else.";
const PREFERENCES_CONTENT: &str = "Answer with the change first and the reasoning after it.";
const DEPLOY_WINDOW: &str = "Deploy window";
const DEPLOY_PURPOSE: &str = "When deploys go out";
const DEPLOY_CONTENT: &str = "Thursdays, after standup.";

/// A line the rulebook refuses, in the family `agent::memory::rules` proves it
/// catches. The refusal is asserted through `Refusal::Secret`, never retyped.
const CREDENTIAL: &str = "The deploy token is ghp_abcdefgh1234, keep it handy.";

/// Two people's remembered entries, worded so a block carrying the wrong
/// person's is unmistakable in the failure text.
const AUTHOR_PROFILE: &str = "Wrote this task and is not watching the run.";
const AUTHOR_PREFERENCES: &str = "Report findings as a numbered list.";
const STARTER_PROFILE: &str = "Started this run and is waiting on its answer.";
const STARTER_PREFERENCES: &str = "Report findings as a single paragraph.";

/// The model a task run pins, so nothing resolves an automatic one.
const TASK_MODEL: &str = "gpt-4";

/// Longest a task run is given to reach its provider and finish.
const RUN: Duration = Duration::from_secs(120);

/// One remembered entry, read outside the store so a scoping mistake inside it
/// cannot hide a row the store then cannot see.
#[derive(Debug, Clone, sqlx::FromRow)]
struct Stored {
    title: String,
    content: String,
    category: String,
    version: i64,
    is_active: bool,
    created_by: Option<Uuid>,
}

/// A chat on a real socket, with a scripted provider behind it, and the person
/// whose memory the chat writes.
struct Remembering {
    harness: Harness,
    user: Uuid,
}

impl Remembering {
    async fn open() -> Self {
        Self::around(Harness::new(Some(WINDOW), true, Vec::new()).await).await
    }

    /// The same, with a compaction budget that holds `pages` tool result pages
    /// and half of one more, so the page after those has to compact.
    async fn compacting(pages: u64) -> Self {
        Self::around(Harness::holding(pages, true, Vec::new()).await).await
    }

    async fn around(harness: Harness) -> Self {
        let user =
            sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1")
                .bind(harness.workspace)
                .fetch_one(&harness.pool)
                .await
                .expect("whoever made the workspace is a member of it");
        Self { harness, user }
    }

    fn script(&self) -> &Script {
        &self.harness.script
    }

    fn pool(&self) -> &PgPool {
        &self.harness.pool
    }

    /// One turn over the socket, which must have ended without an error: a
    /// turn that failed proves nothing about what its prompt carried.
    async fn turn(&self, content: &str) -> Vec<Value> {
        let frames = self.harness.turn(content).await;
        successful(&frames);
        frames
    }

    /// Every round of the agent loop the provider has been sent, in order.
    async fn rounds(&self) -> Vec<Value> {
        let requests = self.harness.requests().await;
        ordinary(&requests).into_iter().cloned().collect()
    }

    async fn rows(&self) -> Vec<Stored> {
        sqlx::query_as(
            "SELECT title, content, category, version, is_active, created_by
             FROM knowledge_entries WHERE workspace_id = $1 AND category LIKE $2
             ORDER BY title",
        )
        .bind(self.harness.workspace)
        .bind(format!("{MEMORY_CATEGORY_PREFIX}%"))
        .fetch_all(self.pool())
        .await
        .expect("the workspace's memory is readable")
    }

    async fn row(&self, title: &str) -> Stored {
        let rows = self.rows().await;
        rows.iter()
            .find(|row| row.title == title)
            .unwrap_or_else(|| panic!("nothing is remembered at {title:?}: {rows:?}"))
            .clone()
    }

    /// What each assistant turn stored on its message, oldest first. A turn
    /// that stored nothing reads as `null` rather than dropping out of the
    /// list, so a metadata assertion still names the turn it is about.
    async fn stored(&self) -> Vec<Value> {
        sqlx::query_scalar::<_, Option<Value>>(
            "SELECT metadata FROM messages WHERE chat_id = $1 AND role = 'assistant'
             ORDER BY created_at, id",
        )
        .bind(self.harness.chat)
        .fetch_all(self.pool())
        .await
        .expect("the chat's messages are readable")
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
    }

    /// Take the fixture back out of the shared database. `also` names anyone
    /// the test registered beyond the person the chat belongs to.
    async fn clean(self, also: &[Uuid]) {
        let mut people = vec![self.user];
        people.extend_from_slice(also);
        common::discard(self.pool(), self.harness.workspace, &people).await;
    }
}

/// The system prompt a round opened with.
fn system(round: &Value) -> &str {
    round["messages"][0]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("a round opens with a system message: {round}"))
}

/// What the tool behind `id` told the model, read out of the round that
/// replayed it. A refusal arrives here too, behind the tool layer's own
/// prefix, so an assertion on a frozen refusal ends the string rather than
/// equalling it.
fn replied<'a>(round: &'a Value, id: &str) -> &'a str {
    round["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("a round carries messages: {round}"))
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .and_then(|message| message["content"].as_str())
        .unwrap_or_else(|| panic!("nothing answered {id} in {round}"))
}

/// The tool names this round's catalog offered.
fn offered(round: &Value) -> Vec<&str> {
    round["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

fn ended(frames: &[Value]) -> &Value {
    frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap_or_else(|| panic!("the turn ended: {frames:?}"))
}

/// The receipts one stored message carries, in the order they were minted.
fn receipts(metadata: &Value) -> Vec<&Value> {
    metadata[RECEIPTS]
        .as_array()
        .map(|receipts| receipts.iter().collect())
        .unwrap_or_default()
}

/// The word the console reads off a memory receipt, taken from the enum rather
/// than retyped.
fn memory_target() -> Value {
    serde_json::to_value(ActionTarget::Memory).expect("the target serializes")
}

/// The stable key a memory receipt names its entry by.
fn entry(category: MemoryCategory, title: &str) -> String {
    format!("{category}/{title}")
}

#[tokio::test]
async fn a_scripted_write_lands_a_row_for_its_owner_at_version_one() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-fact",
            MEMORY_WRITE,
            json!({
                "category": MemoryCategory::Fact.short(),
                "name": DEPLOY_WINDOW,
                "description": DEPLOY_PURPOSE,
                "content": DEPLOY_CONTENT,
            }),
        )],
    ));
    remembering.script().push(answer("Remembered."));

    remembering
        .turn("Remember that deploys go out on Thursdays, after standup.")
        .await;

    let rows = remembering.rows().await;
    assert_eq!(rows.len(), 1, "one write, one entry: {rows:?}");
    let row = &rows[0];
    assert_eq!(row.title, DEPLOY_WINDOW);
    assert_eq!(row.content, DEPLOY_CONTENT);
    assert_eq!(row.category, MemoryCategory::Fact.as_str());
    assert_eq!(row.version, 1, "a first write opens at version one");
    assert!(row.is_active);
    assert_eq!(
        row.created_by,
        Some(remembering.user),
        "the entry belongs to whoever was in the chat"
    );

    remembering.clean(&[]).await;
}

/// The assertion the whole PR is for: what one turn remembered, the next turn
/// is told, in the system message the provider is really sent.
#[tokio::test]
async fn the_next_turn_carries_what_this_turn_remembered() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Two things to keep.",
        vec![
            tool(
                "write-profile",
                MEMORY_WRITE,
                json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
            ),
            tool(
                "write-fact",
                MEMORY_WRITE,
                json!({
                    "category": MemoryCategory::Fact.short(),
                    "name": DEPLOY_WINDOW,
                    "description": DEPLOY_PURPOSE,
                    "content": DEPLOY_CONTENT,
                }),
            ),
        ],
    ));
    remembering.script().push(answer("Remembered both."));
    remembering
        .script()
        .push(answer("Thursdays, as you told me."));

    remembering
        .turn("Remember I review in the morning, and that deploys go out Thursdays.")
        .await;
    remembering.turn("When do deploys go out?").await;

    let rounds = remembering.rounds().await;
    assert_eq!(rounds.len(), 3, "two rounds of writing, one of answering");
    let before = system(&rounds[0]);
    let after = system(&rounds[2]);

    assert!(
        !before.contains(MemoryCategory::Profile.heading()),
        "nothing was remembered yet: {before}"
    );
    assert!(
        after.contains(MemoryCategory::Profile.heading()) && after.contains(PROFILE_CONTENT),
        "the profile written last turn is missing from this one: {after}"
    );
    assert!(
        after.contains(MemoryCategory::Fact.heading())
            && after.contains(DEPLOY_WINDOW)
            && after.contains(DEPLOY_PURPOSE),
        "the fact index is missing its entry: {after}"
    );
    assert!(
        !after.contains(DEPLOY_CONTENT),
        "the index names a fact and describes it; the body is read with {MEMORY_READ}: {after}"
    );

    remembering.clean(&[]).await;
}

/// [S6] The preview quotes a cost for the send it predicts, so both have to be
/// composed over the same remembered bytes. Not whole-prompt identity: the
/// catalogs differ by construction and the generation may append retrieved
/// context on the arm that has no tools.
#[tokio::test]
async fn the_preview_and_the_generation_carry_the_same_memory_bytes() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-profile",
            MEMORY_WRITE,
            json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
        )],
    ));
    remembering.script().push(answer("Remembered."));
    remembering.script().push(answer("Understood."));

    remembering.turn("Remember I review in the morning.").await;
    remembering.turn("Anything else to know?").await;

    let expected = render::prompt(
        remembering.pool(),
        Recall::Indexed,
        remembering.harness.workspace,
        remembering.user,
    )
    .await
    .expect("the block renders");
    assert!(
        expected.contains(PROFILE_CONTENT),
        "the fixture remembered nothing: {expected:?}"
    );

    let state = common::create_test_state(
        remembering.harness.config.clone(),
        remembering.pool().clone(),
    );
    let chat = chats::get_chat(remembering.pool(), remembering.harness.chat)
        .await
        .expect("the chat is readable")
        .expect("the chat is still there");
    let preview = session::build(&state, &chat, remembering.user, None, Mode::Preview)
        .await
        .expect("a preview builds");
    let previewed = preview.context.entries[0]
        .message
        .content
        .as_deref()
        .expect("the preview opens with a system message");

    assert_eq!(preview.memory, expected);
    assert!(
        previewed.ends_with(&expected),
        "the preview does not end in the block it was built with: {previewed}"
    );

    let rounds = remembering.rounds().await;
    assert!(
        system(&rounds[2]).ends_with(&expected),
        "the generation overwrote the system message with different memory: {}",
        system(&rounds[2])
    );

    remembering.clean(&[]).await;
}

/// The badge is server-derived from the turn's own calls, so a turn that wrote
/// and a turn that called nothing must both leave it off.
#[tokio::test]
async fn memory_used_marks_the_turn_that_read_and_no_other() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-profile",
            MEMORY_WRITE,
            json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
        )],
    ));
    remembering.script().push(answer("Remembered."));
    remembering.script().push(calls(
        "Checking what I have.",
        vec![tool(
            "read-profile",
            MEMORY_READ,
            json!({"category": MemoryCategory::Profile.short()}),
        )],
    ));
    remembering
        .script()
        .push(answer("You review in the morning."));
    remembering.script().push(answer("Nothing else needed."));

    let wrote = remembering.turn("Remember I review in the morning.").await;
    let read = remembering.turn("What do you have about me?").await;
    let quiet = remembering.turn("Thanks.").await;

    assert!(
        ended(&wrote)["metadata"][MEMORY_USED].is_null(),
        "a write is not a read: {:?}",
        ended(&wrote)
    );
    assert_eq!(
        ended(&read)["metadata"][MEMORY_USED],
        json!(true),
        "the turn that read is not marked: {:?}",
        ended(&read)
    );
    assert!(
        ended(&quiet)["metadata"][MEMORY_USED].is_null(),
        "a turn that called nothing is marked: {:?}",
        ended(&quiet)
    );

    let stored = remembering.stored().await;
    assert_eq!(stored.len(), 3, "three turns, three messages: {stored:?}");
    assert!(stored[0][MEMORY_USED].is_null(), "{}", stored[0]);
    assert_eq!(
        stored[1][MEMORY_USED],
        json!(true),
        "the badge did not survive into storage: {}",
        stored[1]
    );
    assert!(stored[2][MEMORY_USED].is_null(), "{}", stored[2]);

    remembering.clean(&[]).await;
}

/// [c7] The receipt is the only record the console gets of a memory write, and
/// a read must not mint one: it would report a change that never happened.
#[tokio::test]
async fn every_writer_leaves_a_memory_receipt_and_a_read_leaves_none() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Two things to keep.",
        vec![
            tool(
                "write-profile",
                MEMORY_WRITE,
                json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
            ),
            tool(
                "write-fact",
                MEMORY_WRITE,
                json!({
                    "category": MemoryCategory::Fact.short(),
                    "name": DEPLOY_WINDOW,
                    "description": DEPLOY_PURPOSE,
                    "content": DEPLOY_CONTENT,
                }),
            ),
        ],
    ));
    remembering.script().push(answer("Remembered both."));
    remembering.script().push(calls(
        "Adding to that.",
        vec![tool(
            "append-fact",
            MEMORY_APPEND,
            json!({
                "category": MemoryCategory::Fact.short(),
                "name": DEPLOY_WINDOW,
                "content": "Frozen the week of a release.",
            }),
        )],
    ));
    remembering.script().push(answer("Added."));
    remembering.script().push(calls(
        "Taking that out.",
        vec![tool(
            "delete-fact",
            MEMORY_DELETE,
            json!({
                "category": MemoryCategory::Fact.short(),
                "name": DEPLOY_WINDOW,
                "version": 2,
            }),
        )],
    ));
    remembering.script().push(answer("Forgotten."));
    remembering.script().push(calls(
        "Checking what I have.",
        vec![tool(
            "read-profile",
            MEMORY_READ,
            json!({"category": MemoryCategory::Profile.short()}),
        )],
    ));
    remembering
        .script()
        .push(answer("You review in the morning."));

    remembering
        .turn("Remember I review in the morning, and the deploy window.")
        .await;
    remembering
        .turn("Add that the window is frozen during a release.")
        .await;
    let forgot = remembering.turn("Forget the deploy window.").await;
    remembering.turn("What do you have about me?").await;

    let stored = remembering.stored().await;
    assert_eq!(stored.len(), 4, "four turns, four messages: {stored:?}");

    let written = receipts(&stored[0]);
    assert_eq!(written.len(), 2, "two writes, two receipts: {}", stored[0]);
    for receipt in &written {
        assert_eq!(receipt["action"], MEMORY_WRITE);
    }
    assert_eq!(
        written[0]["target_id"],
        json!(entry(MemoryCategory::Profile, PROFILE_TITLE))
    );
    // A fact is receipted under its kind and never under its name: a receipt
    // rides an assistant message, and chat access is workspace read access
    // rather than ownership, so the name one person chose for their own entry
    // is not a field the rest of the workspace gets to read.
    assert_eq!(written[1]["target_id"], json!(MemoryCategory::Fact.short()));
    for receipt in [&written[1]] {
        assert_ne!(
            receipt["target_label"],
            json!(DEPLOY_WINDOW),
            "a fact's name reached a receipt: {receipt}"
        );
    }

    let appended = receipts(&stored[1]);
    assert_eq!(appended.len(), 1, "{}", stored[1]);
    assert_eq!(appended[0]["action"], MEMORY_APPEND);

    let forgotten = receipts(&stored[2]);
    assert_eq!(forgotten.len(), 1, "{}", stored[2]);
    assert_eq!(forgotten[0]["action"], MEMORY_DELETE);

    for receipt in written.iter().chain(&appended).chain(&forgotten) {
        assert_eq!(
            receipt["target_type"],
            memory_target(),
            "a memory write is filed under another kind of target: {receipt}"
        );
        assert_eq!(receipt["success"], json!(true), "{receipt}");
        assert_eq!(
            receipt["href"],
            json!(""),
            "memory has no page, so a receipt must offer no link: {receipt}"
        );
        assert_eq!(receipt["actor_id"], json!(remembering.user.to_string()));
        assert_ne!(
            receipt["target_id"],
            json!(entry(MemoryCategory::Fact, DEPLOY_WINDOW)),
            "a fact's name reached a receipt: {receipt}"
        );
    }

    assert!(
        receipts(&stored[3]).is_empty(),
        "a read minted a receipt: {}",
        stored[3]
    );
    assert!(
        stored[3][RECEIPTS].is_null(),
        "a turn that changed nothing carries an empty receipt list: {}",
        stored[3]
    );

    assert_eq!(
        ended(&forgot)["metadata"][RECEIPTS]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the receipt reached the live frame too: {:?}",
        ended(&forgot)
    );

    remembering.clean(&[]).await;
}

/// `knowledge_entries` is workspace-wide, so the only thing between two
/// colleagues is `created_by`. Both halves are checked: what the second
/// member's prompt carries, and what their own read is told.
#[tokio::test]
async fn a_second_member_sees_none_of_the_first_ones_memory() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Two things to keep.",
        vec![
            tool(
                "write-profile",
                MEMORY_WRITE,
                json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
            ),
            tool(
                "write-fact",
                MEMORY_WRITE,
                json!({
                    "category": MemoryCategory::Fact.short(),
                    "name": DEPLOY_WINDOW,
                    "description": DEPLOY_PURPOSE,
                    "content": DEPLOY_CONTENT,
                }),
            ),
        ],
    ));
    remembering.script().push(answer("Remembered both."));
    remembering
        .turn("Remember I review in the morning, and the deploy window.")
        .await;

    let (token, other) = register(&remembering.harness).await;
    add_member(
        remembering.pool(),
        remembering.harness.workspace,
        other,
        WorkspaceRole::Member,
        None,
    )
    .await
    .expect("the second person joins the workspace");
    let chat = chat_for(&remembering.harness, &token).await;

    remembering.script().push(calls(
        "Checking what I have.",
        vec![tool(
            "read-profile",
            MEMORY_READ,
            json!({"category": MemoryCategory::Profile.short()}),
        )],
    ));
    remembering
        .script()
        .push(answer("I have nothing remembered for you."));

    let mut socket = connect(&remembering.harness.address, &chat, &token).await;
    send(
        &mut socket,
        json!({"type": "send", "content": "What do you have about me?"}),
    )
    .await;
    successful(&finish(&mut socket).await);
    let _ = socket.close(None).await;

    let rounds = remembering.rounds().await;
    assert_eq!(rounds.len(), 4, "two turns of two rounds each");
    let theirs = system(&rounds[2]);
    for leaked in [
        MemoryCategory::Profile.heading(),
        MemoryCategory::Fact.heading(),
        PROFILE_CONTENT,
        DEPLOY_WINDOW,
    ] {
        assert!(
            !theirs.contains(leaked),
            "another member's prompt carries {leaked:?}: {theirs}"
        );
    }
    assert!(
        replied(&rounds[3], "read-profile")
            .ends_with(&memory::missing(MemoryCategory::Profile, PROFILE_TITLE)),
        "a colleague's read found something: {}",
        replied(&rounds[3], "read-profile")
    );

    let rows = remembering.rows().await;
    assert_eq!(rows.len(), 2, "the first person still has both: {rows:?}");

    remembering.clean(&[other]).await;
}

/// A write that quotes a version the entry has moved past must hand back what
/// the entry says now, and must not overwrite it.
#[tokio::test]
async fn a_stale_write_is_refused_with_what_the_entry_says_now() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-profile",
            MEMORY_WRITE,
            json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
        )],
    ));
    remembering.script().push(answer("Remembered."));
    remembering.script().push(calls(
        "Adding to that.",
        vec![tool(
            "append-profile",
            MEMORY_APPEND,
            json!({
                "category": MemoryCategory::Profile.short(),
                "content": "Works in Auckland time.",
            }),
        )],
    ));
    remembering.script().push(answer("Added."));
    remembering.script().push(calls(
        "Replacing what I read.",
        vec![tool(
            "stale-profile",
            MEMORY_WRITE,
            json!({
                "category": MemoryCategory::Profile.short(),
                "content": "Reviews whenever.",
                "version": 1,
            }),
        )],
    ));
    remembering.script().push(answer("I will merge that."));

    remembering.turn("Remember I review in the morning.").await;
    remembering.turn("Add that I work in Auckland time.").await;

    let current = remembering.row(PROFILE_TITLE).await;
    assert_eq!(current.version, 2, "the append moved the entry on");

    remembering
        .turn("Change it to say I review whenever.")
        .await;

    let rounds = remembering.rounds().await;
    assert!(
        replied(&rounds[5], "stale-profile").ends_with(&memory::conflict(
            MemoryCategory::Profile,
            PROFILE_TITLE,
            current.version,
            &current.content
        )),
        "a stale write was not told what the entry says now: {}",
        replied(&rounds[5], "stale-profile")
    );

    let after = remembering.row(PROFILE_TITLE).await;
    assert_eq!(after.version, current.version, "the stale write landed");
    assert_eq!(after.content, current.content, "the stale write landed");

    remembering.clean(&[]).await;
}

#[tokio::test]
async fn a_credential_is_refused_and_nothing_is_stored() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-secret",
            MEMORY_WRITE,
            json!({"category": MemoryCategory::Profile.short(), "content": CREDENTIAL}),
        )],
    ));
    remembering
        .script()
        .push(answer("I will not keep a token for you."));

    remembering.turn("Remember my deploy token.").await;

    let rounds = remembering.rounds().await;
    assert!(
        replied(&rounds[1], "write-secret").ends_with(Refusal::Secret.message()),
        "a credential was taken, or refused in other words: {}",
        replied(&rounds[1], "write-secret")
    );
    let rows = remembering.rows().await;
    assert!(
        rows.is_empty(),
        "a refused write left a row behind: {rows:?}"
    );

    remembering.clean(&[]).await;
}

/// Compaction rewrites the projection around the system message. The block
/// lives on that message, so a turn long enough to compact is the one place it
/// could silently go.
#[tokio::test]
async fn the_block_survives_a_compaction() {
    let remembering = Remembering::compacting(1).await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-profile",
            MEMORY_WRITE,
            json!({"category": MemoryCategory::Profile.short(), "content": PROFILE_CONTENT}),
        )],
    ));
    remembering.script().push(answer("Remembered."));
    remembering.turn("Remember I review in the morning.").await;

    let first = remembering
        .harness
        .file("first.txt", &format!("FIRST\n{}", "a".repeat(80_000)));
    let second = remembering
        .harness
        .file("second.txt", &format!("SECOND\n{}", "b".repeat(80_000)));
    remembering.script().push(calls(
        "Reading the first.",
        vec![tool("read-first", "read_file", json!({"path": first}))],
    ));
    remembering.script().push(calls(
        "Reading the second.",
        vec![tool("read-second", "read_file", json!({"path": second}))],
    ));
    remembering.script().push(answer("Read them both."));

    remembering.turn("Read both of those files.").await;

    assert!(
        remembering.harness.history().await.summary.is_some(),
        "the turn was not long enough to compact, so it proves nothing"
    );

    let rounds = remembering.rounds().await;
    let last = system(rounds.last().expect("the turn reached the provider"));
    assert!(
        last.contains(MemoryCategory::Profile.heading()) && last.contains(PROFILE_CONTENT),
        "compaction took the block off the system message: {last}"
    );

    remembering.clean(&[]).await;
}

/// A delete has to prove it read what it is removing, and what it removes has
/// to leave the next turn's prompt.
#[tokio::test]
async fn a_delete_with_its_version_takes_the_entry_out_of_the_next_prompt() {
    let remembering = Remembering::open().await;
    remembering.script().push(calls(
        "Noting that.",
        vec![tool(
            "write-fact",
            MEMORY_WRITE,
            json!({
                "category": MemoryCategory::Fact.short(),
                "name": DEPLOY_WINDOW,
                "description": DEPLOY_PURPOSE,
                "content": DEPLOY_CONTENT,
            }),
        )],
    ));
    remembering.script().push(answer("Remembered."));
    remembering.script().push(calls(
        "Taking that out.",
        vec![tool(
            "bare-delete",
            MEMORY_DELETE,
            json!({"category": MemoryCategory::Fact.short(), "name": DEPLOY_WINDOW}),
        )],
    ));
    remembering
        .script()
        .push(answer("I need the version first."));
    remembering.script().push(calls(
        "Quoting the version.",
        vec![tool(
            "versioned-delete",
            MEMORY_DELETE,
            json!({
                "category": MemoryCategory::Fact.short(),
                "name": DEPLOY_WINDOW,
                "version": 1,
            }),
        )],
    ));
    remembering.script().push(answer("Forgotten."));
    remembering.script().push(answer("Nothing on file."));

    remembering
        .turn("Remember that deploys go out on Thursdays.")
        .await;
    // A refused delete and the delete that follows it are separate turns: a
    // failed write is no progress, so the loop that saw one finalizes rather
    // than offering the tools again.
    remembering.turn("Forget the deploy window.").await;
    remembering.turn("Try that again with the version.").await;
    remembering.turn("When do deploys go out?").await;

    let rounds = remembering.rounds().await;
    assert!(
        system(&rounds[2]).contains(DEPLOY_WINDOW),
        "the fact was not there to delete: {}",
        system(&rounds[2])
    );
    assert!(
        replied(&rounds[3], "bare-delete").ends_with(&version_required()),
        "a delete without a version was allowed: {}",
        replied(&rounds[3], "bare-delete")
    );
    assert!(
        system(&rounds[4]).contains(DEPLOY_WINDOW),
        "the refused delete took the entry anyway: {}",
        system(&rounds[4])
    );
    assert_eq!(
        replied(&rounds[5], "versioned-delete"),
        memory::forgotten(MemoryCategory::Fact, DEPLOY_WINDOW),
    );

    let last = system(&rounds[6]);
    assert!(
        !last.contains(DEPLOY_WINDOW) && !last.contains(MemoryCategory::Fact.heading()),
        "a forgotten entry is still in the prompt: {last}"
    );

    let row = remembering.row(DEPLOY_WINDOW).await;
    assert!(!row.is_active, "the entry is still readable: {row:?}");

    remembering.clean(&[]).await;
}

/// [c1] A background run reads the person's profile and preferences and acts
/// on them; it is given no fact index, because it has no `memory_read` to
/// follow one with, and no memory tool, because there is nobody watching.
#[tokio::test]
async fn a_task_run_reads_the_profile_and_is_given_no_fact_index_or_memory_tool() {
    let pool = common::create_test_pool().await;
    let (_organization, workspace, user) = common::setup_workspace_member(&pool).await;
    for (category, title, content) in [
        (MemoryCategory::Profile, PROFILE_TITLE, PROFILE_CONTENT),
        (
            MemoryCategory::Preference,
            PREFERENCES_TITLE,
            PREFERENCES_CONTENT,
        ),
        (MemoryCategory::Fact, DEPLOY_WINDOW, DEPLOY_CONTENT),
    ] {
        memory::write(
            &pool,
            MemoryWrite {
                workspace_id: workspace,
                user_id: user,
                category,
                title,
                description: Some(DEPLOY_PURPOSE),
                content,
                version: None,
            },
        )
        .await
        .expect("the entry is written");
    }

    let provider = answering("Looked around and found nothing to change.").await;
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let state = common::create_test_state(config, pool.clone());

    let task = tasks::create_task_as(
        &pool,
        workspace,
        &[],
        "Look around",
        "Report what you find",
        None,
        None,
        true,
        None,
        Some(user),
    )
    .await
    .expect("a task to run");
    sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
        .bind(task.id)
        .bind(TASK_MODEL)
        .execute(&pool)
        .await
        .expect("the task pins a model");
    let run = tasks::create_task_run_as(&pool, task.id, Some(user))
        .await
        .expect("a run of it");

    tokio::time::timeout(
        RUN,
        zone_server::workers::task::execute_task_run(&state, run.id, task.id),
    )
    .await
    .expect("the run finishes inside its window");

    let rounds: Vec<Value> = provider
        .received_requests()
        .await
        .expect("the provider recorded its requests")
        .into_iter()
        .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
        .filter(|request| request["stream"] == json!(true))
        .collect();
    let guidance = system(rounds.first().expect("the run reached the provider"));

    assert!(
        guidance.contains(MemoryCategory::Profile.heading()) && guidance.contains(PROFILE_CONTENT),
        "the run was given no profile: {guidance}"
    );
    assert!(
        guidance.contains(MemoryCategory::Preference.heading())
            && guidance.contains(PREFERENCES_CONTENT),
        "the run was given no preferences: {guidance}"
    );
    assert!(
        !guidance.contains(MemoryCategory::Fact.heading()) && !guidance.contains(DEPLOY_WINDOW),
        "a run that cannot read a fact was given the index of them: {guidance}"
    );

    let offered = offered(rounds.first().expect("the run reached the provider"));
    for name in [
        MEMORY_LIST,
        MEMORY_READ,
        MEMORY_WRITE,
        MEMORY_APPEND,
        MEMORY_DELETE,
    ] {
        assert!(
            !offered.contains(&name),
            "a background run was handed {name}: {offered:?}"
        );
    }

    common::discard(&pool, workspace, &[user]).await;
}

/// [C1] The run's block follows whoever started it. Editing a task and
/// starting a run of it are both open to any member, so a block fetched on the
/// task's author would show one member their colleague's profile in the output
/// of a run that member started.
#[tokio::test]
async fn a_run_carries_the_memory_of_whoever_started_it_and_none_of_the_author_s() {
    let pool = common::create_test_pool().await;
    let (_organization, workspace, author) = common::setup_workspace_member(&pool).await;
    let starter = users::create_user(
        &pool,
        &format!("starter-{}@example.test", Uuid::new_v4()),
        "password_hash",
        Some("The member who starts it"),
        false,
    )
    .await
    .expect("a second person exists")
    .id;
    add_member(&pool, workspace, starter, WorkspaceRole::Member, None)
        .await
        .expect("the second person joins the workspace");

    for (user, profile, preferences) in [
        (author, AUTHOR_PROFILE, AUTHOR_PREFERENCES),
        (starter, STARTER_PROFILE, STARTER_PREFERENCES),
    ] {
        for (category, title, content) in [
            (MemoryCategory::Profile, PROFILE_TITLE, profile),
            (MemoryCategory::Preference, PREFERENCES_TITLE, preferences),
        ] {
            memory::write(
                &pool,
                MemoryWrite {
                    workspace_id: workspace,
                    user_id: user,
                    category,
                    title,
                    description: None,
                    content,
                    version: None,
                },
            )
            .await
            .expect("the entry is written");
        }
    }

    let task = pinned_task(&pool, workspace, Some(author)).await;
    let run = tasks::create_task_run_as(&pool, task, Some(starter))
        .await
        .expect("the second member starts a run of it");
    let started = system(&first_round(&pool, task, run.id).await).to_string();

    let scheduled_task = pinned_task(&pool, workspace, None).await;
    let unattributed = tasks::create_task_run(&pool, scheduled_task)
        .await
        .expect("a run nobody started");
    let scheduled = system(&first_round(&pool, scheduled_task, unattributed.id).await).to_string();

    // Everything asserted below is already in hand, so a red run takes its
    // fixtures back out of a database it shares with every other suite.
    common::discard(&pool, workspace, &[author, starter]).await;

    assert_eq!(
        run.triggered_by,
        Some(starter),
        "the run is not attributed to the member who started it"
    );
    assert!(
        started.contains(STARTER_PROFILE) && started.contains(STARTER_PREFERENCES),
        "the run was given none of the memory of whoever started it: {started}"
    );
    assert!(
        !started.contains(AUTHOR_PROFILE) && !started.contains(AUTHOR_PREFERENCES),
        "the run rendered the task author's memory to the member who started it: {started}"
    );

    assert_eq!(
        unattributed.triggered_by, None,
        "a scheduled run is attributed to somebody"
    );
    assert!(
        !scheduled.contains(MemoryCategory::Profile.heading())
            && !scheduled.contains(MemoryCategory::Preference.heading())
            && !scheduled.contains(AUTHOR_PROFILE)
            && !scheduled.contains(STARTER_PROFILE),
        "a run nobody started was given somebody's memory: {scheduled}"
    );
}

/// An agentic task pinned to a model so nothing resolves an automatic one.
/// A `None` author is the task nobody owns, the one shape whose runs execute
/// without an actor at all.
async fn pinned_task(pool: &PgPool, workspace: Uuid, author: Option<Uuid>) -> Uuid {
    let task = tasks::create_task_as(
        pool,
        workspace,
        &[],
        "Look around",
        "Report what you find",
        None,
        None,
        true,
        None,
        author,
    )
    .await
    .expect("a task to run");
    sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
        .bind(task.id)
        .bind(TASK_MODEL)
        .execute(pool)
        .await
        .expect("the task pins a model");
    task.id
}

/// Run one task run against a provider of its own and hand back the first
/// round it reached, so two runs in one test cannot be confused for each other.
async fn first_round(pool: &PgPool, task: Uuid, run: Uuid) -> Value {
    let provider = answering("Looked around and found nothing to change.").await;
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let state = common::create_test_state(config, pool.clone());

    tokio::time::timeout(
        RUN,
        zone_server::workers::task::execute_task_run(&state, run, task),
    )
    .await
    .expect("the run finishes inside its window");

    provider
        .received_requests()
        .await
        .expect("the provider recorded its requests")
        .into_iter()
        .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
        .find(|request| request["stream"] == json!(true))
        .expect("the run reached the provider")
}

/// A provider that answers every round with the same prose, and every aside
/// without a stream, so a run reaches its end rather than its retry.
async fn answering(reply: &str) -> MockServer {
    let provider = MockServer::start().await;
    let prose = reply.to_string();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            if body["stream"] == json!(true) {
                let delta = json!({
                    "model": MODEL,
                    "choices": [{"index": 0, "delta": {"content": prose}, "finish_reason": null}]
                });
                let finish = json!({
                    "model": MODEL,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                });
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(format!(
                        "data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
                    ))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "aside", "object": "chat.completion", "created": 0, "model": MODEL,
                    "choices": [{
                        "index": 0, "finish_reason": "stop",
                        "message": {"role": "assistant", "content": prose}
                    }]
                }))
            }
        })
        .mount(&provider)
        .await;
    provider
}

/// A second person on the same router, for the tests about what one member can
/// see of another.
async fn register(harness: &Harness) -> (String, Uuid) {
    let registered = harness
        .client
        .post_json(
            "/api/auth/register",
            &json!({"email": common::test_email(), "password": common::test_password()}),
        )
        .await
        .json_value();
    let token = registered["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration returns an access token: {registered}"))
        .to_string();
    let user = Uuid::parse_str(
        registered["user"]["id"]
            .as_str()
            .expect("registration returns the user"),
    )
    .expect("the user id is a uuid");
    (token, user)
}

/// Their own chat in the workspace they just joined.
async fn chat_for(harness: &Harness, token: &str) -> String {
    harness
        .client
        .post_json_auth(
            "/api/chats",
            &json!({
                "workspace_id": harness.workspace,
                "title": "Theirs",
                "model_name": MODEL,
                "agent_enabled": true,
                "automatic_title": false,
                "auto_approve": true,
            }),
            token,
        )
        .await
        .json_value()["chat"]["id"]
        .as_str()
        .expect("a chat")
        .to_string()
}

async fn connect(address: &str, chat: &str, token: &str) -> Socket {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{chat}"))
        .await
        .expect("the chat socket accepts a connection");
    send(&mut socket, json!({"type": "auth", "token": token})).await;
    let initial = next(&mut socket).await;
    assert_eq!(
        initial["type"], "init",
        "the socket did not initialize: {initial}"
    );
    socket
}
