//! Write down what a run or a chat verified about a repository, unasked.
//!
//! Zone's other learned entries are earned: a pass counts evidence across
//! chats or runs before it files one. This is the write an agent makes on its
//! own judgement, in the middle of the work, when it has seen for itself
//! something the next run would otherwise rediscover. The store is
//! [`crate::db::knowledge::discovery`], which keeps one entry per subject and
//! leaves deciding what may be recorded to its caller. That caller is here.
//!
//! One tool, on both surfaces. A run is where most of what is worth recording
//! gets found and nobody is there to ask for it, so unlike memory this is not
//! chat-only. It is a [`Tier::Write`], which nobody is asked to confirm in a
//! chat or in a run and which a plan hold still refuses.
//!
//! An entry reaches every later prompt in the workspace, so both halves of it
//! pass [`rules::refused_discovery`] and the pair is held to the store's entry
//! bound before anything is written. No refusal repeats what it refused: a
//! refusal is read back into a transcript and onto a receipt, and the thing
//! refused may be the credential.
//!
//! Recording needs member standing, read again on every call. A viewer's chat
//! is still offered the tool, because assembling a catalog reads no database,
//! and is refused in words that say not to try again.

use abnegate_agent::Tool;
use abnegate_agent::ToolContext;
use abnegate_agent::ToolError;
use abnegate_agent::ToolRegistry;
use abnegate_agent::ToolResult;
use abnegate_agent::tool::Session;
use abnegate_agent::tool::Tier;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use super::memory::rules::{self, Refusal};
use super::tools::{WorkspaceScope, task_writer};
use crate::db::knowledge::discovery::{
    self, Discovery, Kind, MAX_ENTRY_BYTES, Outcome, Provenance, Source,
};

pub const RECORD_DISCOVERY: &str = "record_discovery";

/// What the tool tells the model it is for.
///
/// `pub` for the reason memory's descriptions are: it reaches the model's
/// catalog, and tests outside this module assert on it.
pub const DESCRIPTION: &str = "Record one durable thing you verified about this repository, so \
    the next run or chat starts with it. One subject per call; recording a subject again replaces \
    its entry, which is how to correct one. Requires member role or higher.";

/// What a stored call did, as the model reads it and as its receipt shows it.
pub const RECORDED: &str = "Recorded as a new entry.";

/// The subject already had an entry. That covers a changed statement and the
/// same statement confirmed again from another run or chat, under another
/// kind or on a later day, so the line claims only what is true of both.
pub const REPLACED: &str = "Recorded over this subject's earlier entry, which now carries this \
    statement and today's confirmation.";

pub const UNCHANGED: &str = "Already recorded in these words today; nothing changed.";

/// Why a call recorded nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declined {
    /// The call could not be read as a kind, a subject and a statement.
    Arguments,
    /// The kind is not one the store knows.
    Kind,
    /// The subject or the statement holds nothing.
    Blank,
    /// The subject and statement together exceed [`MAX_ENTRY_BYTES`].
    Oversized,
    /// The rulebook refused the subject or the statement.
    Screened(Refusal),
    /// The call belongs to no chat and no run, so it has no source to name.
    Sourceless,
    /// The caller is below member standing in the workspace.
    Standing,
    /// The subject's entry was deleted, and a deleted entry stays deleted.
    Retired,
    /// The store did not answer.
    Unwritable,
}

