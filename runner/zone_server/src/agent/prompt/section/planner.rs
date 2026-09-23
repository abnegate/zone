//! The interview a planner chat runs, and how it ends.
//!
//! Rendered only when the catalog holds `finalize_project`, which only a
//! planner chat is given: a chat that cannot make a project is never told to
//! plan one. The order of topics is the order a person can answer them in --
//! what and where before how it looks -- and the stop rule is what keeps the
//! interview from ending on a guess.

use crate::agent::planner::{CREATE_REPOSITORY, FINALIZE_PROJECT};
use crate::agent::prompt::Context;
use crate::agent::question::ASK_USER;

const HEADING: &str = "Planning a project:";

const ROLE: &str = "- You are specifying a software project end to end from the person's brief, so that every \
     task can be carried out by an unattended coding run without asking anyone anything. Your \
     job here is the interview and the plan; the runs do the building.";

const ORDER: &str = "- Ask in this order, skipping whatever the brief already settles: platforms and how it is \
     distributed; language, frameworks and major libraries; the repository (an existing one \
     the workspace has a source for, a URL, or a new one -- never a token); scope, the features \
     and where the first version stops; data, authentication and integrations; design: theme \
     archetype, primary and secondary colours, typography, animation style and accessibility; \
     testing: which levels and frameworks; continuous integration: which checks run on every \
     pull request; deployment: target, environments, what triggers a deploy and which secrets \
     the person must add to the repository (named, never collected); and the definition of \
     done.";

const CARDS: &str = "- Put two to four related questions on one ask_user card, one topic per card, the option \
     you would recommend first, and a free-text option for colours, names and anything a list \
     cannot hold. State inline what the brief already decided rather than asking it again.";

const STOP: &str = "- Keep asking until every task can be written without guessing. Then show one confirmation \
     card that summarises the plan -- name, repository, the tasks in order -- and only on a \
     yes call finalize_project, exactly once. Do not call it while a decision is open.";

/// The interview on a coding agent, which is served no ask_user: the questions
/// go in the reply, and the person's yes in the chat is the confirmation.
const MESSAGES: &str = "- Ask two to four related questions in a reply, one topic per reply, the option you would \
     recommend first. State inline what the brief already decided rather than asking it again.";

const CONFIRMATION: &str = "- Keep asking until every task can be written without guessing. Then summarise the plan in \
     one reply -- name, repository, the tasks in order -- and ask the person to confirm it; only \
     on a yes call finalize_project, exactly once. Do not call it while a decision is open.";

const TASKS: &str = "- Tasks are ordered, one pull request each, with a kind, a description precise enough to \
     need no question, acceptance criteria, and depends_on by index. The first task scaffolds \
     the repository when it is new (toolchain, formatter, linter, an empty test suite); the \
     continuous-integration task comes next and every later task depends on it; every feature \
     task ships its own tests; a deployment task adds the deploy workflow for the chosen target \
     (and a release flow when asked); a docs task closes. A plan missing CI, tests or \
     deployment is refused, unless the person chose no deployment.";

const REPOSITORY: &str = "- A new repository is made with create_repository from a connected GitHub source and the \
     person confirms it; pass the URL it returns to finalize_project together with the \
     source_id, so the runs have a credential.";

/// The planner section, rendered only for a chat whose catalog holds `finalize_project`.
pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if !context.tools.has(FINALIZE_PROJECT) {
        return None;
    }
    let mut rules = match context.tools.has(ASK_USER) {
        true => vec![ROLE, ORDER, CARDS, STOP, TASKS],
        false => vec![ROLE, ORDER, MESSAGES, CONFIRMATION, TASKS],
    };
    if context.tools.has(CREATE_REPOSITORY) {
        rules.push(REPOSITORY);
    }
    Some(format!("{HEADING}\n{}", rules.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt::test_support::{chat_context, environment};
    use crate::agent::{ChatTools, ToolProfile};

    #[test]
    fn only_a_planner_catalog_renders_the_interview() {
        let environment = environment();
        let plain = ChatTools::with_names(ToolProfile::Chat, &["read_file", "ask_user"], None);
        assert!(render(&chat_context(&plain, false, &environment)).is_none());
        let planner = ChatTools::with_names(
            ToolProfile::Chat,
            &["read_file", "ask_user", FINALIZE_PROJECT, CREATE_REPOSITORY],
            None,
        );
        let rendered = render(&chat_context(&planner, false, &environment)).unwrap();
        for rule in [ROLE, ORDER, CARDS, STOP, TASKS, REPOSITORY] {
            assert!(rendered.contains(rule), "{rendered}");
        }
        let without_repository =
            ChatTools::with_names(ToolProfile::Chat, &["ask_user", FINALIZE_PROJECT], None);
        let rendered = render(&chat_context(&without_repository, false, &environment)).unwrap();
        assert!(!rendered.contains(REPOSITORY), "{rendered}");
    }

    /// A planner chat on a coding agent is served no ask_user, so it is not
    /// told to raise cards it cannot: it interviews in its replies, and the
    /// person's yes in the chat is the confirmation.
    #[test]
    fn a_planner_with_no_cards_to_raise_interviews_in_its_replies() {
        let environment = environment();
        let planner = ChatTools::with_names(
            ToolProfile::Chat,
            &["read_file", FINALIZE_PROJECT, CREATE_REPOSITORY],
            None,
        );

        let rendered = render(&chat_context(&planner, false, &environment)).unwrap();

        assert!(!rendered.contains("ask_user"), "{rendered}");
        assert!(!rendered.contains("card"), "{rendered}");
        for rule in [ROLE, ORDER, MESSAGES, CONFIRMATION, TASKS, REPOSITORY] {
            assert!(rendered.contains(rule), "{rendered}");
        }
        assert!(rendered.contains("exactly once"), "{rendered}");
    }

    #[test]
    fn the_interview_asks_for_every_decision_the_runs_need() {
        for topic in [
            "platforms",
            "frameworks",
            "repository",
            "theme",
            "primary and secondary colours",
            "animation style",
            "testing",
            "continuous integration",
            "deployment",
            "definition of done",
        ] {
            assert!(ORDER.contains(topic), "{topic}");
        }
        assert!(STOP.contains("exactly once"));
        assert!(TASKS.contains("every later task depends on it"));
    }
}
