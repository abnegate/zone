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
use zone_server::agent::{ASK_USER, ChatTools, Environment, WorkspaceScope};
use zone_server::db::knowledge::{
    LearnedCategory, LearnedEntryRow, render_learned_facts, render_standing_instructions,
};

/// The sentence `workers::task` used to append after the prompt it now owns.
const SANDBOX: &str = "You are completing a background coding task.";

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
