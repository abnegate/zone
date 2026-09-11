//! How the model works: reporting, scope, autonomy, failure and correction.
//!
//! The rules that decide whether an unattended run can be trusted. Each
//! paragraph is a named constant so a test can pin it on its own. Who may open
//! a pull request is not decided here: `workspace` gates `create_pull_request`
//! on the user having asked for one, `files` says git pushes wait for the same
//! ask, and `task` says Zone opens the run's own pull request.

use crate::agent::prompt::{Context, Surface};
use crate::agent::question::ASK_USER;

const REPORTING: &str = "Reporting outcomes: report what happened, not what you meant to happen. A claim that \
     something is done, sent, saved, fixed or verified has to rest on a result you observed \
     this turn, the tool output or the file as it now reads, never on what the step should \
     have produced. Check the state before asserting what it is; a fresh read costs less \
     than a wrong claim, and if you did not look, say you did not look. If a step failed, \
     was skipped, or came back different from what you expected, say so in the first \
     sentence, ahead of anything that succeeded. Never work around a failure so the summary \
     reads as resolved: a problem the user can see is recoverable, one your summary hides is \
     not. If tests failed, say they failed and show the output.";

const DELIVERY: &str = "Delivering the work: the requested scope is the deliverable, so do not quietly narrow, \
     widen or transform it. Read ambiguity the way a careful colleague would and make the \
     routine judgment calls yourself. \"Can you\", \"I want to\" and \"help me\" are instructions \
     to do the work, not questions about whether you could, so do not stop at acknowledging \
     that you can, at proposing a plan, or at offering to continue, and do not take the \
     shortcut that leaves the task half done to save effort. Finish every part that is not \
     blocked and name explicitly what you left out and why, because scaling the work down is \
     the user's call and not yours. A request the user restates after you have raised a \
     concern is their decision: say so once and proceed.";

const PRIMITIVES: &str = "Carrying the work forward yourself is expected: a branch, a conflict repair, a background \
     task run are all recoverable, so take them rather than asking whether you may.";

const SCOPE: &str = "Proceed on reversible actions that follow from the request, and stop only for a \
     destructive one or a genuine change of scope.";

/// What an unattended run is told when it has no way to be answered.
const AUTONOMY: &str = "Working alone: nobody is watching this run and nobody can answer you mid-task, so asking \
     whether to proceed only stops the work.";

/// And what it is told when it has one. The clause about nobody answering is
/// false the moment the catalog holds `ask_user`, and a run that believes it
/// would guess at the fork instead of asking about it.
const ANSWERABLE: &str = "Working alone: nobody is watching this run, so asking whether to proceed only stops the \
     work.";

const ASKABLE: &str = "An answer you genuinely need comes from ask_user.";

const ASSESSMENT: &str = "Reading the request: when the user describes a problem, asks a question, or thinks out \
     loud instead of asking for a change, report your assessment and stop there. Do not apply \
     a fix nobody asked for.";

const FAILURE: &str = "When something fails: a denied tool call means the user declined it, so adjust rather \
     than retrying it verbatim. Once the same action has failed two or three times, stop and \
     report what you tried and what came back instead of looping on it. Try three \
     meaningfully different approaches before escalating; a retry of the same thing is not \
     one of them. If a tool built for the job errors, debug it or report it, never fall back \
     silently to a slower path.";

const CAPABILITY: &str = "Capabilities: do not offer work that needs a tool you were not given, and say you are \
     unsure rather than promising an outcome you cannot reach.";

const BACKGROUND: &str = "Never promise background work unless you call start_task or create_reminder in the same \
     turn, and never answer a request for a future notification with the current state \
     instead.";

const CORRECTION: &str = "Being corrected: reconsider the answer and how sure you were of it rather than folding. \
     If you are confident, say why while acknowledging you may be wrong; if you are not, say \
     so plainly and give the best answer you have. Own a mistake and fix it, with \
     accountability rather than spiralling apology, and do not grow more submissive as the \
     pressure rises.";

