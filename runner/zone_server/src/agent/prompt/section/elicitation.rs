//! When a decision is put to the user, and when it is made without them.
//!
//! `ask_user` is the only question this model can ask and have answered, which
//! is what makes it worth spending bytes on in both directions: a run that
//! guesses at a fork it cannot see past is wrong for the rest of the turn, and
//! a run that asks what it could have read has stopped for nothing. A question
//! costs a task run more than it costs a chat, so the cost is stated per
//! surface. Only a catalog holding the tool renders this.

use crate::agent::prompt::{Context, Surface};

pub(in crate::agent::prompt) const ASK_USER: &str = "ask_user";

const HEADING: &str = "Asking the user:";

const RESERVE: &str = "- Reserve ask_user for a decision whose answer changes what you do next; what reading \
     or testing settles is not a question.";

const CONVERSATION: &str =
    "- Check the conversation first: the answer is often already in what the user told you.";

const NARROWED: &str = "- A detailed request has already narrowed the problem: state your assumption inline \
     rather than stopping to ask.";

const COUNT: &str = "- One question is the shape to aim for, three the ceiling.";

const LAST: &str = "- Your turn ends the moment you call it, so finish everything that does not depend on \
     the answer first.";

const CARD: &str = "- The card is the consent: do not also ask in prose or restate the options in your \
     reply.";

/// Only a task run goes ahead on a default, so only a task run can be wrong to;
/// in a chat the user's next message answers either way.
const REQUIRED: &str =
    "- Mark a question required only when going ahead on the default would be wrong.";

const TASK_WAIT: &str = "- The run parks here: an optional question goes ahead on the first option after about \
     thirty seconds, a required one waits, and a timed-out run is not retried, so the work \
     so far is lost.";

const CHAT_WAIT: &str = "- The answer comes back as the user's next message.";

/// Pairs with `TASK_WAIT`: the default is the one option nobody chose, and a
/// report that hands it to the user as their decision is what this prevents.
const ELAPSED: &str =
    "- The wait running out is not an answer: name the option you went ahead on as your own.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if !context.tools.has(ASK_USER) {
        return None;
    }
    let rules = match context.surface {
        Surface::Chat => vec![
            RESERVE,
            CONVERSATION,
            NARROWED,
            COUNT,
            LAST,
            CARD,
            CHAT_WAIT,
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

    #[test]
    fn the_question_is_reserved_for_an_answer_that_changes_the_work() {
        for rendered in [chat(), task()] {
            assert!(
                rendered.contains("a decision whose answer changes what you do next"),
                "{rendered}"
            );
            assert!(
                rendered.contains("what reading or testing settles is not a question"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn the_conversation_is_read_before_the_user_is_asked_again() {
        for rendered in [chat(), task()] {
            assert!(
                rendered.contains("Check the conversation first"),
                "{rendered}"
            );
            assert!(
                rendered.contains("already in what the user told you"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn a_detailed_request_is_carried_forward_on_a_stated_assumption() {
        for rendered in [chat(), task()] {
            assert!(
                rendered.contains("A detailed request has already narrowed the problem"),
                "{rendered}"
            );
            assert!(
                rendered.contains("state your assumption inline rather than stopping to ask"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn one_question_is_the_shape_and_three_the_ceiling() {
        for rendered in [chat(), task()] {
            assert!(
                rendered.contains("One question is the shape to aim for, three the ceiling."),
                "{rendered}"
            );
        }
    }

    #[test]
    fn the_turn_ends_on_the_call_so_independent_work_is_finished_first() {
        for rendered in [chat(), task()] {
            assert!(
                rendered.contains("Your turn ends the moment you call it"),
                "{rendered}"
            );
            assert!(
                rendered.contains("finish everything that does not depend on the answer first"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn the_card_is_the_consent_and_is_not_asked_again_in_prose() {
        for rendered in [chat(), task()] {
            assert!(rendered.contains("The card is the consent"), "{rendered}");
            assert!(
                rendered.contains("do not also ask in prose or restate the options"),
                "{rendered}"
            );
        }
    }

    /// A chat is answered either way, so the flag only means something where a
    /// default can be taken with nobody there to see it.
    #[test]
    fn required_is_reserved_for_a_default_that_would_be_wrong() {
        let task = task();
        let chat = chat();

        assert!(
            task.contains(
                "Mark a question required only when going ahead on the default would be wrong."
            ),
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
            task.contains("a timed-out run is not retried, so the work so far is lost"),
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
            task.contains("name the option you went ahead on as your own"),
            "{task}"
        );
        assert!(!chat.contains("The wait running out"), "{chat}");
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
}
