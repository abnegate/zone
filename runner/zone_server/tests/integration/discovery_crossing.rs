//! A discovery crossing from the chat that recorded it into the next prompt.
//!
//! `discovery` follows `record_discovery` from the tool into the store and as
//! far as the block a run reads. `prompt_assembly` follows the store into a
//! chat's system entry, and seeds its rows through the store to do it. Neither
//! joins the halves, so a tool filing its entry where a chat's reader does not
//! look would pass both. Here one chat records through [`ChatTools::execute`]
//! and a second chat of the same workspace is prepared by [`session::build`],
//! with nothing between them but the database.
//!
//! [`ChatTools::execute`]: zone_server::agent::ChatTools::execute

use crate::common;
use crate::discovery::{Fixture, record, refusal, stored};
use crate::prompt_assembly::UNREACHABLE;

use uuid::Uuid;
use zone_core::llm::LlmBackend;
use zone_server::agent::discovery::{Declined, RECORDED};
use zone_server::db::chats::{self, ChatRow};
use zone_server::db::knowledge;
use zone_server::db::knowledge::discovery::{self, Kind, Provenance, Source};
use zone_server::services::chat::session::{self, Generation, Mode, Preparation};
use zone_server::services::route::Route;
use zone_server::state::AppState;

const SUBJECT: &str = "Frontend test isolation";
const STATEMENT: &str = "Run bun test with --isolate; a bare run reports phantom failures.";

/// The heading the entries are listed under, as it opens their block.
const HEADING: &str = "\n\n# Recorded while working here\n";

const BULLET: &str = "- ";

/// A model no route is asked to serve, because preparing a turn sends nothing.
const MODEL: &str = "fixture-model";

/// A second chat in a fixture's workspace: no character card, agent on, and
/// opened by the member who recorded in the first.
struct Reader {
    state: AppState,
    chat: ChatRow,
    member: Uuid,
}

impl Reader {
    async fn of(fixture: &Fixture) -> Self {
        let mut config = common::test_config();
        config.litellm_host = UNREACHABLE.to_string();
        config.ollama_host = UNREACHABLE.to_string();
        let chat = chats::create_chat(
            &fixture.pool,
            Some(fixture.workspace),
            "Reads what was recorded",
            MODEL,
            true,
            false,
        )
        .await
        .expect("a second chat in the workspace");
        Self {
            state: common::create_test_state(config, fixture.pool.clone()),
            chat,
            member: fixture.member,
        }
    }

    /// The chat's next turn, as `session::build` prepares it.
    async fn turn(&self) -> Preparation {
        let workspace = self.chat.workspace_id.expect("the chat has a workspace");
        let endpoint = Route::for_workspace(&self.state, workspace)
            .await
            .into_endpoint()
            .expect("a usable route");
        session::build(
            &self.state,
            &self.chat,
            self.member,
            None,
            Mode::Generation(Generation::unrouted(LlmBackend::Http)),
            endpoint,
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("the turn is prepared: {error}"))
    }
}

fn system(preparation: &Preparation) -> &str {
    preparation.context.entries[0]
        .message
        .content
        .as_deref()
        .expect("the turn's system entry")
}

/// What a run of the fixture's workspace is handed.
async fn run_reads(fixture: &Fixture) -> String {
    knowledge::learned_facts_prompt(&fixture.pool, fixture.workspace).await
}