impl Declined {
    /// What the model is told. Every line opens the same way so that none can
    /// be read as a write that stuck, and none repeats the subject or the
    /// statement it refused.
    pub fn message(self) -> String {
        let reason = match self {
            Self::Kind => {
                return format!(
                    "Not recorded: the kind of a discovery is {}.",
                    Kind::ALL.map(Kind::as_str).join(" or ")
                );
            }
            Self::Oversized => {
                return format!(
                    "Not recorded: a subject and its statement may hold {MAX_ENTRY_BYTES} \
                     bytes together, and these hold more. Shorten the statement, or split the \
                     finding into separate subjects and record each on its own. What still \
                     does not fit goes in your reply, or in this run's report, rather than \
                     being dropped."
                );
            }
            Self::Arguments => "a discovery takes a kind, a subject and a statement, each as text.",
            Self::Blank => "a discovery needs a subject and a statement, and neither may be blank.",
            Self::Screened(Refusal::Secret) => {
                "that looks like a credential, a connection string carrying a password, or a \
                 long run of letters and digits such as a full commit hash. Name the variable \
                 rather than its value, or shorten the value."
            }
            Self::Screened(Refusal::Identifier) => {
                "that carries a person's contact detail. A discovery is about the repository, \
                 not about anyone who works on it."
            }
            Self::Screened(Refusal::Suppression) => {
                "an entry that would have a later run hold back an error, a disagreement or a \
                 concern is not something to record."
            }
            Self::Sourceless => {
                "this call belongs to no chat and no task run, so there is nothing to say the \
                 entry came from."
            }
            Self::Standing => {
                "recording a discovery needs member standing in this workspace. Do not try \
                 again in this chat."
            }
            Self::Retired => {
                "this subject's entry was deleted from the workspace, and a deleted entry \
                 stays deleted. Do not record it again."
            }
            Self::Unwritable => "the entry could not be written just now, and nothing changed.",
        };
        format!("Not recorded: {reason}")
    }
}

#[derive(Deserialize)]
struct Request {
    kind: String,
    subject: String,
    statement: String,
}

/// A call's arguments once everything that needs no database has been
/// decided about them.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: Kind,
    subject: String,
    statement: String,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    registry.register(Arc::new(RecordTool(scope.clone())));
}

struct RecordTool(WorkspaceScope);

#[async_trait]
impl Tool for RecordTool {
    fn name(&self) -> &str {
        RECORD_DISCOVERY
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": Kind::ALL.map(Kind::as_str),
                    "description": "convention: how this repository does something. lesson: a \
                                    command or trap that cost time and would again."
                },
                "subject": {
                    "type": "string",
                    "description": "What the entry is about, in a few words. The same subject \
                                    names the same entry."
                },
                "statement": {
                    "type": "string",
                    "description": bounded()
                }
            },
            "required": ["kind", "subject", "statement"],
            "additionalProperties": false
        })
    }

    fn tier(&self) -> Tier {
        Tier::Write
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(match self.run(params, context).await {
            Ok(stored) => ToolResult::success(stored),
            Err(declined) => ToolResult::error(declined.message()),
        })
    }
}

impl RecordTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, Declined> {
        let entry = entry(params)?;
        let scope = &self.0;
        let source = source(context.session, scope.chat_id)?;
        if !task_writer(&scope.state, scope.workspace_id, scope.user_id).await {
            return Err(Declined::Standing);
        }

        let discovery = Discovery {
            workspace_id: scope.workspace_id,
            subject: entry.subject,
            statement: entry.statement,
            provenance: Provenance {
                kind: entry.kind,
                source,
                last_confirmed: Utc::now().date_naive(),
            },
        };
        let (id, outcome) = discovery::record(scope.state.db(), &discovery)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "A discovery could not be written");
                Declined::Unwritable
            })?;

        // `title` is the subject under the name a receipt reads a document's
        // label from, and `outcome` is the line that receipt shows.
        Ok(json!({
            "id": id,
            "title": discovery.subject,
            "kind": discovery.provenance.kind.as_str(),
            "outcome": settled(outcome)?,
        })
        .to_string())
    }
}

/// The statement parameter's description, carrying the bound the tool
/// enforces. Stated where the model chooses its length, because the refusal it
/// would otherwise learn it from arrives after the write it wanted has failed.
fn bounded() -> String {
    format!(
        "What you verified about the subject, with the command or path that shows it. Subject \
         and statement hold {MAX_ENTRY_BYTES} bytes together at most; split a longer finding \
         across subjects."
    )
}

/// Everything about a call that can be decided without the store: its shape,
/// its kind, that neither half is blank, the entry bound, and the rulebook
/// over both halves, since both are rendered into later prompts.
fn entry(params: Value) -> Result<Entry, Declined> {
    let request: Request = serde_json::from_value(params).map_err(|_| Declined::Arguments)?;
    let kind = Kind::parse(request.kind.trim()).ok_or(Declined::Kind)?;
    let subject = request.subject.trim();
    let statement = request.statement.trim();
    if subject.is_empty() || statement.is_empty() {
        return Err(Declined::Blank);
    }
    if subject.len() + statement.len() > MAX_ENTRY_BYTES {
        return Err(Declined::Oversized);
    }
    for text in [subject, statement] {
        if let Some(refusal) = rules::refused_discovery(text) {
            return Err(Declined::Screened(refusal));
        }
    }
    Ok(Entry {
        kind,
        subject: subject.to_string(),
        statement: statement.to_string(),
    })
}

