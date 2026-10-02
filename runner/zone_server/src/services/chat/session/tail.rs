//! What a resumed agent session is sent: what it has not seen, and nothing it has.

use std::collections::HashSet;

use zone_core::context::Entry;
use zone_core::llm::{Message, Role};
use zone_search::client::SearchContext;

use super::{INSTRUCTIONS, RunContext, prompt};

/// The one system note a resumed turn opens with.
///
/// The whole of `prompt` when it is not the one the session last saw, whose stable hash is
/// `seen`, else only its clock line; then this turn's workspace `retrieval`, and the outcome of
/// a web lookup when `search` ran one. Search that is off or was not asked for is said by the
/// prompt itself.
pub fn note(prompt: &str, seen: Option<&str>, retrieval: &str, search: &SearchContext) -> String {
    let changed = seen != Some(prompt::stable(prompt).as_str());
    let opening = if changed {
        Some(prompt.to_string())
    } else {
        prompt::now(prompt).map(str::to_string)
    };
    let outcome = search.has_lookup_outcome().then(|| search.prompt());
    [opening, Some(retrieval.trim().to_string()), outcome]
        .into_iter()
        .flatten()
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `context` cut down to what a resumed session has not seen: `note`, then the entries `unseen`
/// names, in the order they were written. Nothing when no user message is among them, which
/// leaves the session nothing to answer.
pub fn assemble(context: &RunContext, unseen: &[String], note: String) -> Option<RunContext> {
    let unseen: HashSet<&str> = unseen.iter().map(String::as_str).collect();
    let entries: Vec<Entry> = context
        .entries
        .iter()
        .filter(|entry| unseen.contains(entry.id.as_str()))
        .cloned()
        .collect();
    if !entries.iter().any(|entry| entry.message.role == Role::User) {
        return None;
    }
    Some(RunContext {
        entries: std::iter::once(Entry {
            id: INSTRUCTIONS.into(),
            message: Message::system(note),
            preserve: true,
            consumed: true,
        })
        .chain(entries)
        .collect(),
        summary: None,
        policy: context.policy.clone(),
        reason: context.reason.clone(),
        incomplete: context.incomplete,
        artifacts: context.artifacts.clone(),
        vision: context.vision,
    })
}

#[cfg(test)]
mod tests {
    use zone_core::context::project;
    use zone_core::llm::provider::render;
    use zone_search::client::SearchContext;

    use super::*;
    use crate::services::chat::session::composed;

    const EARLIER: &str = "2026-09-09T09:30:00+12:00";
    const LATER: &str = "2026-09-09T09:31:00+12:00";
    const ASKED: &str = "What changed in the deploy checklist?";
    const PICTURE: &str = "/api/artifacts/w/c/o/diagram.png";

    fn retrieval() -> String {
        prompt::retrieved(&["[knowledge] Deploys: run the checklist first.".to_string()])
    }

    /// A chat two turns in, as `session::build` and `prepare_chat` leave it: the
    /// instructions, the first exchange, the new question with an image, and the
    /// search supplement.
    fn context(prompt: &str) -> RunContext {
        let mut asked = Message::user(ASKED);
        asked.images = vec![PICTURE.to_string()];
        let mut context = RunContext::from_messages(vec![
            Message::system(prompt),
            Message::user("Where is the deploy checklist?"),
            Message::assistant("In the Operations space."),
            asked,
        ]);
        context.entries[0].id = INSTRUCTIONS.into();
        context.search(&SearchContext::Empty);
        context
    }

    fn unseen(context: &RunContext) -> Vec<String> {
        vec![context.entries[3].id.clone()]
    }

    fn sent(tail: &RunContext) -> String {
        render(&project(&tail.entries, tail.summary.as_ref()).expect("a projectable tail"))
    }

    #[test]
    fn a_resumed_turn_sends_the_prompt_again_only_when_it_changed() {
        let prompt = composed(EARLIER, "");
        let seen = prompt::stable(&prompt);
        let changed = composed(EARLIER, "\n\nThe user is called Ada.");

        let unchanged = note(&prompt, Some(&seen), "", &SearchContext::NotRequested);
        let resent = note(&changed, Some(&seen), "", &SearchContext::NotRequested);
        let unknown = note(&prompt, None, "", &SearchContext::NotRequested);

        assert!(
            !unchanged.contains("You are Zone's assistant"),
            "{unchanged}"
        );
        assert_eq!(Some(unchanged.as_str()), prompt::now(&prompt));
        assert_eq!(resent, changed);
        assert_eq!(
            unknown, prompt,
            "a session with no hash on record is sent the prompt"
        );
    }

    #[test]
    fn two_resumed_turns_a_minute_apart_do_not_resend_an_unchanged_prompt() {
        let earlier = composed(EARLIER, "");
        let later = composed(LATER, "");
        let seen = prompt::stable(&earlier);

        let first = note(&earlier, Some(&seen), "", &SearchContext::Disabled);
        let second = note(&later, Some(&seen), "", &SearchContext::Disabled);

        assert_eq!(prompt::stable(&later), seen);
        for (note, clock) in [(&first, "09:30:00"), (&second, "09:31:00")] {
            assert!(note.starts_with(crate::agent::prompt::NOW), "{note}");
            assert!(note.contains(clock), "{note}");
            assert!(!note.contains("You are Zone's assistant"), "{note}");
            assert_eq!(note.lines().count(), 1, "more than the clock line: {note}");
        }
    }

    #[test]
    fn a_resumed_turn_carries_this_turns_retrieval_search_and_attachments() {
        let prompt = composed(EARLIER, "");
        let context = context(&format!("{prompt}{}", retrieval()));
        let search = SearchContext::Empty;
        let seen = prompt::stable(&prompt);

        let tail = assemble(
            &context,
            &unseen(&context),
            note(&prompt, Some(&seen), &retrieval(), &search),
        )
        .expect("a tail with the new question");

        assert_eq!(tail.entries.len(), 2, "{:?}", tail.entries);
        assert_eq!(tail.entries[0].message.role, Role::System);
        let opening = tail.entries[0]
            .message
            .content
            .as_deref()
            .unwrap_or_default();
        assert!(opening.contains("<retrieved_context>"), "{opening}");
        assert!(
            opening.contains("Deploys: run the checklist first."),
            "{opening}"
        );
        assert!(opening.ends_with(search.prompt().trim()), "{opening}");
        assert!(opening.starts_with(crate::agent::prompt::NOW), "{opening}");
        assert_eq!(tail.entries[1].message.content.as_deref(), Some(ASKED));
        assert_eq!(tail.entries[1].message.images, [PICTURE]);
        assert_eq!(tail.summary, None);

        let stdin = sent(&tail);
        assert_eq!(stdin, format!("System:\n{opening}\n\nUser:\n{ASKED}"));
        assert!(!stdin.contains("Where is the deploy checklist?"), "{stdin}");
        assert!(!stdin.contains("In the Operations space."), "{stdin}");
    }

    #[test]
    fn a_resumed_turns_retrieval_never_overwrites_a_tail_entry() {
        let prompt = composed(EARLIER, "");
        let context = context(&prompt);
        let seen = prompt::stable(&prompt);

        let tail = assemble(
            &context,
            &unseen(&context),
            note(
                &prompt,
                Some(&seen),
                &retrieval(),
                &SearchContext::NotRequested,
            ),
        )
        .expect("a tail with the new question");

        let question = &context.entries[3];
        let carried = &tail.entries[1];
        assert_eq!(carried.id, question.id);
        assert_eq!(carried.message.role, Role::User);
        assert_eq!(carried.message.content, question.message.content);
        assert_eq!(carried.message.images, question.message.images);
        assert!(
            tail.entries[1..].iter().all(|entry| {
                !entry
                    .message
                    .content
                    .as_deref()
                    .unwrap_or_default()
                    .contains("<retrieved_context>")
            }),
            "the retrieval block landed on a tail entry: {:?}",
            tail.entries
        );
        assert_eq!(tail.entries[0].id, INSTRUCTIONS);
    }

    #[test]
    fn a_session_with_nothing_new_to_answer_has_no_tail() {
        let context = context(&composed(EARLIER, ""));
        let answered = vec![context.entries[2].id.clone()];

        assert!(assemble(&context, &[], "note".into()).is_none());
        assert!(assemble(&context, &answered, "note".into()).is_none());
    }
}
