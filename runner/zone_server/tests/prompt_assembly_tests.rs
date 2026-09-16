//! What each surface actually assembles, checked from outside the crate.
//!
//! The section tests each pin one block against a catalog they built by hand.
//! These build the catalogs the way the server does, then read the assembled
//! result, which is the only place a section that should have stayed silent, or
//! one that should have spoken, shows up.

mod common;

use chrono::DateTime;
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use uuid::Uuid;
use zone_server::agent::prompt;
use zone_server::agent::wait::WAIT_FOR;
use zone_server::agent::{ASK_USER, ChatTools, Environment, WorkspaceScope};
use zone_server::db::knowledge::{
    LearnedCategory, LearnedEntryRow, render_learned_facts, render_standing_instructions,
};

/// The sentence `workers::task` used to append after the prompt it now owns.
const SANDBOX: &str = "You are completing a background coding task.";

/// What each of the six model-facing strings said before `wait_for` existed,
/// lowercased, reduced to the half no replacement could contain.
///
/// A fragment rather than the whole string because the whole string is gone: an
/// absence asserted against text nothing ever says again is vacuous the moment
/// its owner rewords anything. These are the words that mandated the poll, so a
/// revert restores them whatever else it changes around them.
const SUPERSEDED_POLL_WORDING: [&str; 6] = [
    "do not claim the runner finished; poll",
    "does not wait for completion — poll",
    "use to monitor start_task progress",
    "runner started. poll",
    "to wait longer, return and check again in a later call",
    "return without waiting and check again in a later call",
];

/// Fixed, so a rendered prompt is the same bytes on every run.
fn environment() -> Environment {
    Environment::at(
        DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
        "Pacific/Auckland",
        PathBuf::from("/srv/zone"),
    )
}

/// A pool nothing dials, because building a catalog only reads its own registry.
fn state() -> zone_server::state::AppState {
    common::create_test_state(
        common::test_config(),
        PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap(),
    )
}

async fn task_tools() -> ChatTools {
    ChatTools::for_task(&state(), PathBuf::from("/srv/zone"), Uuid::new_v4(), None).await
}

async fn chat_tools() -> ChatTools {
    ChatTools::build(WorkspaceScope {
        state: state(),
        workspace_id: Uuid::new_v4(),
        chat_id: Some(Uuid::new_v4()),
        user_id: Uuid::new_v4(),
    })
    .await
}

#[tokio::test]
async fn a_background_run_reads_the_task_sections_and_none_of_the_chat_ones() {
    let rendered = prompt::task(&task_tools().await, &environment());

    assert!(rendered.contains(SANDBOX), "{rendered}");
    assert!(
        rendered.contains("There is no unrestricted shell."),
        "{rendered}"
    );
    assert!(
        rendered.contains("nobody is watching this run, so asking whether to proceed only stops"),
        "{rendered}"
    );
    assert!(
        rendered.contains("An answer you genuinely need comes from ask_user."),
        "{rendered}"
    );
    assert!(
        !rendered.contains("nobody can answer you mid-task"),
        "a run offered the question tool can be answered: {rendered}"
    );
    assert!(
        rendered.contains("ships with a regression test that fails without the fix"),
        "{rendered}"
    );
    assert!(rendered.contains("opens the pull request"), "{rendered}");

    for chat_only in [
        "Declining and directness:",
        "Web tools:",
        "Citing sources:",
        "Images:",
        "Cluster:",
        "act in the server runtime",
        "report your assessment and stop there",
        "Git: interactive flags",
    ] {
        assert!(!rendered.contains(chat_only), "{chat_only}: {rendered}");
    }
}

#[tokio::test]
async fn a_chat_reads_the_chat_sections_and_none_of_the_run_only_ones() {
    let rendered = prompt::chat(&chat_tools().await, false, &environment());

    assert!(rendered.contains("Declining and directness:"), "{rendered}");
    assert!(
        rendered.contains("report your assessment and stop there"),
        "{rendered}"
    );
    assert!(rendered.contains("act in the server runtime"), "{rendered}");
    assert!(rendered.contains("Git: interactive flags"), "{rendered}");
    assert!(rendered.contains("Citing sources:"), "{rendered}");

    for run_only in [
        SANDBOX,
        "There is no unrestricted shell.",
        "nobody is watching this run",
        "opens the pull request",
    ] {
        assert!(!rendered.contains(run_only), "{run_only}: {rendered}");
    }
}