/// The entries a prompt lists after the discoveries heading, one per line, and
/// none when the heading is not in it.
fn listed(prompt: &str) -> Vec<&str> {
    prompt
        .split_once(HEADING)
        .map(|(_, entries)| {
            entries
                .lines()
                .filter(|line| line.starts_with(BULLET))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a prompt carries nothing of the entry: neither its block nor its
/// words under any other heading.
fn silent(prompt: &str) -> bool {
    !prompt.contains(HEADING) && !prompt.contains(STATEMENT)
}

/// The line the fixture's one entry renders as. The day is the one the tool
/// stamped on it, read back from the store, so the line holds across midnight.
async fn line(fixture: &Fixture) -> String {
    let entries = discovery::list(&fixture.pool, fixture.workspace)
        .await
        .expect("the workspace's discoveries are readable");
    let [entry] = entries.as_slice() else {
        panic!("one entry was recorded: {entries:?}");
    };
    let provenance = Provenance::from_tags(&entry.tags).expect("the tags read back");
    assert_eq!(
        provenance.source,
        Source::Chat(fixture.chat),
        "the entry names the chat that recorded it, not the one reading it"
    );
    format!(
        "{BULLET}{SUBJECT}: {STATEMENT} ({}, recorded in a chat on {})",
        Kind::Lesson.as_str(),
        provenance.last_confirmed
    )
}

#[tokio::test]
async fn what_one_chat_records_the_next_chat_and_the_next_run_of_its_workspace_read() {
    let fixture = Fixture::new().await;
    let elsewhere = Fixture::new().await;
    let reader = Reader::of(&fixture).await;
    let outsider = Reader::of(&elsewhere).await;
    let before = reader.turn().await;
    assert!(
        silent(system(&before)),
        "nothing is recorded yet: {}",
        system(&before)
    );
    assert_eq!(run_reads(&fixture).await, "");

    let writer = fixture.chat().await;
    let recorded = stored(&record(&writer, Kind::Lesson, SUBJECT, STATEMENT).await);
    assert_eq!(recorded["outcome"], RECORDED);
    let line = line(&fixture).await;

    let next = reader.turn().await;
    assert_eq!(
        listed(system(&next)),
        [line.as_str()],
        "the second chat's system entry: {}",
        system(&next)
    );
    let run = run_reads(&fixture).await;
    assert_eq!(listed(&run), [line.as_str()], "what a run reads: {run}");
    let outside = outsider.turn().await;
    assert!(
        silent(system(&outside)),
        "a discovery crossed into a chat of a workspace it was not recorded in: {}",
        system(&outside)
    );
    assert_eq!(
        run_reads(&elsewhere).await,
        "",
        "a discovery crossed into a run of a workspace it was not recorded in"
    );

    elsewhere.clean().await;
    fixture.clean().await;
}

#[tokio::test]
async fn a_deleted_discovery_leaves_the_next_prompt_and_cannot_be_recorded_back_into_it() {
    let fixture = Fixture::new().await;
    let reader = Reader::of(&fixture).await;
    let writer = fixture.chat().await;
    let recorded = stored(&record(&writer, Kind::Lesson, SUBJECT, STATEMENT).await);
    let id: Uuid = serde_json::from_value(recorded["id"].clone()).expect("an id");
    let line = line(&fixture).await;
    let told = reader.turn().await;
    assert_eq!(
        listed(system(&told)),
        [line.as_str()],
        "the entry is there to lose: {}",
        system(&told)
    );
    assert_eq!(listed(&run_reads(&fixture).await), [line.as_str()]);

    assert!(
        knowledge::delete_knowledge(&fixture.pool, id)
            .await
            .expect("the wiki's delete"),
        "the entry was there to delete"
    );

    let untold = reader.turn().await;
    assert!(
        silent(system(&untold)),
        "a deleted entry reached the next turn: {}",
        system(&untold)
    );
    assert_eq!(
        run_reads(&fixture).await,
        "",
        "a deleted entry reached the next run"
    );

    let again = record(&untold.tools, Kind::Lesson, SUBJECT, STATEMENT).await;

    assert_eq!(
        refusal(&again),
        Declined::Retired.message(),
        "the chat that no longer reads the entry wrote it back"
    );
    let after = reader.turn().await;
    assert!(
        silent(system(&after)),
        "a refused record reached the next turn: {}",
        system(&after)
    );
    assert_eq!(
        run_reads(&fixture).await,
        "",
        "a refused record reached the next run"
    );

    fixture.clean().await;
}