/// Where the agent was when it made the call. The session is what a run's
/// lease and a chat's assembly thread into the context; the scope's chat is
/// the same answer for a chat whose context was never given one.
fn source(session: Session, chat: Option<Uuid>) -> Result<Source, Declined> {
    match session {
        Session::Task(run) => Ok(Source::Run(run)),
        Session::Chat(chat) => Ok(Source::Chat(chat)),
        _ => chat.map(Source::Chat).ok_or(Declined::Sourceless),
    }
}

fn settled(outcome: Outcome) -> Result<&'static str, Declined> {
    match outcome {
        Outcome::Recorded => Ok(RECORDED),
        Outcome::Replaced => Ok(REPLACED),
        Outcome::Unchanged => Ok(UNCHANGED),
        Outcome::Retired => Err(Declined::Retired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::rules::test_support::{NOT_RECORDED, RECORDED as ACCEPTED};
    use crate::state::AppState;
    use abnegate_agent::tool::REASON_PARAMETER;

    const SUBJECT: &str = "Running the integration tests";
    const STATEMENT: &str = "Use cargo nextest; cargo test takes a single filter.";

    const EVERY: [Declined; 11] = [
        Declined::Arguments,
        Declined::Kind,
        Declined::Blank,
        Declined::Oversized,
        Declined::Screened(Refusal::Secret),
        Declined::Screened(Refusal::Identifier),
        Declined::Screened(Refusal::Suppression),
        Declined::Sourceless,
        Declined::Standing,
        Declined::Retired,
        Declined::Unwritable,
    ];

    fn tool() -> RecordTool {
        RecordTool(WorkspaceScope {
            state: AppState::for_tests(),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
            offline: false,
            origin: crate::db::tasks::Origin::Member,
        })
    }

    fn call(kind: &str, subject: &str, statement: &str) -> Value {
        json!({"kind": kind, "subject": subject, "statement": statement})
    }

    fn convention(subject: &str, statement: &str) -> Result<Entry, Declined> {
        entry(call("convention", subject, statement))
    }

    async fn declined(params: Value) -> String {
        let context = ToolContext::default().with_session(Session::Chat(Uuid::new_v4()));
        let result = tool()
            .execute(params, &context)
            .await
            .expect("a refusal is a tool error, not a failed call");
        assert!(!result.success, "{result:?}");
        assert_eq!(result.output, None);
        result.error.expect("a refusal carries its reason")
    }

    #[tokio::test]
    async fn the_tool_is_an_unconfirmed_write_that_leaves_the_turn_open() {
        let tool = tool();

        assert_eq!(tool.name(), RECORD_DISCOVERY);
        assert_eq!(tool.tier(), Tier::Write);
        assert!(!tool.tier().confirmed());
        assert!(tool.tier().mutating());
        assert!(!tool.ends_turn());
        assert!(
            tool.preview(&call("lesson", SUBJECT, STATEMENT)).is_none(),
            "an approval preview nobody asks for"
        );
        assert_eq!(
            tool.to_definition().function.description,
            tool.description()
        );
    }

    #[test]
    fn the_description_is_frozen() {
        assert_eq!(
            DESCRIPTION,
            "Record one durable thing you verified about this repository, so the next run or \
             chat starts with it. One subject per call; recording a subject again replaces its \
             entry, which is how to correct one. Requires member role or higher."
        );
    }

    #[tokio::test]
    async fn the_schema_asks_for_three_strings_and_states_the_bound() {
        assert_eq!(
            tool().parameters_schema(),
            json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["convention", "lesson"],
                        "description": "convention: how this repository does something. \
                                        lesson: a command or trap that cost time and would again."
                    },
                    "subject": {
                        "type": "string",
                        "description": "What the entry is about, in a few words. The same \
                                        subject names the same entry."
                    },
                    "statement": {
                        "type": "string",
                        "description": "What you verified about the subject, with the command \
                                        or path that shows it. Subject and statement hold 1000 \
                                        bytes together at most; split a longer finding across \
                                        subjects."
                    }
                },
                "required": ["kind", "subject", "statement"],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn the_schema_nests_nothing_and_asks_for_no_reason() {
        let schema = tool().parameters_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("the schema lists its properties");

        assert_eq!(properties.len(), 3);
        for (name, property) in properties {
            assert_eq!(property["type"], "string", "{name}");
        }
        assert!(!properties.contains_key(REASON_PARAMETER));
    }

    #[test]
    fn every_refusal_says_what_it_did_not_do() {
        for declined in EVERY {
            assert!(
                declined.message().starts_with("Not recorded: "),
                "{}",
                declined.message()
            );
        }
    }

    #[test]
    fn refusals_are_frozen() {
        let frozen = [
            "Not recorded: a discovery takes a kind, a subject and a statement, each as text.",
            "Not recorded: the kind of a discovery is convention or lesson.",
            "Not recorded: a discovery needs a subject and a statement, and neither may be \
             blank.",
            "Not recorded: a subject and its statement may hold 1000 bytes together, and these \
             hold more. Shorten the statement, or split the finding into separate subjects and \
             record each on its own. What still does not fit goes in your reply, or in this \
             run's report, rather than being dropped.",
            "Not recorded: that looks like a credential, a connection string carrying a \
             password, or a long run of letters and digits such as a full commit hash. Name the \
             variable rather than its value, or shorten the value.",
            "Not recorded: that carries a person's contact detail. A discovery is about the \
             repository, not about anyone who works on it.",
            "Not recorded: an entry that would have a later run hold back an error, a \
             disagreement or a concern is not something to record.",
            "Not recorded: this call belongs to no chat and no task run, so there is nothing to \
             say the entry came from.",
            "Not recorded: recording a discovery needs member standing in this workspace. Do \
             not try again in this chat.",
            "Not recorded: this subject's entry was deleted from the workspace, and a deleted \
             entry stays deleted. Do not record it again.",
            "Not recorded: the entry could not be written just now, and nothing changed.",
        ];

        for (declined, expected) in EVERY.into_iter().zip(frozen) {
            assert_eq!(declined.message(), expected, "{declined:?}");
        }
    }

    #[test]
    fn the_outcome_lines_are_frozen() {
        assert_eq!(settled(Outcome::Recorded), Ok("Recorded as a new entry."));
        assert_eq!(
            settled(Outcome::Replaced),
            Ok(
                "Recorded over this subject's earlier entry, which now carries this statement \
                 and today's confirmation."
            )
        );
        assert_eq!(
            settled(Outcome::Unchanged),
            Ok("Already recorded in these words today; nothing changed.")
        );
        assert_eq!(settled(Outcome::Retired), Err(Declined::Retired));
    }

    #[test]
    fn an_entry_is_trimmed_and_keeps_its_kind() {
        assert_eq!(
            entry(call(
                " lesson ",
                "  Running the tests \n",
                "\tUse nextest.  "
            )),
            Ok(Entry {
                kind: Kind::Lesson,
                subject: "Running the tests".to_string(),
                statement: "Use nextest.".to_string(),
            })
        );
    }

    #[test]
    fn every_accepted_note_passes_as_a_subject_and_as_a_statement() {
        for text in ACCEPTED {
            assert!(convention(text, STATEMENT).is_ok(), "subject: {text}");
            assert!(convention(SUBJECT, text).is_ok(), "statement: {text}");
        }
    }

    #[test]
    fn every_refused_note_is_refused_as_a_subject_and_as_a_statement() {
        for (text, refusal) in NOT_RECORDED {
            assert_eq!(
                convention(text, STATEMENT),
                Err(Declined::Screened(refusal)),
                "subject: {text}"
            );
            assert_eq!(
                convention(SUBJECT, text),
                Err(Declined::Screened(refusal)),
                "statement: {text}"
            );
        }
    }

    #[test]
    fn a_call_that_is_not_three_strings_is_refused() {
        for params in [
            json!({}),
            json!({"kind": "lesson", "subject": SUBJECT}),
            json!({"kind": "lesson", "statement": STATEMENT}),
            json!({"subject": SUBJECT, "statement": STATEMENT}),
            json!({"kind": "lesson", "subject": 7, "statement": STATEMENT}),
            json!({"kind": "lesson", "subject": SUBJECT, "statement": [STATEMENT]}),
            json!("lesson"),
        ] {
            assert_eq!(entry(params.clone()), Err(Declined::Arguments), "{params}");
        }
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        for kind in ["document", "Convention", "", "memory-fact"] {
            assert_eq!(
                entry(call(kind, SUBJECT, STATEMENT)),
                Err(Declined::Kind),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_blank_subject_or_statement_is_refused() {
        for (subject, statement) in [("", STATEMENT), (" \n\t", STATEMENT), (SUBJECT, "  ")] {
            assert_eq!(convention(subject, statement), Err(Declined::Blank));
        }
    }

    #[test]
    fn the_entry_bound_is_the_store_s_and_is_inclusive() {
        let fits = "s".repeat(MAX_ENTRY_BYTES - SUBJECT.len());
        let over = "s".repeat(MAX_ENTRY_BYTES - SUBJECT.len() + 1);

        let kept = convention(SUBJECT, &fits).expect("an entry of exactly the bound is taken");
        assert_eq!(kept.subject.len() + kept.statement.len(), MAX_ENTRY_BYTES);
        assert_eq!(convention(SUBJECT, &over), Err(Declined::Oversized));
        assert_eq!(
            convention(&"s".repeat(MAX_ENTRY_BYTES), "s"),
            Err(Declined::Oversized),
            "the bound is on the pair, whichever half is long"
        );
        assert_eq!(
            convention(SUBJECT, &"é".repeat(MAX_ENTRY_BYTES / 2)),
            Err(Declined::Oversized),
            "bytes, as the rendered block is measured, not characters"
        );
    }

    #[test]
    fn a_call_is_sourced_from_its_run_or_its_chat() {
        let run = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let scoped = Uuid::new_v4();

        assert_eq!(
            source(Session::Task(run), None),
            Ok(Source::Run(run)),
            "a run's scope carries no chat"
        );
        assert_eq!(
            source(Session::Task(run), Some(scoped)),
            Ok(Source::Run(run))
        );
        assert_eq!(
            source(Session::Chat(chat), Some(scoped)),
            Ok(Source::Chat(chat))
        );
        assert_eq!(
            source(Session::Detached, Some(scoped)),
            Ok(Source::Chat(scoped))
        );
        assert_eq!(
            source(Session::Detached, None),
            Err(Declined::Sourceless),
            "a run that has not taken its lease has nothing to name"
        );
    }

    /// Through `execute`, against a pool nothing answers on: each of these is
    /// decided before the store is reached, and what comes back is the frozen
    /// line and no part of what was sent.
    #[tokio::test]
    async fn a_refused_call_is_answered_without_repeating_what_it_refused() {
        for (text, refusal) in NOT_RECORDED {
            for params in [
                call("lesson", text, STATEMENT),
                call("lesson", SUBJECT, text),
            ] {
                let message = declined(params).await;
                assert_eq!(message, Declined::Screened(refusal).message(), "{text}");
                assert!(!message.contains(text), "{message}");
                for word in text.split_whitespace().filter(|word| word.len() > 12) {
                    assert!(!message.contains(word), "{word} in {message}");
                }
            }
        }

        let long = "s".repeat(MAX_ENTRY_BYTES);
        assert_eq!(
            declined(call("lesson", SUBJECT, &long)).await,
            Declined::Oversized.message()
        );
        assert_eq!(
            declined(call("lesson", " ", STATEMENT)).await,
            Declined::Blank.message()
        );
        let unknown = declined(call("ghp_abcdefgh1234", SUBJECT, STATEMENT)).await;
        assert_eq!(unknown, Declined::Kind.message());
        assert!(!unknown.contains("ghp_"), "{unknown}");
        assert_eq!(
            declined(json!({"kind": "lesson", "subject": 7})).await,
            Declined::Arguments.message()
        );
    }
}