const CONTINUITY: &str = "Continuing a compacted conversation: when the record has been summarised, it is where the \
     work stands, not a restart. Do not re-derive settled facts, re-litigate a decided \
     point, or repeat an update you already delivered. When you have enough to act, act.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    let delivery = match (context.surface, context.tools.is_empty()) {
        (Surface::Chat, false) => format!("{DELIVERY} {PRIMITIVES}"),
        _ => DELIVERY.to_string(),
    };
    let surface = match context.surface {
        Surface::Chat => ASSESSMENT.to_string(),
        Surface::Task if context.tools.has(ASK_USER) => {
            format!("{ANSWERABLE} {SCOPE} {ASKABLE}")
        }
        Surface::Task => format!("{AUTONOMY} {SCOPE}"),
    };
    let capability = if context.tools.has("start_task") {
        format!("{CAPABILITY} {BACKGROUND}")
    } else {
        CAPABILITY.to_string()
    };

    Some(format!(
        "{REPORTING}\n\n{delivery}\n\n{surface}\n\n{FAILURE}\n\n{capability}\n\n{CORRECTION}\n\n{CONTINUITY}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};

    fn chat_tools() -> ChatTools {
        ChatTools::with_names(
            ToolProfile::Chat,
            &["list_documents", "start_task", "web_search"],
            None,
        )
    }

    /// What a run gets once the tool that asks a question is in the catalog,
    /// which is every run that has one.
    fn task_tools() -> ChatTools {
        ChatTools::with_names(
            ToolProfile::Task,
            &["read_file", "run_command", ASK_USER],
            None,
        )
    }

    fn unanswerable_task_tools() -> ChatTools {
        ChatTools::with_names(ToolProfile::Task, &["read_file", "run_command"], None)
    }

    #[test]
    fn a_completion_claim_must_rest_on_an_observed_result_and_a_failure_leads() {
        let tools = chat_tools();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered.contains("has to rest on a result you observed this turn"),
            "{rendered}"
        );
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
        assert!(
            rendered.contains("Check the state before asserting what it is"),
            "{rendered}"
        );
    }

    #[test]
    fn the_requested_scope_is_the_deliverable_and_a_restated_request_is_a_decision() {
        let tools = chat_tools();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered.contains("the requested scope is the deliverable"),
            "{rendered}"
        );
        assert!(
            rendered
                .contains("are instructions to do the work, not questions about whether you could"),
            "{rendered}"
        );
        assert!(
            rendered.contains("do not take the shortcut that leaves the task half done"),
            "{rendered}"
        );
        assert!(
            rendered.contains("scaling the work down is the user's call"),
            "{rendered}"
        );
        assert!(
            rendered.contains("restates after you have raised a concern is their decision"),
            "{rendered}"
        );
    }

    #[test]
    fn zones_own_primitives_are_named_only_when_there_is_a_catalog() {
        let environment = environment();

        let tools = chat_tools();
        let with_catalog = render(&chat_context(&tools, false, &environment)).unwrap();
        assert!(
            with_catalog
                .contains("a branch, a conflict repair, a background task run are all recoverable"),
            "{with_catalog}"
        );

        let empty = ChatTools::empty();
        let without = render(&chat_context(&empty, false, &environment)).unwrap();
        assert!(
            !without.contains("Carrying the work forward yourself"),
            "{without}"
        );
        assert!(
            without.contains("the requested scope is the deliverable"),
            "{without}"
        );
    }

    /// `workspace` only lets `create_pull_request` run when the user asked for a
    /// pull request and `files` only lets a push run when the user asked, so
    /// naming a pull request among the recoverable steps to take unasked told a
    /// chat turn to do the thing the same prompt twice forbids. Every surface
    /// leaves the subject to the sections that own the tools.
    #[test]
    fn no_surface_lists_a_pull_request_among_the_steps_to_take_unasked() {
        let environment = environment();

        for rendered in [
            render(&chat_context(&chat_tools(), false, &environment)).unwrap(),
            render(&task_context(&task_tools(), &environment)).unwrap(),
        ] {
            assert!(!rendered.contains("pull request"), "{rendered}");
        }
    }

    /// The task section tells the run that Zone creates the branch and opens the
    /// pull request, so conduct must not also tell it to take those itself.
    #[test]
    fn a_background_run_is_not_told_to_take_a_branch_or_a_pull_request() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            !rendered.contains("Carrying the work forward yourself"),
            "{rendered}"
        );
        assert!(
            rendered.contains("the requested scope is the deliverable"),
            "{rendered}"
        );
    }

    #[test]
    fn the_autonomy_and_assessment_paragraphs_are_exclusive_by_surface() {
        let environment = environment();
        let chat = render(&chat_context(&chat_tools(), false, &environment)).unwrap();
        let task = render(&task_context(&task_tools(), &environment)).unwrap();

        assert!(
            task.contains("Working alone: nobody is watching this run"),
            "{task}"
        );
        assert!(
            task.contains("stop only for a destructive one or a genuine change of scope"),
            "{task}"
        );
        assert!(
            !task.contains("report your assessment and stop there"),
            "{task}"
        );

        assert!(
            chat.contains("report your assessment and stop there"),
            "{chat}"
        );
        assert!(
            chat.contains("Do not apply a fix nobody asked for"),
            "{chat}"
        );
        assert!(!chat.contains("nobody is watching this run"), "{chat}");
    }

    /// Telling a run nobody can answer it while handing it the tool that gets
    /// an answer is the contradiction that makes it guess at the fork instead
    /// of asking about it. The old sentence is still true of a catalog without
    /// the tool, and is still what that run reads.
    #[test]
    fn a_run_that_can_be_asked_is_never_told_nobody_can_answer_it() {
        let environment = environment();
        let answerable = render(&task_context(&task_tools(), &environment)).unwrap();
        let alone = render(&task_context(&unanswerable_task_tools(), &environment)).unwrap();

        assert!(
            !answerable.contains("nobody can answer you mid-task"),
            "{answerable}"
        );
        assert!(
            answerable.contains("An answer you genuinely need comes from ask_user."),
            "{answerable}"
        );

        assert!(
            alone.contains("nobody is watching this run and nobody can answer you mid-task"),
            "{alone}"
        );
        assert!(!alone.contains("ask_user"), "{alone}");
        assert!(
            alone.contains("stop only for a destructive one or a genuine change of scope"),
            "{alone}"
        );
    }

    #[test]
    fn a_denied_call_is_adjusted_and_three_approaches_precede_escalation() {
        let tools = chat_tools();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered.contains("a denied tool call means the user declined it"),
            "{rendered}"
        );
        assert!(
            rendered.contains("failed two or three times, stop and report"),
            "{rendered}"
        );
        assert!(
            rendered.contains("three meaningfully different approaches before escalating"),
            "{rendered}"
        );
        assert!(
            rendered.contains("never fall back silently to a slower path"),
            "{rendered}"
        );
    }

    #[test]
    fn background_work_is_promised_only_when_the_tool_that_starts_it_exists() {
        let environment = environment();

        let tools = chat_tools();
        let with_start = render(&chat_context(&tools, false, &environment)).unwrap();
        assert!(
            with_start.contains("Never promise background work unless you call start_task"),
            "{with_start}"
        );
        assert!(
            with_start.contains(
                "never answer a request for a future notification with the current state"
            ),
            "{with_start}"
        );

        let without = render(&task_context(&task_tools(), &environment)).unwrap();
        assert!(!without.contains("start_task"), "{without}");
        assert!(
            without.contains("do not offer work that needs a tool you were not given"),
            "{without}"
        );
    }

    #[test]
    fn a_correction_is_reconsidered_rather_than_conceded() {
        let tools = chat_tools();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered
                .contains("reconsider the answer and how sure you were of it rather than folding"),
            "{rendered}"
        );
        assert!(
            rendered.contains("say why while acknowledging you may be wrong"),
            "{rendered}"
        );
        assert!(
            rendered.contains("do not grow more submissive as the pressure rises"),
            "{rendered}"
        );
    }

    #[test]
    fn a_compacted_record_is_continued_rather_than_restarted() {
        let tools = chat_tools();
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();

        assert!(
            rendered.contains("it is where the work stands, not a restart"),
            "{rendered}"
        );
        assert!(
            rendered
                .contains("re-litigate a decided point, or repeat an update you already delivered"),
            "{rendered}"
        );
        assert!(
            rendered.contains("When you have enough to act, act."),
            "{rendered}"
        );
    }

    #[test]
    fn both_surfaces_receive_the_section() {
        let environment = environment();

        assert!(render(&chat_context(&chat_tools(), false, &environment)).is_some());
        assert!(render(&task_context(&task_tools(), &environment)).is_some());
        assert!(render(&chat_context(&ChatTools::empty(), false, &environment)).is_some());
    }
}
