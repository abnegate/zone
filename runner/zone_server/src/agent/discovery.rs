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
//! bound before anything is written. A person checks an entry by reading its
//! wiki page, so a character that page would not show them is refused too,
//! and what is stored is the one line the prompt will render. No refusal
//! repeats what it refused: a refusal is read back into a transcript and onto
//! a receipt, and the thing refused may be the credential.
//!
//! The same holds for the trace, which is written as a call starts and before
//! the tool has refused anything. Everything a call can be refused for
//! without the store is decided by one pure function, so [`trace_arguments`]
//! asks it first and a call the tool will refuse leaves its kind and nothing
//! else where the workspace reads it.
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
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::LazyLock;
use uuid::Uuid;

use super::memory::rules::{self, Refusal};
use super::tools::{WorkspaceScope, task_writer};
use crate::db::knowledge::discovery::{
    self, Discovery, Kind, MAX_ENTRY_BYTES, Outcome, Provenance, Source,
};

pub const RECORD_DISCOVERY: &str = "record_discovery";

/// Characters a reader of the stored entry would not see: controls, the
/// format characters (zero-width marks, direction overrides and isolates, the
/// soft hyphen) and the whole tag block, assigned or not. Ordinary white
/// space never reaches it, because an entry is collapsed first.
static UNPRINTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{Cc}\p{Cf}\x{E0000}-\x{E007F}]").expect("a valid character class")
});

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
/// same statement recorded again from another run or chat, under another
/// kind or on a later day, so the line claims only what is true of both.
pub const REPLACED: &str = "Recorded over this subject's earlier entry, which now carries this \
    statement and today's date.";

pub const UNCHANGED: &str = "Already recorded in these words today; nothing changed.";

/// Why a call recorded nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declined {
    /// The call could not be read as a kind, a subject and a statement and
    /// nothing besides.
    Arguments,
    /// The kind is not one the store knows.
    Kind,
    /// The subject or the statement holds nothing.
    Blank,
    /// The subject or the statement holds a character that does not print.
    Unprintable,
    /// The subject and statement together exceed [`MAX_ENTRY_BYTES`].
    Oversized,
    /// The rulebook refused the subject or the statement.
    Screened(Refusal),
    /// The call belongs to no chat and no run, so it has no source to name.
    Sourceless,
    /// The caller is below member standing in the workspace.
    Standing,
    /// An entry with this subject or this statement was deleted, and a
    /// deleted entry stays deleted.
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
            Self::Arguments => {
                "a discovery takes a kind, a subject and a statement, each as text, and no \
                 other field."
            }
            Self::Blank => "a discovery needs a subject and a statement, and neither may be blank.",
            Self::Unprintable => {
                "the subject or the statement carries a character that does not print, such as \
                 a zero-width space, a text-direction mark or a control code. Write it again in \
                 plain characters."
            }
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
                "an entry with this subject or this statement was deleted from the workspace, \
                 and a deleted entry stays deleted. Do not record it again."
            }
            Self::Unwritable => "the entry could not be written just now, and nothing changed.",
        };
        format!("Not recorded: {reason}")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    kind: String,
    subject: String,
    statement: String,
}

/// What a stored call answers with: the record the model reads, and the one
/// its receipt is built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub id: Uuid,
    pub subject: String,
    pub kind: Kind,
    /// One of [`RECORDED`], [`REPLACED`] and [`UNCHANGED`].
    pub outcome: String,
    /// What the subject's entry said before this call wrote over it, so a
    /// model that did not mean to replace it can see that it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<String>,
}

/// What the trace keeps of a call the tool will refuse: its kind when that is
/// one the store knows, a fixed word, and nothing the model wrote.
#[derive(Default, Serialize, Deserialize)]
struct Withheld {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<Kind>,
}

/// The arguments the trace shows for a call, if they are not to be shown as
/// they are.
///
/// `None` means show them: the call is another tool's, or it is one this tool
/// will take as far as the store, whose entry the workspace reads anyway. A
/// call [`entry`] refuses is refused for its shape, its size or what the
/// screen found in it, and the last of those may be a credential, so all of
/// them leave the kind alone. A reason volunteered beside the three fields
/// goes the same way, since a call carrying one is refused for its shape.
pub fn trace_arguments(name: &str, arguments: &str) -> Option<String> {
    if name != RECORD_DISCOVERY {
        return None;
    }
    let parameters = serde_json::from_str::<Value>(arguments).unwrap_or(Value::Null);
    if entry(parameters.clone()).is_ok() {
        return None;
    }
    let withheld: Withheld = serde_json::from_value(parameters).unwrap_or_default();
    Some(json!(withheld).to_string())
}

