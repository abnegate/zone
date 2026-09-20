//! When a decision is put to the user, and when it is made without them.
//!
//! `ask_user` is the only question this model can ask and have answered, which
//! is what makes it worth spending bytes on in both directions: a run that
//! guesses at a fork it cannot see past is wrong for the rest of the turn, and
//! a run that asks what it could have read has stopped for nothing. A question
//! costs a task run more than it costs a chat, so the cost is stated per
//! surface. Only a catalog holding the tool renders this.

use crate::agent::prompt::{Context, Surface};
use crate::agent::question::ASK_USER;

const HEADING: &str = "Asking the user:";

const RESERVE: &str = "- Reserve ask_user for a decision that changes what you do next; reading or testing \
     settles the rest.";

const CONVERSATION: &str = "- Check the conversation first: it often answers.";

const NARROWED: &str =
    "- A detailed request has narrowed it: state the assumption inline rather than ask.";

const COUNT: &str = "- One question is the shape to aim for, three the ceiling.";

/// A planner chat is an interview: cards carry a topic's related questions
/// together, and the ceiling is the card's.
const PLANNER_COUNT: &str =
    "- Group the related questions of one topic on a card, two to four at a time.";

const LAST: &str =
    "- Your turn ends on the call: finish everything the answer does not block first.";

const CARD: &str = "- The card is the consent: do not also ask in prose or restate the options.";

/// Only a task run goes ahead on a default, so only a task run can be wrong to;
/// in a chat the user's next message answers either way.
const REQUIRED: &str = "- Mark a question required only when the default would be wrong.";

const TASK_WAIT: &str = "- The run parks here: an optional question goes ahead on the first option after about \
     thirty seconds, a required one waits, and a timed-out run is not retried, so its work \
     is lost.";

const CHAT_WAIT: &str = "- The answer comes back as the user's next message.";

/// An unattended run has nobody to answer it: the wait is the same thirty
/// seconds for every card, required or not, and then the first option stands.
const UNATTENDED_WAIT: &str = "- Nobody reads this run while it runs: any card you ask goes ahead on its first option \
     after about thirty seconds, required or not, so decide for yourself, put the decision and \
     the assumption behind it in your report, and ask only when no reading could settle it.";

