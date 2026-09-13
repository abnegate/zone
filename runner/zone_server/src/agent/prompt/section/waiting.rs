//! When to wait for something outside the loop, and when a wait is a poll
//! wearing another name.
//!
//! Every status read Zone offers answers instantly, so a model watching one of
//! them spends a round per glance and learns nothing between them. `wait_for`
//! is the other half of that trade: it suspends the loop on the event itself
//! and costs no iteration. The two ways to get it wrong are opposites — asking
//! for far more time than the work needs, and asking for so little that the
//! wait is a poll again — so both are stated. What a wait costs differs by
//! surface, so the price is stated per surface. Only a catalog holding the tool
//! renders this.

use crate::agent::prompt::{Context, Surface};
use crate::agent::wait::{
    DEFAULT_WAIT_SECS, KIND_CHECK, KIND_JOB, KIND_TASK_RUN, MAX_WAIT_SECS, MAX_WAITS_PER_ATTEMPT,
    MIN_WAIT_SECS, WAIT_FOR,
};
use std::sync::LazyLock;

const HEADING: &str = "Waiting for something to finish:";

/// The three reads this section sends a model away from. Their names live in
/// `Tool::name()` arms on tools a prompt section cannot construct — a
/// workspace action needs a scope and an integration needs a source — so they
/// are named here and the rule is built from them rather than spelling them
/// out mid-sentence.
const TAIL_TASK_LOG: &str = "tail_task_log";
const GET_TASK_RUN: &str = "get_task_run";
const GET_BUILD_STATUS: &str = "get_build_status";

const BACKGROUND: &str = "- Send a long command to the background rather than holding the turn open on it, then \
     wait for the job instead of watching it.";

static NO_POLLING: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- Never call {TAIL_TASK_LOG}, {GET_TASK_RUN} or {GET_BUILD_STATUS} in a loop to learn \
         whether something finished. Each read spends a round and reports only the instant it \
         ran in; {WAIT_FOR} returns when the thing itself happens."
    )
});

/// The floor and the default are quoted because a model given no sense of the
/// window asks for one end of it, and both ends are wrong for most subjects.
static SIZE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- Size a wait to what you are waiting for: {MIN_WAIT_SECS}s is the floor, \
         {DEFAULT_WAIT_SECS}s covers a build or a test run getting under way, and something you \
         already know is slower is worth the time it takes."
    )
});

const NOT_SUCCESS: &str = "- A wait that ends without its event is a timeout, not a result. Nothing having happened \
     is not the same as it having gone well, so report which of the two you have.";

static EXCESSIVE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- Do not ask for more time than the work could plausibly take. {MAX_WAIT_SECS}s is the \
         ceiling, and spending it on something that finishes in a minute leaves the user \
         watching nothing happen."
    )
});

/// The loop's no-progress detector is rebuilt on the far side of a wait, so a
/// stalled read loop laundered through `wait_for` is invisible to it. This
/// sentence is the whole mitigation.
const UNCHANGED: &str = "- A wait is not a way to re-read something that has not changed. Wait for an event you \
     have a reason to expect; when the last read added nothing, act on what you already have \
     instead of waiting to read it again.";

static TASK_PARK: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- A wait parks this run and hands its slot back, so the waiting itself costs you \
         nothing — but queueing for the slot again does, and you get {MAX_WAITS_PER_ATTEMPT} \
         waits in an attempt. One wait long enough to settle beats several that do not."
    )
});

static TASK_KINDS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- kind={KIND_TASK_RUN} is not available from inside a run. Wait on kind={KIND_JOB} or \
         kind={KIND_CHECK}, or finish and leave the rest to whoever started this run."
    )
});

static CHAT_DEADLINE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- A wait runs inside this turn's own deadline, so one longer than the turn has left is \
         cut short and comes back as a timeout. {MAX_WAIT_SECS}s is the whole turn, not an \
         extension of it."
    )
});