/// `workers::task` appends its guidance to this prompt, and the sandbox
/// sentence used to live in both. Each guidance block opens with its own blank
/// line, so a builder that closed with one would push them three apart.
#[tokio::test]
async fn the_run_guidance_joins_the_prompt_without_restating_the_sandbox_sentence() {
    fn entry(title: &str, content: &str) -> LearnedEntryRow {
        LearnedEntryRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            title: title.to_string(),
            content: content.to_string(),
            tags: Vec::new(),
            updated_at: None,
        }
    }

    /// Each block owns the blank line that opens it, so what is already there is
    /// trimmed rather than separated. `workers::task` appends the same way.
    fn push(guidance: &mut String, block: &str) {
        if block.trim().is_empty() {
            return;
        }
        guidance.truncate(guidance.trim_end().len());
        guidance.push_str(block);
    }

    let mut guidance = String::new();
    push(
        &mut guidance,
        "\n\n# Acceptance Criteria\nThe suite passes and clippy is clean.",
    );
    push(
        &mut guidance,
        &render_standing_instructions(&[entry(
            "Migrations",
            "Never edit a migration that has shipped.",
        )]),
    );
    push(
        &mut guidance,
        &render_learned_facts(
            LearnedCategory::RepositoryConvention,
            &[entry("Naming", "Sections are one file each.")],
        ),
    );
    let composed = prompt::task(&task_tools().await, &environment()) + &guidance;

    assert_eq!(composed.matches(SANDBOX).count(), 1, "{composed}");
    assert!(!composed.contains("\n\n\n"), "{composed}");
    assert!(composed.contains("# Standing instructions"), "{composed}");
    assert!(composed.contains("# Repository conventions"), "{composed}");
}