/// The stated reason the trace and the receipt show for a call. This tool
/// takes none, so one that arrives is text the screen never read.
pub fn trace_reason(name: &str, reason: Option<String>) -> Option<String> {
    if name == RECORD_DISCOVERY {
        None
    } else {
        reason
    }
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

    async fn execute(
        &self,
        parameters: Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(match self.run(parameters, context).await {
            Ok(stored) => ToolResult::success(json!(stored).to_string()),
            Err(declined) => ToolResult::error(declined.message()),
        })
    }
}

impl RecordTool {
    async fn run(&self, parameters: Value, context: &ToolContext) -> Result<Stored, Declined> {
        let entry = entry(parameters)?;
        let scope = &self.0;
        let source = source(context.session, scope.chat_id)?;
        if !task_writer(&scope.state, scope.workspace_id, scope.user_id).await {
            return Err(Declined::Standing);
        }

        let discovery = Discovery {
            workspace_id: scope.workspace_id,
            subject: entry.subject,
            statement: entry.statement,
            author: scope.user_id,
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
        let (outcome, superseded) = settled(outcome)?;

        Ok(Stored {
            id,
            subject: discovery.subject,
            kind: discovery.provenance.kind,
            outcome: outcome.to_string(),
            superseded,
        })
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
/// its kind, that neither half is blank or carries what does not print, the
/// entry bound, and the rulebook over both halves, since both are rendered
/// into later prompts.
///
/// Both halves are collapsed to one line first, so the bound and the screen
/// read what will be stored, and what is stored is what a prompt renders.
/// Pure, which is what lets [`trace_arguments`] ask it before the call runs.
fn entry(parameters: Value) -> Result<Entry, Declined> {
    let request: Request = serde_json::from_value(parameters).map_err(|_| Declined::Arguments)?;
    let kind = Kind::parse(request.kind.trim()).ok_or(Declined::Kind)?;
    let subject = discovery::collapsed(&request.subject);
    let statement = discovery::collapsed(&request.statement);
    if subject.is_empty() || statement.is_empty() {
        return Err(Declined::Blank);
    }
    if UNPRINTED.is_match(&subject) || UNPRINTED.is_match(&statement) {
        return Err(Declined::Unprintable);
    }
    if subject.len() + statement.len() > MAX_ENTRY_BYTES {
        return Err(Declined::Oversized);
    }
    for text in [&subject, &statement] {
        if let Some(refusal) = rules::refused_discovery(text) {
            return Err(Declined::Screened(refusal));
        }
    }
    Ok(Entry {
        kind,
        subject,
        statement,
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

/// The line a stored call answers with, and the statement it wrote over when
/// it wrote over one.
fn settled(outcome: Outcome) -> Result<(&'static str, Option<String>), Declined> {
    match outcome {
        Outcome::Recorded => Ok((RECORDED, None)),
        Outcome::Replaced { superseded } => Ok((REPLACED, Some(superseded))),
        Outcome::Unchanged => Ok((UNCHANGED, None)),
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

    const EVERY: [Declined; 12] = [
        Declined::Arguments,
        Declined::Kind,
        Declined::Blank,
        Declined::Unprintable,
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

    async fn declined(parameters: Value) -> String {
        let context = ToolContext::default().with_session(Session::Chat(Uuid::new_v4()));
        let result = tool()
            .execute(parameters, &context)
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
            "Not recorded: a discovery takes a kind, a subject and a statement, each as text, \
             and no other field.",
            "Not recorded: the kind of a discovery is convention or lesson.",
            "Not recorded: a discovery needs a subject and a statement, and neither may be \
             blank.",
            "Not recorded: the subject or the statement carries a character that does not \
             print, such as a zero-width space, a text-direction mark or a control code. Write \
             it again in plain characters.",
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
            "Not recorded: an entry with this subject or this statement was deleted from the \
             workspace, and a deleted entry stays deleted. Do not record it again.",
            "Not recorded: the entry could not be written just now, and nothing changed.",
        ];

        for (declined, expected) in EVERY.into_iter().zip(frozen) {
            assert_eq!(declined.message(), expected, "{declined:?}");
        }
    }

    #[test]
    fn the_outcome_lines_are_frozen() {
        assert_eq!(
            settled(Outcome::Recorded),
            Ok(("Recorded as a new entry.", None))
        );
        assert_eq!(
            settled(Outcome::Replaced {
                superseded: STATEMENT.to_string()
            }),
            Ok((
                "Recorded over this subject's earlier entry, which now carries this statement \
                 and today's date.",
                Some(STATEMENT.to_string())
            ))
        );
        assert_eq!(
            settled(Outcome::Unchanged),
            Ok((
                "Already recorded in these words today; nothing changed.",
                None
            ))
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
        for parameters in [
            json!({}),
            json!({"kind": "lesson", "subject": SUBJECT}),
            json!({"kind": "lesson", "statement": STATEMENT}),
            json!({"subject": SUBJECT, "statement": STATEMENT}),
            json!({"kind": "lesson", "subject": 7, "statement": STATEMENT}),
            json!({"kind": "lesson", "subject": SUBJECT, "statement": [STATEMENT]}),
            json!("lesson"),
        ] {
            assert_eq!(
                entry(parameters.clone()),
                Err(Declined::Arguments),
                "{parameters}"
            );
        }
    }

    /// The schema says no other property is taken, and the tools that own a
    /// reason ask for one. A field this tool never read is one the screen
    /// never read either.
    #[test]
    fn a_field_the_tool_does_not_take_is_refused() {
        for (field, value) in [
            (REASON_PARAMETER, "So the next run starts with it."),
            ("title", SUBJECT),
        ] {
            let mut parameters = call("lesson", SUBJECT, STATEMENT);
            parameters[field] = json!(value);
            assert_eq!(entry(parameters), Err(Declined::Arguments), "{field}");
        }
    }

    /// One of each kind of character a reader of the wiki page would not see
    /// and the model reading the prompt would.
    const UNPRINTABLE: [(&str, char); 13] = [
        ("a zero-width space", '\u{200b}'),
        ("a zero-width non-joiner", '\u{200c}'),
        ("a zero-width joiner", '\u{200d}'),
        ("a word joiner", '\u{2060}'),
        ("a byte-order mark", '\u{feff}'),
        ("a soft hyphen", '\u{ad}'),
        ("a right-to-left override", '\u{202e}'),
        ("a directional isolate", '\u{2066}'),
        ("a tag character", '\u{e0041}'),
        ("an unassigned code point of the tag block", '\u{e0002}'),
        ("a null", '\u{0}'),
        ("an escape", '\u{1b}'),
        ("a C1 control", '\u{9b}'),
    ];

    #[test]
    fn a_character_that_does_not_print_is_refused_in_either_half() {
        for (name, unprintable) in UNPRINTABLE {
            assert_eq!(
                convention(&format!("Running the{unprintable} tests"), STATEMENT),
                Err(Declined::Unprintable),
                "{name} in the subject"
            );
            assert_eq!(
                convention(SUBJECT, &format!("Use cargo{unprintable} nextest.")),
                Err(Declined::Unprintable),
                "{name} in the statement"
            );
        }
        assert_eq!(
            convention("\u{200b}", STATEMENT),
            Err(Declined::Unprintable),
            "a subject of nothing but what does not print is not a blank one"
        );
    }

    /// A subject is matched by its fingerprint, and the store's sees past
    /// case and spacing only. Were the character let through it would name a
    /// second entry for a subject that reads the same.
    #[test]
    fn a_zero_width_character_cannot_mint_a_second_subject() {
        let split = "Running the integra\u{200b}tion tests";

        assert_ne!(
            discovery::fingerprint(split),
            discovery::fingerprint(SUBJECT)
        );
        assert_eq!(convention(split, STATEMENT), Err(Declined::Unprintable));
    }

    #[test]
    fn ordinary_white_space_is_collapsed_and_never_refused() {
        assert_eq!(
            convention(
                " Running\tthe\u{a0}integration\r\n\r\ntests\u{2029}",
                "Use cargo nextest;\ncargo test\u{2028}takes a\u{85}single   filter."
            ),
            Ok(Entry {
                kind: Kind::Convention,
                subject: SUBJECT.to_string(),
                statement: STATEMENT.to_string(),
            })
        );
    }

    #[test]
    fn a_call_the_tool_will_refuse_is_traced_by_its_kind_alone() {
        let long = "s".repeat(MAX_ENTRY_BYTES);
        for (parameters, shown) in [
            (
                call("lesson", "The deploy token is ghp_abcdefgh1234", STATEMENT),
                r#"{"kind":"lesson"}"#,
            ),
            (
                call("convention", SUBJECT, "It is ghp_abcdefgh1234"),
                r#"{"kind":"convention"}"#,
            ),
            (call("lesson", SUBJECT, &long), r#"{"kind":"lesson"}"#),
            (call("lesson", " ", STATEMENT), r#"{"kind":"lesson"}"#),
            (
                call("lesson", "Running\u{202e} the tests", STATEMENT),
                r#"{"kind":"lesson"}"#,
            ),
            (
                json!({
                    "kind": "lesson",
                    "subject": SUBJECT,
                    "statement": STATEMENT,
                    REASON_PARAMETER: "The token is ghp_abcdefgh1234",
                }),
                r#"{"kind":"lesson"}"#,
            ),
            (call("ghp_abcdefgh1234", SUBJECT, STATEMENT), "{}"),
            (
                json!({"kind": "lesson", "subject": 7}),
                r#"{"kind":"lesson"}"#,
            ),
            (json!({"kind": 7, "subject": SUBJECT}), "{}"),
            (json!("ghp_abcdefgh1234"), "{}"),
        ] {
            assert!(entry(parameters.clone()).is_err(), "{parameters}");
            assert_eq!(
                trace_arguments(RECORD_DISCOVERY, &parameters.to_string()).as_deref(),
                Some(shown),
                "{parameters}"
            );
        }
        assert_eq!(
            trace_arguments(RECORD_DISCOVERY, "ghp_abcdefgh1234, not json").as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn a_call_the_tool_will_take_and_every_other_tool_s_are_traced_as_written() {
        let taken = call("lesson", SUBJECT, STATEMENT).to_string();

        assert_eq!(trace_arguments(RECORD_DISCOVERY, &taken), None);
        assert_eq!(
            trace_arguments("read_file", r#"{"path":"ghp_abcdefgh1234"}"#),
            None,
            "another tool's arguments are not this module's to judge"
        );
    }

    #[test]
    fn the_trace_shows_no_reason_for_this_tool_and_every_other_tool_s_as_stated() {
        assert_eq!(
            trace_reason(
                RECORD_DISCOVERY,
                Some("The token is ghp_abcdefgh1234".into())
            ),
            None
        );
        assert_eq!(
            trace_reason("read_file", Some("To see what main does.".into())).as_deref(),
            Some("To see what main does.")
        );
    }

    #[test]
    fn a_stored_call_answers_with_one_record_and_names_what_it_wrote_over() {
        let id = Uuid::from_u128(0x5d0c_7e1a);
        let recorded = Stored {
            id,
            subject: SUBJECT.to_string(),
            kind: Kind::Lesson,
            outcome: RECORDED.to_string(),
            superseded: None,
        };
        assert_eq!(
            json!(recorded),
            json!({"id": id, "subject": SUBJECT, "kind": "lesson", "outcome": RECORDED})
        );

        let replaced = Stored {
            outcome: REPLACED.to_string(),
            superseded: Some("Use cargo test.".to_string()),
            ..recorded.clone()
        };
        assert_eq!(
            json!(replaced),
            json!({
                "id": id,
                "subject": SUBJECT,
                "kind": "lesson",
                "outcome": REPLACED,
                "superseded": "Use cargo test.",
            })
        );
        for stored in [recorded, replaced] {
            assert_eq!(
                serde_json::from_value::<Stored>(json!(stored)).ok(),
                Some(stored),
                "a receipt reads back what the tool wrote"
            );
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
            for parameters in [
                call("lesson", text, STATEMENT),
                call("lesson", SUBJECT, text),
            ] {
                let message = declined(parameters).await;
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
