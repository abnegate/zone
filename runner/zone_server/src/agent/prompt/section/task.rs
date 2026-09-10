//! What a background run may assume about its sandbox, its checks and its PR.
//!
//! Only the task surface renders this. A chat turn has a person who can answer
//! and who runs git themselves; an unattended run has neither, so the frame it
//! works in has to be stated rather than inferred.

use crate::agent::prompt::{Context, Surface};

const RUN: &str = "You are completing a background coding task. Stay inside the sandboxed working directory: \
     it is the only place your work is collected from when the run ends.";

const TESTING: &str = "Testing: run the checks this change actually calls for, and once they pass stop \
     re-running them unless something changed. Do not write a test that only mirrors the \
     implementation back at itself, and do not add tests for a reversible, low-impact change. \
     A bug fix is the exception, and ships with a regression test that fails without the fix \
     and passes with it.";

const DELIVERY: &str = "Zone closes out the run for you: it creates the branch, commits whatever the checkout \
     holds and opens the pull request the user reviews. So do not commit, push, tag, \
     force-push or rewrite history yourself, and leave the checkout on the branch Zone put \
     it on: a run that moved it or rewrote its history cannot be published.";

const REPORT: &str = "Your closing message is the report. Zone puts it in the commit and in the pull request, \
     where it is read by someone who was never in this run and has only the diff to go on. \
     Lead with the outcome, then what changed and why, how you checked it, and any risk you \
     are leaving behind. One line covers routine checks. Leave out approaches you abandoned, \
     and say what you did not finish rather than letting the diff say it for you.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    match context.surface {
        Surface::Chat => None,
        Surface::Task => Some(format!("{RUN}\n\n{TESTING}\n\n{DELIVERY}\n\n{REPORT}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};

    fn task_tools() -> ChatTools {
        ChatTools::with_names(ToolProfile::Task, &["read_file", "run_command"], None)
    }

    #[test]
    fn only_a_background_run_receives_the_section() {
        let environment = environment();
        let tools = task_tools();

        assert!(render(&task_context(&tools, &environment)).is_some());
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
        assert!(render(&chat_context(&ChatTools::empty(), false, &environment)).is_none());
    }

    #[test]
    fn the_run_is_framed_by_the_working_directory_its_output_is_collected_from() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            rendered.starts_with("You are completing a background coding task."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Stay inside the sandboxed working directory"),
            "{rendered}"
        );
        assert!(
            rendered.contains("the only place your work is collected from"),
            "{rendered}"
        );
    }

    #[test]
    fn testing_is_proportionate_to_the_change_and_stops_once_it_is_green() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            rendered.contains("run the checks this change actually calls for"),
            "{rendered}"
        );
        assert!(
            rendered.contains("stop re-running them unless something changed"),
            "{rendered}"
        );
        assert!(
            rendered.contains("a test that only mirrors the implementation back at itself"),
            "{rendered}"
        );
        assert!(
            rendered.contains("do not add tests for a reversible, low-impact change"),
            "{rendered}"
        );
    }

    #[test]
    fn a_bug_fix_still_ships_with_a_regression_test() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            rendered.contains("a regression test that fails without the fix and passes with it"),
            "{rendered}"
        );
    }

    /// The chat surface tells the model to commit only when asked; here Zone
    /// does the committing, so the run is told to keep its hands off git rather
    /// than told the same rule twice.
    #[test]
    fn zone_opens_the_pull_request_so_the_run_never_writes_history_itself() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            rendered.contains(
                "it creates the branch, commits whatever the checkout holds and opens the pull \
                 request the user reviews"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("do not commit, push, tag, force-push or rewrite history yourself"),
            "{rendered}"
        );
        assert!(
            rendered.contains("leave the checkout on the branch Zone put it on"),
            "{rendered}"
        );
        assert!(
            rendered.contains("a run that moved it or rewrote its history cannot be published"),
            "{rendered}"
        );
    }

    /// The run's own closing message is what a reviewer reads, so the prompt
    /// has to say so: nothing else tells the model its last paragraph is going
    /// to be quoted somewhere it cannot answer questions about it.
    #[test]
    fn the_closing_message_is_written_for_the_reviewer_it_is_quoted_to() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(rendered.contains(REPORT), "{rendered}");
        assert!(
            rendered.contains("Zone puts it in the commit and in the pull request"),
            "{rendered}"
        );
        assert!(
            rendered.contains("read by someone who was never in this run"),
            "{rendered}"
        );
        assert!(rendered.contains("Lead with the outcome"), "{rendered}");
        assert!(
            rendered.contains("how you checked it, and any risk you are leaving behind"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Leave out approaches you abandoned"),
            "{rendered}"
        );
    }

    /// Wave 3 shares the task budget four ways, so this section's own share is
    /// pinned rather than left to the assembled total to discover.
    #[test]
    fn the_section_stays_inside_its_share_of_the_task_budget() {
        let tools = task_tools();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();

        assert!(
            rendered.len() + "\n\n".len() <= 1_400,
            "the task section adds {} chars",
            rendered.len() + 2
        );
    }
}