/// A run that hides a failed step behind a summary of what succeeded is the
/// failure mode an unattended surface cannot recover from, so both surfaces say
/// the failure leads.
#[tokio::test]
async fn a_failure_is_reported_ahead_of_anything_that_succeeded() {
    for rendered in [
        prompt::chat(&chat_tools().await, false, &environment()),
        prompt::task(&task_tools().await, &environment()),
    ] {
        assert!(
            rendered.contains("say so in the first sentence, ahead of anything that succeeded"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Never work around a failure so the summary reads as resolved"),
            "{rendered}"
        );
        assert!(
            rendered.contains("If tests failed, say they failed and show the output."),
            "{rendered}"
        );
    }
}

/// `conduct` renders the same rules for a failed call on both surfaces, and its
/// own tests only ever build a chat context, so a surface split there would
/// leave the run nobody is watching with no rule for a call that came back an
/// error.
#[tokio::test]
async fn both_surfaces_are_told_what_to_do_when_a_call_fails() {
    for rendered in [
        prompt::chat(&chat_tools().await, false, &environment()),
        prompt::task(&task_tools().await, &environment()),
    ] {
        for rule in [
            "a denied tool call means the user declined it",
            "failed two or three times, stop and report",
            "three meaningfully different approaches before escalating",
            "never fall back silently to a slower path",
        ] {
            assert!(rendered.contains(rule), "{rule}: {rendered}");
        }
    }
}

/// The rule is about the turn after a tool call, so it renders only where there
/// is a catalog: a chat with no tools never reaches the situation.
#[tokio::test]
async fn a_sign_off_alone_is_not_a_reply() {
    for rendered in [
        prompt::chat(&chat_tools().await, false, &environment()),
        prompt::task(&task_tools().await, &environment()),
    ] {
        assert!(
            rendered.contains("A sign-off alone, such as \"Done.\", is not a reply."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Your first sentence answers the question"),
            "{rendered}"
        );
    }

    let plain = prompt::plain(&environment());
    assert!(!plain.contains("is not a reply"), "{plain}");
}

/// A citation has to point at something a tool returned this turn, which is
/// also why the model's own earlier output cannot stand in for one.
#[tokio::test]
async fn a_citation_names_a_source_the_tools_returned() {
    let rendered = prompt::chat(&chat_tools().await, false, &environment());

    assert!(
        rendered.contains("Name the sources you drew on."),
        "{rendered}"
    );
    assert!(
        rendered.contains(
            "Structured citations keep the source URL, immutable ref or document revision, and \
             observation time."
        ),
        "{rendered}"
    );
    assert!(
        rendered.contains("Incomplete evidence is never a passing result."),
        "{rendered}"
    );
    assert!(
        rendered.contains(
            "Your own earlier replies and Zone's other output are not evidence and not a source \
             of opinion."
        ),
        "{rendered}"
    );
    assert!(
        rendered.contains("Reason a contested question out from what the tools returned"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Cite only identifiers a tool returned in this chat."),
        "{rendered}"
    );
    assert!(
        rendered.contains("bracketed identifier such as [web:a3f21c]"),
        "{rendered}"
    );
}

/// The marker is what the console resolves back to a source, so a reply that
/// spelled the source out as a link would leave nothing to resolve. A chat with
/// no catalog is still handed the pre-turn search block, so it reads the same
/// rule without the sentence about a tool returning anything.
#[tokio::test]
async fn every_chat_surface_cites_by_marker_rather_than_by_link() {
    let rendered = prompt::chat(&chat_tools().await, false, &environment());
    let plain = prompt::plain(&environment());

    for prompt in [&rendered, &plain] {
        assert!(
            prompt.contains("Never write a markdown link or a bare URL for a cited source"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Put the marker after the final punctuation of the sentence or table"),
            "{prompt}"
        );
    }

    assert!(
        plain.contains("Cite only identifiers the sources in this prompt arrived with."),
        "{plain}"
    );
    assert!(!plain.contains("a tool returned in this chat"), "{plain}");
}

/// `tiers` names the outward tier off the tier each tool declares, not off a
/// list of names, so a catalog whose tiers were guessed at says nothing about a
/// send being final. A chat registers six tools that leave the workspace; a run
/// with no authorized writer registers none.
#[tokio::test]
async fn only_a_catalog_that_reaches_outside_the_workspace_is_told_the_send_is_final() {
    let chat = prompt::chat(&chat_tools().await, false, &environment());
    let task = prompt::task(&task_tools().await, &environment());

    assert!(
        chat.contains("- Outward: anything that reaches a person or leaves this workspace"),
        "{chat}"
    );
    assert!(
        chat.contains("Sending it publishes it, and nothing you do afterwards recalls it."),
        "{chat}"
    );
    assert!(
        chat.contains("only when the user asked for it in their own words"),
        "{chat}"
    );
    assert!(!task.contains("- Outward:"), "{task}");
}

/// The section renders only for a catalog holding the tool, so a surface that
/// stopped offering it would lose the rules silently rather than fail here.
#[tokio::test]
async fn both_surfaces_offer_the_question_tool_and_read_its_rules() {
    let chat = chat_tools().await;
    let task = task_tools().await;

    assert!(chat.has(ASK_USER), "a chat catalog must offer {ASK_USER}");
    assert!(task.has(ASK_USER), "a task catalog must offer {ASK_USER}");

    for rendered in [
        prompt::chat(&chat, false, &environment()),
        prompt::task(&task, &environment()),
    ] {
        assert!(rendered.contains("Asking the user:"), "{rendered}");
    }
}

/// The waiting section renders only for a catalog holding `wait_for`, and these
/// are the catalogs the server registers rather than ones a test named by hand,
/// so a registration that stopped happening loses every rule below in silence.
///
/// The absences are the other half of the same claim. A section teaching
/// `wait_for` while a nearer instruction still mandates a poll teaches nothing,
/// so each superseded string is pinned by the half of itself that only the
/// poll wording contained. Only the start_task bullet renders into a prompt at
/// all, and only into the chat one; the other five reach the model as a tool
/// description, a schema or a tool result, and are pinned in `chat_agent_tests`
/// where a turn carries them.
#[tokio::test]
async fn both_surfaces_read_the_waiting_rules_and_no_surviving_poll_instruction() {
    let chat = chat_tools().await;
    let task = task_tools().await;

    assert!(chat.has(WAIT_FOR), "a chat catalog must offer {WAIT_FOR}");
    assert!(task.has(WAIT_FOR), "a task catalog must offer {WAIT_FOR}");

    let chat = prompt::chat(&chat, false, &environment());
    let task = prompt::task(&task, &environment());

    for rendered in [&chat, &task] {
        assert!(rendered.contains(WAIT_FOR), "{rendered}");
        assert!(
            rendered.contains("Waiting for something to finish:"),
            "{rendered}"
        );
        assert!(
            rendered
                .contains("Never call tail_task_log, get_task_run or get_build_status in a loop"),
            "{rendered}"
        );
        assert!(
            rendered.contains("A wait that ends without its event is a timeout, not a result."),
            "{rendered}"
        );
        assert!(
            rendered.contains("A wait is not a way to re-read something that has not changed."),
            "{rendered}"
        );
        for superseded in SUPERSEDED_POLL_WORDING {
            assert!(
                !rendered.to_lowercase().contains(superseded),
                "{superseded:?} survives in {rendered}"
            );
        }
    }

    assert!(chat.contains("this turn's own deadline"), "{chat}");
    assert!(
        task.contains("parks this run and hands its slot back"),
        "{task}"
    );
}

use zone_core::llm::ReasoningEffort;
use zone_server::agent::memory::render;
use zone_server::agent::memory::{
    MEMORY_APPEND, MEMORY_DELETE, MEMORY_LIST, MEMORY_READ, MEMORY_WRITE,
};
use zone_server::db::chats::ChatRow;
use zone_server::db::memory::{
    MemoryCategory, MemoryIndexRow, MemoryRow, PREFERENCES_TITLE, PROFILE_TITLE,
};
use zone_server::services::chat::session::system_prompt;

/// The five tools, in the order the catalog sorts them.
const MEMORY_TOOLS: [&str; 5] = [
    MEMORY_APPEND,
    MEMORY_DELETE,
    MEMORY_LIST,
    MEMORY_READ,
    MEMORY_WRITE,
];

/// The section's heading, as a literal because the constant carrying it is
/// private to that section.
const MEMORY_SECTION: &str = "Memory:";

/// The web-search tail `ws::chat` composes a chat prompt with. Its own wording
/// is not this test's subject; that it is what the block follows is.
const CAPABILITY: &str = "Web search is unavailable this turn.";

const PROFILE: &str = "Ada, an electrical engineer in Wellington.";

const PREFERENCES: &str = "Lead with the answer, then the reasoning.";

const FACT: &str = "Deploy window";

fn remembered(category: MemoryCategory, title: &str, content: &str) -> MemoryRow {
    MemoryRow {
        id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        title: title.to_string(),
        description: None,
        content: content.to_string(),
        category: category.as_str().to_string(),
        version: 1,
        updated_at: None,
    }
}

fn facts() -> Vec<MemoryIndexRow> {
    vec![MemoryIndexRow {
        title: FACT.to_string(),
        description: Some("When deploys go out".to_string()),
        category: MemoryCategory::Fact.as_str().to_string(),
        version: 1,
    }]
}

/// Everything one person has stored, as that reader receives it.
fn block(recall: render::Recall) -> String {
    render::render(
        recall,
        Some(&remembered(MemoryCategory::Profile, PROFILE_TITLE, PROFILE)),
        Some(&remembered(
            MemoryCategory::Preference,
            PREFERENCES_TITLE,
            PREFERENCES,
        )),
        &facts(),
    )
}

/// A chat the agent loop runs in, so `system_prompt` composes the real chat
/// prompt rather than the plain one.
fn chat_row() -> ChatRow {
    ChatRow {
        id: Uuid::new_v4(),
        workspace_id: Some(Uuid::new_v4()),
        title: "Prompt fixture".to_string(),
        model_name: "fixture-model".to_string(),
        archived: None,
        agent_enabled: true,
        agent_sandboxed: false,
        auto_approve: false,
        reasoning_effort: ReasoningEffort::Auto,
        character: None,
        created_at: None,
        updated_at: None,
    }
}

/// The rules are gated on the catalog holding `memory_read`, so the tools and
/// the section are one claim: a registration that stopped happening would take
/// the rules with it and no section test would notice.
#[tokio::test]
async fn a_chat_offers_every_memory_tool_and_reads_the_rules_that_govern_them() {
    let tools = chat_tools().await;

    for tool in MEMORY_TOOLS {
        assert!(tools.has(tool), "a chat catalog must offer {tool}");
    }

    let rendered = prompt::chat(&tools, false, &environment());
    assert!(rendered.contains(MEMORY_SECTION), "{rendered}");
    for tool in MEMORY_TOOLS {
        assert!(rendered.contains(tool), "{tool} is unnamed in {rendered}");
    }
}

/// The other half of the same claim, and the one no catalog test can make:
/// `task_tools()` is built from hand-written constants, so only the assembled
/// prompt shows that nothing registered a memory tool on the run's surface.
#[tokio::test]
async fn a_background_run_is_offered_no_memory_tool_and_no_memory_rules() {
    let tools = task_tools().await;

    for tool in MEMORY_TOOLS {
        assert!(!tools.has(tool), "a task catalog must not offer {tool}");
    }

    let rendered = prompt::task(&tools, &environment());
    assert!(!rendered.contains(MEMORY_SECTION), "{rendered}");
    for tool in MEMORY_TOOLS {
        assert!(!rendered.contains(tool), "{tool} reaches a run: {rendered}");
    }
}

/// What a run is left with once the rules and the tools are gone: the profile
/// and the preferences, and no index of facts it has no way to read.
///
/// The guidance is composed the way `workers::task` composes it -- each block
/// owning the blank line that opens it, memory ahead of the repository block --
/// so the sandbox sentence stays single and nothing opens a third newline.
#[tokio::test]
async fn a_run_reads_the_remembered_profile_and_no_index_it_could_not_act_on() {
    fn push(guidance: &mut String, block: &str) {
        let block = block.trim_end();
        if block.is_empty() {
            return;
        }
        guidance.truncate(guidance.trim_end().len());
        guidance.push_str(block);
    }

    let memory = block(render::Recall::Passive);
    assert!(
        !memory.is_empty(),
        "a run with a stored profile renders one"
    );

    let mut guidance = String::new();
    push(&mut guidance, &memory);
    push(
        &mut guidance,
        "\n\n# Repository Instructions\nRun the suite before opening the pull request.",
    );
    let composed = prompt::task(&task_tools().await, &environment()) + &guidance;

    assert!(
        composed.contains(MemoryCategory::Profile.heading()),
        "{composed}"
    );
    assert!(composed.contains(PROFILE), "{composed}");
    assert!(
        composed.contains(MemoryCategory::Preference.heading()),
        "{composed}"
    );
    assert!(
        !composed.contains(MemoryCategory::Fact.heading()),
        "a run was handed an index it has no tool to read: {composed}"
    );
    assert!(!composed.contains(FACT), "{composed}");
    assert!(!composed.contains(MEMORY_SECTION), "{composed}");
    assert!(
        composed.find(MemoryCategory::Profile.heading()) < composed.find("# Repository"),
        "{composed}"
    );
    assert_eq!(composed.matches(SANDBOX).count(), 1, "{composed}");
    assert!(!composed.contains("\n\n\n"), "{composed}");
}

/// A chat with its agent off is still a chat with the person, so it reads the
/// profile and the preferences — and is given no fact index, because it holds
/// no `memory_read` to open one with.
///
/// This is the plain prompt, not the agent one: the block has to survive the
/// branch that picks between them, which is where it used to be dropped.
#[tokio::test]
async fn a_chat_with_the_agent_off_reads_the_profile_and_no_index() {
    let memory = block(render::Recall::Passive);
    assert!(
        !memory.is_empty(),
        "a stored profile renders for any reader"
    );

    let composed = system_prompt(
        &chat_row(),
        &chat_tools().await,
        false,
        CAPABILITY,
        &environment(),
        &memory,
        "",
    );

    let tail = composed.find(CAPABILITY).expect("the capability tail");
    for heading in [
        MemoryCategory::Profile.heading(),
        MemoryCategory::Preference.heading(),
    ] {
        let at = composed
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} is missing from {composed}"));
        assert!(at > tail, "{heading} precedes the tail: {composed}");
    }
    assert!(composed.contains(PROFILE), "{composed}");
    assert!(composed.contains(PREFERENCES), "{composed}");
    assert!(
        !composed.contains(MemoryCategory::Fact.heading()),
        "a chat with no memory tool was handed an index: {composed}"
    );
    assert!(!composed.contains(FACT), "{composed}");
    // The rules section belongs to the agent prompt, which this chat is not.
    assert!(!composed.contains(MEMORY_SECTION), "{composed}");
    assert_eq!(composed.matches(SANDBOX).count(), 0, "{composed}");
    assert!(!composed.contains("\n\n\n"), "{composed}");
}

/// The same block on the chat surface, composed by the function that composes
/// it in production: after the capability tail, index and all.
///
/// The sandbox sentence is counted here too, as an absence. A chat prompt has
/// never carried it, and appending a run's block would be one way to acquire
/// one.
#[tokio::test]
async fn a_chat_reads_the_remembered_block_after_the_capability_tail() {
    let memory = block(render::Recall::Indexed);
    let composed = system_prompt(
        &chat_row(),
        &chat_tools().await,
        true,
        CAPABILITY,
        &environment(),
        &memory,
        "",
    );

    let tail = composed.find(CAPABILITY).expect("the capability tail");
    for heading in [
        MemoryCategory::Profile.heading(),
        MemoryCategory::Preference.heading(),
        MemoryCategory::Fact.heading(),
    ] {
        let at = composed
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} is missing from {composed}"));
        assert!(at > tail, "{heading} precedes the tail: {composed}");
    }
    assert!(composed.contains(PROFILE), "{composed}");
    assert!(composed.contains(PREFERENCES), "{composed}");
    assert!(composed.contains(&format!("- {FACT} — ")), "{composed}");
    assert!(composed.contains(MEMORY_SECTION), "{composed}");
    assert_eq!(composed.matches(SANDBOX).count(), 0, "{composed}");
    assert!(!composed.contains("\n\n\n"), "{composed}");
}