static CHAT_KINDS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "- kind={KIND_TASK_RUN} is available here, so a run you started is something you can \
         wait on directly."
    )
});

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if !context.tools.has(WAIT_FOR) {
        return None;
    }
    let rules = match context.surface {
        Surface::Chat => vec![
            BACKGROUND,
            NO_POLLING.as_str(),
            SIZE.as_str(),
            NOT_SUCCESS,
            EXCESSIVE.as_str(),
            UNCHANGED,
            CHAT_DEADLINE.as_str(),
            CHAT_KINDS.as_str(),
        ],
        Surface::Task => vec![
            BACKGROUND,
            NO_POLLING.as_str(),
            SIZE.as_str(),
            NOT_SUCCESS,
            EXCESSIVE.as_str(),
            UNCHANGED,
            TASK_PARK.as_str(),
            TASK_KINDS.as_str(),
        ],
    };
    Some(format!("{HEADING}\n{}", rules.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::{START_TASK_DESCRIPTION, TAIL_TASK_LOG_DESCRIPTION};
    use crate::agent::prompt;
    use crate::agent::prompt::section::tiers::FINISH_FIRST;
    use crate::agent::prompt::section::workspace::START_TASK;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};
    use crate::db::actions::RUNNER_STARTED;

    fn chat() -> String {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file", WAIT_FOR], None);
        let environment = environment();
        render(&chat_context(&tools, false, &environment)).unwrap()
    }

    fn task() -> String {
        let tools = ChatTools::with_names(ToolProfile::Task, &["read_file", WAIT_FOR], None);
        let environment = environment();
        render(&task_context(&tools, &environment)).unwrap()
    }

    /// The catalog without the tool is the one that most needs these rules and
    /// least may have them: it holds the reads a model would otherwise loop on,
    /// and being sent to wait_for instead would name a tool it cannot call.
    #[test]
    fn the_rules_arrive_with_the_tool_and_not_before() {
        let environment = environment();
        let without = ChatTools::with_names(
            ToolProfile::Chat,
            &["read_file", GET_TASK_RUN, TAIL_TASK_LOG],
            None,
        );
        let with = ChatTools::with_names(
            ToolProfile::Chat,
            &["read_file", GET_TASK_RUN, TAIL_TASK_LOG, WAIT_FOR],
            None,
        );

        assert!(render(&chat_context(&without, false, &environment)).is_none());
        assert!(render(&task_context(&without, &environment)).is_none());
        assert!(render(&chat_context(&with, false, &environment)).is_some());
        assert!(render(&task_context(&with, &environment)).is_some());
    }

    /// Every rule below is asserted whole rather than by a fragment of itself.
    /// A fragment survives the rule being inverted around it, and the constant
    /// is the spec line, so the test reads the constant.
    #[test]
    fn a_long_command_is_backgrounded_rather_than_blocked_on() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(BACKGROUND), "{rendered}");
        }
    }

    #[test]
    fn the_three_status_reads_are_never_called_in_a_loop() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(NO_POLLING.as_str()), "{rendered}");
        }
    }

    /// The one rule built from tool names rather than from numbers, so the
    /// sentence a model reads is pinned here as a literal. A rule asserted
    /// through its own constant cannot notice a name interpolated wrong, and a
    /// name a model cannot call teaches it nothing.
    #[test]
    fn the_rule_names_the_three_reads_and_the_tool_that_replaces_them() {
        assert_eq!(
            *NO_POLLING,
            "- Never call tail_task_log, get_task_run or get_build_status in a loop to learn \
             whether something finished. Each read spends a round and reports only the instant \
             it ran in; wait_for returns when the thing itself happens."
        );
    }

    #[test]
    fn a_wait_is_sized_to_what_it_is_waiting_for() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(SIZE.as_str()), "{rendered}");
        }
    }

    #[test]
    fn a_wait_that_ends_without_its_event_is_never_a_pass() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(NOT_SUCCESS), "{rendered}");
        }
    }

    #[test]
    fn an_excessive_timeout_is_asked_for_on_neither_surface() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(EXCESSIVE.as_str()), "{rendered}");
        }
    }

    #[test]
    fn a_wait_is_not_a_way_to_re_read_unchanged_evidence() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(UNCHANGED), "{rendered}");
        }
    }

    /// A chat wait holds a turn somebody is watching; a task wait gives its
    /// admission slot back and queues for it again. Neither cost describes the
    /// other, so neither surface reads the other's sentence.
    #[test]
    fn each_surface_says_what_a_wait_costs_it() {
        let chat = chat();
        let task = task();

        assert!(task.contains(TASK_PARK.as_str()), "{task}");
        assert!(!chat.contains("parks this run"), "{chat}");

        assert!(chat.contains(CHAT_DEADLINE.as_str()), "{chat}");
        assert!(!task.contains("this turn's own deadline"), "{task}");
    }

    #[test]
    fn each_surface_says_which_kinds_it_may_wait_on() {
        let chat = chat();
        let task = task();

        assert!(task.contains(TASK_KINDS.as_str()), "{task}");
        assert!(chat.contains(CHAT_KINDS.as_str()), "{chat}");
        assert!(!task.contains("is available here"), "{task}");
        assert!(
            !chat.contains("is not available from inside a run"),
            "{chat}"
        );
    }

    /// The window and the park limit are enforced by the tool, and a prompt
    /// carrying its own copy of either goes stale the first time one moves.
    #[test]
    fn the_window_the_prompt_quotes_is_the_window_the_tool_enforces() {
        let task = task();

        for quoted in [
            format!("{MIN_WAIT_SECS}s is the floor"),
            format!("{DEFAULT_WAIT_SECS}s covers a build"),
            format!("{MAX_WAIT_SECS}s is the ceiling"),
            format!("{MAX_WAITS_PER_ATTEMPT} waits in an attempt"),
        ] {
            assert!(task.contains(&quoted), "{quoted} missing from {task}");
        }
    }

    /// `workspace` owns what start_task is and what proves its runner finished,
    /// `task` owns when a passing check stops being re-run, and `files` owns
    /// bounding a long log. All three sit one sentence from what this section
    /// says, which is exactly how a prompt starts saying it twice.
    #[test]
    fn the_wording_workspace_task_and_files_own_is_not_repeated_here() {
        for rendered in [chat(), task()] {
            assert!(
                !rendered.contains("Do not claim the runner finished"),
                "{rendered}"
            );
            assert!(!rendered.contains("It is not create_task"), "{rendered}");
            assert!(
                !rendered.contains("stop re-running them unless something changed"),
                "{rendered}"
            );
            assert!(!rendered.contains("max_output_chars"), "{rendered}");
        }
    }

    /// Those absences are literals, so either owner rewording turns them all
    /// vacuous and the duplication they exist to catch walks back in. Counting
    /// the owning constants over the assembled prompt is what still fails:
    /// whatever they say, the model reads each of them once.
    #[test]
    fn each_rule_another_section_owns_reaches_the_prompt_exactly_once() {
        let tools = ChatTools::with_names(
            ToolProfile::Chat,
            &[
                "run_shell",
                "start_task",
                "create_task",
                GET_TASK_RUN,
                TAIL_TASK_LOG,
                WAIT_FOR,
            ],
            None,
        );
        let environment = environment();
        let rendered = prompt::chat(&tools, false, &environment);

        for owned in [START_TASK, FINISH_FIRST] {
            assert_eq!(rendered.matches(owned).count(), 1, "{owned}");
        }
    }

    /// The four strings a model reads at the moment it starts a runner. A
    /// section teaching wait_for while those still mandate a poll teaches
    /// nothing, because the nearest instruction is the one that wins.
    #[test]
    fn nothing_that_starts_a_runner_still_asks_for_a_poll() {
        for string in [
            START_TASK,
            START_TASK_DESCRIPTION,
            TAIL_TASK_LOG_DESCRIPTION,
            RUNNER_STARTED,
        ] {
            assert!(string.contains(WAIT_FOR), "{string}");
            assert!(!string.to_lowercase().contains("poll"), "{string}");
        }
    }
}