/// Pairs with `TASK_WAIT`: the default is the one option nobody chose, and a
/// report that hands it to the user as their decision is what this prevents.
const ELAPSED: &str =
    "- The wait running out is not an answer: name the option you took as your own.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if !context.tools.has(ASK_USER) {
        return None;
    }
    let rules = match context.surface {
        Surface::Chat if context.tools.has(crate::agent::planner::FINALIZE_PROJECT) => vec![
            RESERVE,
            CONVERSATION,
            NARROWED,
            PLANNER_COUNT,
            LAST,
            CARD,
            CHAT_WAIT,
        ],
        Surface::Chat => vec![
            RESERVE,
            CONVERSATION,
            NARROWED,
            COUNT,
            LAST,
            CARD,
            CHAT_WAIT,
        ],
        Surface::Task if context.tools.is_unattended() => vec![
            RESERVE,
            CONVERSATION,
            NARROWED,
            COUNT,
            LAST,
            CARD,
            UNATTENDED_WAIT,
            ELAPSED,
        ],
        Surface::Task => vec![
            RESERVE,
            CONVERSATION,
            NARROWED,
            COUNT,
            LAST,
            CARD,
            REQUIRED,
            TASK_WAIT,
            ELAPSED,
        ],
    };
    Some(format!("{HEADING}\n{}", rules.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt;
    use crate::agent::prompt::section::boundary::BOUNDARY;
    use crate::agent::prompt::section::tiers::FINISH_FIRST;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::{ChatTools, ToolProfile};

    fn chat() -> String {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file", ASK_USER], None);
        let environment = environment();
        render(&chat_context(&tools, false, &environment)).unwrap()
    }

    fn task() -> String {
        let tools = ChatTools::with_names(ToolProfile::Task, &["read_file", ASK_USER], None);
        let environment = environment();
        render(&task_context(&tools, &environment)).unwrap()
    }

    #[test]
    fn a_catalog_without_the_tool_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file", "web_search"], None);
        let environment = environment();

        assert!(render(&chat_context(&tools, false, &environment)).is_none());
        assert!(render(&task_context(&tools, &environment)).is_none());
    }

    /// Every rule below is asserted whole rather than by a fragment of itself.
    /// A fragment survives the rule being inverted around it, and rewording ten
    /// of these constants once cost nine assertion edits; the constant is the
    /// spec line, so the test reads the constant.
    #[test]
    fn the_question_is_reserved_for_an_answer_that_changes_the_work() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(RESERVE), "{rendered}");
        }
    }

    #[test]
    fn the_conversation_is_read_before_the_user_is_asked_again() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(CONVERSATION), "{rendered}");
        }
    }

    #[test]
    fn a_detailed_request_is_carried_forward_on_a_stated_assumption() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(NARROWED), "{rendered}");
        }
    }

    #[test]
    fn one_question_is_the_shape_and_three_the_ceiling() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(COUNT), "{rendered}");
        }
    }

    #[test]
    fn the_turn_ends_on_the_call_so_independent_work_is_finished_first() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(LAST), "{rendered}");
        }
    }

    #[test]
    fn the_card_is_the_consent_and_is_not_asked_again_in_prose() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains(CARD), "{rendered}");
        }
    }

    /// A chat is answered either way, so the flag only means something where a
    /// default can be taken with nobody there to see it.
    #[test]
    fn required_is_reserved_for_a_default_that_would_be_wrong() {
        let task = task();
        let chat = chat();

        assert!(
            task.contains("Mark a question required only when the default would be wrong."),
            "{task}"
        );
        assert!(!chat.contains("Mark a question required"), "{chat}");
    }

    #[test]
    fn each_surface_says_what_the_question_costs_it() {
        let task = task();
        let chat = chat();

        assert!(task.contains("The run parks here"), "{task}");
        assert!(
            task.contains("goes ahead on the first option after about thirty seconds"),
            "{task}"
        );
        assert!(
            task.contains("a timed-out run is not retried, so its work is lost"),
            "{task}"
        );

        assert!(
            chat.contains("The answer comes back as the user's next message."),
            "{chat}"
        );
        assert!(!chat.contains("The run parks here"), "{chat}");
        assert!(!chat.contains("thirty seconds"), "{chat}");
    }

    #[test]
    fn a_wait_that_runs_out_is_not_the_user_having_answered() {
        let task = task();
        let chat = chat();

        assert!(
            task.contains("The wait running out is not an answer"),
            "{task}"
        );
        assert!(
            task.contains("name the option you took as your own"),
            "{task}"
        );
        assert!(!chat.contains("The wait running out"), "{chat}");
    }

    #[test]
    fn a_planner_chat_groups_its_questions_instead_of_asking_one() {
        let tools = ChatTools::with_names(
            ToolProfile::Chat,
            &[
                "read_file",
                ASK_USER,
                crate::agent::planner::FINALIZE_PROJECT,
            ],
            None,
        );
        let environment = environment();
        let rendered = render(&chat_context(&tools, false, &environment)).unwrap();
        assert!(rendered.contains(PLANNER_COUNT), "{rendered}");
        assert!(!rendered.contains(COUNT), "{rendered}");
    }

    #[test]
    fn an_unattended_run_is_told_nobody_answers_and_loses_the_required_rule() {
        let tools =
            ChatTools::with_names(ToolProfile::Task, &["read_file", ASK_USER], None).unattended();
        let environment = environment();
        let rendered = render(&task_context(&tools, &environment)).unwrap();
        assert!(rendered.contains(UNATTENDED_WAIT), "{rendered}");
        assert!(!rendered.contains(REQUIRED), "{rendered}");
        assert!(!rendered.contains(TASK_WAIT), "{rendered}");
        assert!(rendered.contains(ELAPSED), "{rendered}");
    }

    /// `boundary` owns what an elapsed wait means for consent and `tiers` owns
    /// when an approval is asked for. Both sit one sentence away from what this
    /// section says, which is exactly how a prompt starts saying it twice.
    #[test]
    fn the_wording_boundary_and_tiers_own_is_not_repeated_here() {
        for rendered in [chat(), task()] {
            assert!(!rendered.contains("silence is not a yes"), "{rendered}");
            assert!(!rendered.contains("Ask once, at the end"), "{rendered}");
            assert!(
                !rendered.contains("the message as it will read"),
                "{rendered}"
            );
        }
    }

    /// Those absences are literals, so either owner rewording turns all three
    /// vacuous and the duplication they exist to catch walks back in. Counting
    /// the owning constants over the assembled prompt is what still fails:
    /// whatever they say, the model reads each of them once.
    #[test]
    fn each_rule_another_section_owns_reaches_the_prompt_exactly_once() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["run_shell", ASK_USER], None);
        let environment = environment();
        let rendered = prompt::chat(&tools, false, &environment);

        for owned in [BOUNDARY, FINISH_FIRST] {
            assert_eq!(rendered.matches(owned).count(), 1, "{owned}");
        }
    }
}
