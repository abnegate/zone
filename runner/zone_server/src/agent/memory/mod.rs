//! Remember what the user asked to have remembered, and nothing else.
//!
//! Zone's other memory is server-derived: the learning loop earns a standing
//! instruction by clearing an occurrence and agreement bar. This is the other
//! half — what a person said about themselves, written because they asked for
//! it, read back under their own name.
//!
//! Every tool here is chat-only. A background run reads the user's profile and
//! preferences from the rendered block and can act on them, but it cannot
//! write them: rewriting someone's profile from a run they are not watching is
//! the one write in this area with no undo, and a receipt reaches a chat's
//! message metadata, not a run's log. The registration point is the existing
//! `chat_id.is_some()` gate, so this needs no new mechanism.
//!
//! What the server can enforce about *content* lives in [`rules`], and what it
//! cannot is stated in the prompt instead — [`rules`]'s own module doc draws
//! that line.

use async_trait::async_trait;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Arc;
use zone_core::tools::{Session, Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::WorkspaceScope;
use crate::db::memory::{
    self, MAX_FACTS, MAX_NAME_CHARS, MemoryCategory, MemoryIndexRow, MemoryOutcome, MemoryRow,
    MemoryWrite,
};

pub mod render;
pub mod rules;

pub const MEMORY_LIST: &str = "memory_list";
pub const MEMORY_READ: &str = "memory_read";
pub const MEMORY_WRITE: &str = "memory_write";
pub const MEMORY_APPEND: &str = "memory_append";
pub const MEMORY_DELETE: &str = "memory_delete";

/// What the chat's trace may keep of a memory call.
///
/// The trace — the `tool_calls` record on the assistant message and the live
/// frames that mirror it — is readable by anyone who can read the chat, which
/// is the workspace. What these five tools are handed and hand back belongs to
/// one person. So the trace keeps a call's name, whether it succeeded and how
/// long it took, and nothing it carried: the arguments shrink to the category
/// alone, the way a receipt names a fact by its kind, and the outcome line is
/// fixed. The model is unaffected — its copy of the result travels as the
/// tool-result message, which the trace never was.
pub fn is_private(name: &str) -> bool {
    matches!(
        name,
        MEMORY_LIST | MEMORY_READ | MEMORY_WRITE | MEMORY_APPEND | MEMORY_DELETE
    )
}

/// The arguments the trace shows for a call, if this tool's are private.
///
/// `None` means show them as they are. For a memory tool the category is kept
/// when the arguments parse — it is one of three fixed words and names
/// nobody — and everything else is dropped. Arguments that do not parse are
/// shown as nothing at all rather than guessed at.
pub fn trace_arguments(name: &str, arguments: &str) -> Option<String> {
    if !is_private(name) {
        return None;
    }
    let category = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| value.get("category")?.as_str().map(str::to_owned));
    Some(match category {
        Some(category) => serde_json::json!({ "category": category }).to_string(),
        None => "{}".to_string(),
    })
}

/// The outcome line the trace shows for a memory call, whichever way it went.
///
/// One line for success and failure alike: a success's first line is the
/// stored content, and a refusal's names the entry it refused.
pub const TRACE_DETAIL: &str = "Private to the person it belongs to; not shown here.";

/// The outcome line the trace shows for a finished call.
pub fn trace_detail(name: &str, detail: &str) -> String {
    if is_private(name) {
        TRACE_DETAIL.to_string()
    } else {
        detail.to_string()
    }
}

/// What each tool tells the model it is for.
///
/// `pub` rather than private as `question.rs` and `wait.rs` keep theirs: these
/// reach the model's catalog and the prompt-assembly tests assert on them from
/// outside this module.
pub const LIST_DESCRIPTION: &str = "List what is remembered for this user: each entry's name \
    and what it is for, with the version to quote when changing it. Read one with memory_read.";

pub const READ_DESCRIPTION: &str = "Read one remembered entry in full. Do this before changing \
    an entry, so the version you write with is the version you read.";

pub const WRITE_DESCRIPTION: &str = "Store something the user asked to have remembered, or \
    replace an entry you have read. Their profile, how they want you to work, or a named fact.";

pub const APPEND_DESCRIPTION: &str = "Add a line to a remembered entry without rewriting it. \
    Use this when the entry is a list and the user added to it.";

pub const DELETE_DESCRIPTION: &str = "Forget a remembered entry the user asked you to forget. \
    Never do this on your own initiative.";

/// A task run has no user to remember for, so this reads as a redirection
/// rather than as a failure.
pub fn chat_only() -> String {
    "Memory is available in a chat only. A background run has no user to remember for: leave \
     it to whoever started the run."
        .to_string()
}

/// A fact nobody can tell the purpose of is a fact no later turn will read, so
/// the description is refused rather than defaulted.
pub fn description_required() -> String {
    "A remembered fact needs a description: one sentence saying what it is for, so a later \
     turn can tell whether to read it."
        .to_string()
}

pub fn version_required() -> String {
    format!(
        "{MEMORY_DELETE} needs the version {MEMORY_READ} returned, so a delete proves it read \
         what it is removing."
    )
}

#[derive(Deserialize)]
struct ListRequest {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct ReadRequest {
    category: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct WriteRequest {
    category: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    content: String,
    #[serde(default)]
    version: Option<i64>,
}

#[derive(Deserialize)]
struct AppendRequest {
    category: String,
    #[serde(default)]
    name: Option<String>,
    content: String,
}

#[derive(Deserialize)]
struct DeleteRequest {
    category: String,
    #[serde(default)]
    name: Option<String>,
    version: i64,
}

/// The one argument name this module looks up rather than deserializes: a
/// delete whose version is absent is refused in the words that tell the model
/// how to get one, where `serde` could only report a missing field.
const VERSION: &str = "version";

/// A memory call that could not be read at all. The five shapes differ, so this
/// names the vocabulary rather than one tool's arguments.
const INVALID_ARGUMENTS: &str = "A memory call takes a category, and whichever of name, \
    description, content and version its tool asks for.";

/// The store did not answer. Says nothing about what is remembered, because
/// nothing was read.
const UNREADABLE: &str = "Memory could not be read just now. Tell the user that rather than \
    answering as though there is nothing to read.";

const UNWRITABLE: &str = "Memory could not be written just now, and nothing changed. Tell the \
    user that rather than letting them think it did.";

const NOTHING_REMEMBERED: &str = "Nothing is remembered for this user yet.";

/// A fact is reached by the name it was given, so one written without a name
/// could never be read back.
fn name_required() -> String {
    format!(
        "A {} needs a name to be read back by. A {} and a {} hold one entry each and take none.",
        MemoryCategory::Fact,
        MemoryCategory::Profile,
        MemoryCategory::Preference
    )
}

fn unknown_category(supplied: &str) -> String {
    format!(
        "{supplied:?} is not a kind of memory. Use {}, {} or {}.",
        MemoryCategory::Profile,
        MemoryCategory::Preference,
        MemoryCategory::Fact
    )
}

/// The words a schema offers, in the order the enum declares them, so a kind
/// added later reaches the model without five schemas being edited by hand.
fn kinds() -> Vec<&'static str> {
    MemoryCategory::ALL
        .into_iter()
        .map(MemoryCategory::short)
        .collect()
}

/// A name parameter's description, carrying the bound the store enforces.
///
/// Stated where the model chooses the name, because the refusal it would
/// otherwise learn it from arrives after the write it wanted has failed.
fn names(lead: &str) -> String {
    format!("{lead} {MAX_NAME_CHARS} characters at most.")
}

/// Register all five memory tools.
///
/// The scope is not optional. Every path here needs a workspace and a user, so
/// a caller holding neither is a compile error rather than a refusal string
/// nothing in production could reach.
pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    registry.register(Arc::new(ListTool(scope.clone())));
    registry.register(Arc::new(ReadTool(scope.clone())));
    registry.register(Arc::new(WriteTool(scope.clone())));
    registry.register(Arc::new(AppendTool(scope.clone())));
    registry.register(Arc::new(DeleteTool(scope.clone())));
}

struct ListTool(WorkspaceScope);
struct ReadTool(WorkspaceScope);
struct WriteTool(WorkspaceScope);
struct AppendTool(WorkspaceScope);
struct DeleteTool(WorkspaceScope);

#[async_trait]
impl Tool for ListTool {
    fn name(&self) -> &str {
        MEMORY_LIST
    }

    fn description(&self) -> &str {
        LIST_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": kinds(),
                    "description": "Limit the index to one kind. Omit for everything stored."
                }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(reply(self.run(params, context).await))
    }
}

impl ListTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        only_in_a_chat(context)?;
        let request: ListRequest = parse(params)?;
        let filter = request.category.as_deref().map(category).transpose()?;
        let scope = &self.0;
        let rows = memory::index(scope.state.db(), scope.workspace_id, scope.user_id, filter)
            .await
            .map_err(|error| unreadable(MEMORY_LIST, error))?;

        Ok(index(&rows))
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        MEMORY_READ
    }

    fn description(&self) -> &str {
        READ_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": kinds()},
                "name": {
                    "type": "string",
                    "description": names(
                        "The name memory_list gave. Omit for profile and preference, which hold \
                         one entry each."
                    )
                }
            },
            "required": ["category"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(reply(self.run(params, context).await))
    }
}

impl ReadTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        only_in_a_chat(context)?;
        let request: ReadRequest = parse(params)?;
        let category = category(&request.category)?;
        let title = entry(category, request.name.as_deref())?;
        let scope = &self.0;
        let row = memory::read(
            scope.state.db(),
            scope.workspace_id,
            scope.user_id,
            category,
            &title,
        )
        .await
        .map_err(|error| unreadable(MEMORY_READ, error))?;

        match row {
            Some(row) => Ok(entry_in_full(category, &row)),
            None => Err(memory::missing(category, &title)),
        }
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        MEMORY_WRITE
    }

    fn description(&self) -> &str {
        WRITE_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": kinds()},
                "name": {
                    "type": "string",
                    "description": names(
                        "What to call this, for a fact. Ignored for profile and preference."
                    )
                },
                "description": {
                    "type": "string",
                    "description": "One sentence saying what this entry is for, so a later turn \
                                    can tell whether to read it. Required for a fact."
                },
                "content": {
                    "type": "string",
                    "description": "What to remember, in the user's own terms. Durable phrasing \
                                    over a precise figure."
                },
                "version": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "The version memory_read returned. Omit to create something \
                                    new; supply it to replace what you read."
                }
            },
            "required": ["category", "content"],
            "additionalProperties": false
        })
    }

    fn tier(&self) -> Tier {
        Tier::Write
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(reply(self.run(params, context).await))
    }
}

impl WriteTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        only_in_a_chat(context)?;
        let request: WriteRequest = parse(params)?;
        let category = category(&request.category)?;
        let title = entry(category, request.name.as_deref())?;
        let description = stated(request.description.as_deref());
        if category == MemoryCategory::Fact && description.is_none() {
            return Err(description_required());
        }
        allowed(&request.content, description)?;

        let scope = &self.0;
        let outcome = memory::write(
            scope.state.db(),
            MemoryWrite {
                workspace_id: scope.workspace_id,
                user_id: scope.user_id,
                category,
                title: &title,
                description,
                content: &request.content,
                version: request.version,
            },
        )
        .await
        .map_err(|error| unwritable(MEMORY_WRITE, error))?;

        Ok(match stored(outcome, category, &title)? {
            Stored::Created(version) => memory::remembered(category, &title, version),
            Stored::Updated(version) => memory::updated(category, &title, version),
        })
    }
}

#[async_trait]
impl Tool for AppendTool {
    fn name(&self) -> &str {
        MEMORY_APPEND
    }

    fn description(&self) -> &str {
        APPEND_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": kinds()},
                "name": {
                    "type": "string",
                    "description": names(
                        "The name memory_list gave. Omit for profile and preference."
                    )
                },
                "content": {
                    "type": "string",
                    "description": "The line to add. The entry keeps everything already in it."
                }
            },
            "required": ["category", "content"],
            "additionalProperties": false
        })
    }

    fn tier(&self) -> Tier {
        Tier::Write
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(reply(self.run(params, context).await))
    }
}

impl AppendTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        only_in_a_chat(context)?;
        let request: AppendRequest = parse(params)?;
        let category = category(&request.category)?;
        let title = entry(category, request.name.as_deref())?;
        allowed(&request.content, None)?;

        let scope = &self.0;
        let outcome = memory::append(
            scope.state.db(),
            scope.workspace_id,
            scope.user_id,
            category,
            &title,
            &request.content,
        )
        .await
        .map_err(|error| unwritable(MEMORY_APPEND, error))?;

        Ok(memory::appended(
            category,
            &title,
            stored(outcome, category, &title)?.version(),
        ))
    }
}

#[async_trait]
impl Tool for DeleteTool {
    fn name(&self) -> &str {
        MEMORY_DELETE
    }

    fn description(&self) -> &str {
        DELETE_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {"type": "string", "enum": kinds()},
                "name": {
                    "type": "string",
                    "description": "The name memory_list gave. Omit for profile and preference."
                },
                "version": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "The version memory_read returned. A delete has to prove it \
                                    read what it is removing."
                }
            },
            "required": ["category", "version"],
            "additionalProperties": false
        })
    }

    fn tier(&self) -> Tier {
        Tier::Write
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(reply(self.run(params, context).await))
    }
}

impl DeleteTool {
    async fn run(&self, params: Value, context: &ToolContext) -> Result<String, String> {
        only_in_a_chat(context)?;
        if params.get(VERSION).is_none() {
            return Err(version_required());
        }
        let request: DeleteRequest = parse(params)?;
        let category = category(&request.category)?;
        let title = entry(category, request.name.as_deref())?;

        let scope = &self.0;
        let outcome = memory::forget(
            scope.state.db(),
            scope.workspace_id,
            scope.user_id,
            category,
            &title,
            request.version,
        )
        .await
        .map_err(|error| unwritable(MEMORY_DELETE, error))?;

        stored(outcome, category, &title)?;
        Ok(memory::forgotten(category, &title))
    }
}

/// A refusal is an observation the model can act on, so it comes back as a tool
/// error and the turn continues.
fn reply(result: Result<String, String>) -> ToolResult {
    match result {
        Ok(message) => ToolResult::success(message),
        Err(refusal) => ToolResult::error(refusal),
    }
}

/// The surface rule, enforced where the tool runs rather than only where it is
/// registered: the registration is one line inside one gate, and a line is the
/// kind of thing a later edit moves.
fn only_in_a_chat(context: &ToolContext) -> Result<(), String> {
    match context.session {
        Session::Task(_) => Err(chat_only()),
        _ => Ok(()),
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, String> {
    serde_json::from_value(params).map_err(|_| INVALID_ARGUMENTS.to_string())
}

fn category(supplied: &str) -> Result<MemoryCategory, String> {
    MemoryCategory::parse(supplied).ok_or_else(|| unknown_category(supplied))
}

/// The title this call addresses, resolved the way the store resolves it, so
/// the message the model reads and the receipt the console renders both name
/// the entry that was really written.
fn entry(category: MemoryCategory, name: Option<&str>) -> Result<String, String> {
    if let Some(title) = category.title() {
        return Ok(title.to_string());
    }
    stated(name).map(str::to_string).ok_or_else(name_required)
}

/// An absent field and one holding blank space are the same thing to a reader.
fn stated(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// Both halves of an entry are stored, so the rulebook screens both.
fn allowed(content: &str, description: Option<&str>) -> Result<(), String> {
    for text in std::iter::once(content).chain(description) {
        if let Some(refusal) = rules::refused(text) {
            return Err(refusal.message().to_string());
        }
    }
    Ok(())
}

/// What a write settled at, once the outcomes that are not a write have been
/// turned into what the model is told instead.
enum Stored {
    Created(i64),
    Updated(i64),
}

impl Stored {
    fn version(self) -> i64 {
        match self {
            Self::Created(version) | Self::Updated(version) => version,
        }
    }
}

/// One place where an outcome becomes a refusal, so the three writers cannot
/// drift on what a conflict, a missing entry or an over-long one says.
fn stored(outcome: MemoryOutcome, category: MemoryCategory, title: &str) -> Result<Stored, String> {
    match outcome {
        MemoryOutcome::Created { version } => Ok(Stored::Created(version)),
        MemoryOutcome::Updated { version } => Ok(Stored::Updated(version)),
        MemoryOutcome::Conflict { version, content } => {
            Err(memory::conflict(category, title, version, &content))
        }
        MemoryOutcome::Missing => Err(memory::missing(category, title)),
        MemoryOutcome::TooLong { limit } => Err(memory::too_long(limit)),
        MemoryOutcome::NameTooLong { limit } => Err(memory::name_too_long(limit)),
    }
}

/// What an entry is called, what it is for, and the version to quote when
/// changing it.
fn headline(
    category: MemoryCategory,
    title: &str,
    version: i64,
    description: Option<&str>,
) -> String {
    match stated(description) {
        Some(description) => format!("{category}/{title}, version {version}. {description}"),
        None => format!("{category}/{title}, version {version}."),
    }
}

fn entry_in_full(category: MemoryCategory, row: &MemoryRow) -> String {
    format!(
        "{}\n\n{}",
        headline(
            category,
            &row.title,
            row.version,
            row.description.as_deref()
        ),
        row.content
    )
}

/// What a list says when it stopped at its bound. The rendered block sends the
/// model here for what it left out, so a list that stopped without saying so
/// would be the end of that trail.
fn more_than_listed() -> String {
    format!(
        "Not every entry is listed here: this index stops at the first {MAX_FACTS}. \
         {MEMORY_READ} opens one by name."
    )
}

/// A row whose kind this build does not know is left out rather than described
/// under a word the model cannot pass back.
fn index(rows: &[MemoryIndexRow]) -> String {
    let bound = usize::try_from(MAX_FACTS).unwrap_or(usize::MAX);
    let lines: Vec<String> = rows
        .iter()
        .take(bound)
        .filter_map(|row| {
            let category = MemoryCategory::from_stored(&row.category)?;
            Some(headline(
                category,
                &row.title,
                row.version,
                row.description.as_deref(),
            ))
        })
        .collect();

    if lines.is_empty() {
        return NOTHING_REMEMBERED.to_string();
    }
    if rows.len() > bound {
        return format!("{}\n{}", lines.join("\n"), more_than_listed());
    }
    lines.join("\n")
}

fn unreadable(tool: &str, error: sqlx::Error) -> String {
    tracing::warn!(tool, %error, "Memory could not be read");
    UNREADABLE.to_string()
}

fn unwritable(tool: &str, error: sqlx::Error) -> String {
    tracing::warn!(tool, %error, "Memory could not be written");
    UNWRITABLE.to_string()
}

#[cfg(test)]
mod tests {
    use super::rules::Refusal;
    use super::*;
    use crate::db::memory::{
        FACT_CATEGORY, MAX_ENTRY_CHARS, MAX_FACTS, MAX_NAME_CHARS, MemoryCategory,
        PREFERENCE_CATEGORY, PREFERENCES_TITLE,
    };
    use crate::state::{AppState, test_config};
    use regex::Regex;
    use uuid::Uuid;

    /// The trace is workspace-readable and memory is one person's. Realistic
    /// arguments, because a test that redacts `{}` proves nothing.
    #[test]
    fn a_memory_call_shows_its_kind_in_the_trace_and_nothing_else() {
        let written = trace_arguments(
            MEMORY_WRITE,
            r#"{"category":"fact","name":"Deploy window","description":"When we ship.","content":"Thursdays, never Fridays."}"#,
        )
        .expect("a memory tool's arguments are private");
        assert_eq!(written, r#"{"category":"fact"}"#);

        assert_eq!(
            trace_arguments(MEMORY_LIST, "{}").as_deref(),
            Some("{}"),
            "a listing carries no category and shows nothing"
        );
        assert_eq!(
            trace_arguments(MEMORY_READ, "not json").as_deref(),
            Some("{}"),
            "arguments that do not parse are dropped, not shown"
        );
        assert!(
            trace_arguments("read_file", r#"{"path":"src/main.rs"}"#).is_none(),
            "any other tool's arguments are shown as they are"
        );

        assert_eq!(
            trace_detail(MEMORY_READ, "Thursdays, never Fridays. (3 lines)"),
            TRACE_DETAIL
        );
        assert_eq!(
            trace_detail(
                MEMORY_WRITE,
                "Memory write refused: Deploy window is at version 3"
            ),
            TRACE_DETAIL,
            "a refusal names the entry, so it is hidden the same way"
        );
        assert_eq!(
            trace_detail("read_file", "fn main() {} (12 lines)"),
            "fn main() {} (12 lines)"
        );
    }

    /// PR 6's rule for its timeout strings, carried over: a refusal a model
    /// can read as a write that stuck is worse than no refusal at all. Two of
    /// the four claim words are also this feature's vocabulary — `Not stored:`
    /// and `a remembered fact` — so the rule is the affirmative position, not
    /// the word.
    fn claims_success(text: &str) -> bool {
        let claim = Regex::new(
            r"(?i)(?:^|[.!?]\s+)(?:remembered|stored|saved|noted)\b|\b(?:i|we|it|that|this|has been|have been|was|were)\s+(?:remembered|stored|saved|noted)\b",
        )
        .expect("claim pattern is a valid regex");
        claim.is_match(text)
    }

    /// Every refusal the feature can return, tool-level and rulebook alike,
    /// and the one informational line a successful list may carry, which is
    /// held to the same rule: it is read in the same place as a result.
    fn refusals() -> Vec<String> {
        let mut all = vec![
            chat_only(),
            description_required(),
            version_required(),
            name_required(),
            unknown_category("facts"),
            INVALID_ARGUMENTS.to_string(),
            UNREADABLE.to_string(),
            UNWRITABLE.to_string(),
            more_than_listed(),
            memory::missing(MemoryCategory::Fact, "Deploy window"),
            memory::conflict(
                MemoryCategory::Fact,
                "Deploy window",
                3,
                "Thursdays, after standup.",
            ),
            memory::too_long(MAX_ENTRY_CHARS),
            memory::name_too_long(MAX_NAME_CHARS),
        ];
        all.extend(
            [Refusal::Secret, Refusal::Identifier, Refusal::Suppression]
                .into_iter()
                .map(|refusal| refusal.message().to_string()),
        );
        all
    }

    #[test]
    fn every_tool_name_is_prefixed_and_distinct() {
        let names = [
            MEMORY_LIST,
            MEMORY_READ,
            MEMORY_WRITE,
            MEMORY_APPEND,
            MEMORY_DELETE,
        ];
        for name in names {
            assert!(name.starts_with("memory_"), "{name}");
        }
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "two tools share a name");
    }

    #[test]
    fn no_refusal_can_be_read_as_a_write_having_happened() {
        for refusal in refusals() {
            assert!(
                !claims_success(&refusal),
                "{refusal:?} reads as a write that stuck"
            );
        }
        assert!(
            claims_success(&crate::db::memory::remembered(MemoryCategory::Fact, "x", 1)),
            "the check has to fail on an actual success or it proves nothing"
        );
    }

    #[test]
    fn no_refusal_claims_a_write_happened_in_the_word_it_would_use() {
        for refusal in refusals() {
            let lowered = refusal.to_lowercase();
            for claim in ["saved", "noted"] {
                assert!(!lowered.contains(claim), "{refusal:?} contains {claim:?}");
            }
        }
    }

    #[test]
    fn chat_only_says_who_owns_the_write_instead() {
        assert_eq!(
            chat_only(),
            "Memory is available in a chat only. A background run has no user to remember \
             for: leave it to whoever started the run."
        );
    }

    #[test]
    fn description_required_says_what_the_description_is_for() {
        assert_eq!(
            description_required(),
            "A remembered fact needs a description: one sentence saying what it is for, so a \
             later turn can tell whether to read it."
        );
    }

    #[test]
    fn version_required_names_both_tools() {
        assert_eq!(
            version_required(),
            "memory_delete needs the version memory_read returned, so a delete proves it read \
             what it is removing."
        );
    }

    #[test]
    fn descriptions_are_frozen() {
        assert_eq!(
            LIST_DESCRIPTION,
            "List what is remembered for this user: each entry's name and what it is for, \
             with the version to quote when changing it. Read one with memory_read."
        );
        assert_eq!(
            READ_DESCRIPTION,
            "Read one remembered entry in full. Do this before changing an entry, so the \
             version you write with is the version you read."
        );
        assert_eq!(
            WRITE_DESCRIPTION,
            "Store something the user asked to have remembered, or replace an entry you have \
             read. Their profile, how they want you to work, or a named fact."
        );
        assert_eq!(
            APPEND_DESCRIPTION,
            "Add a line to a remembered entry without rewriting it. Use this when the entry \
             is a list and the user added to it."
        );
        assert_eq!(
            DELETE_DESCRIPTION,
            "Forget a remembered entry the user asked you to forget. Never do this on your \
             own initiative."
        );
    }

    #[test]
    fn a_description_naming_another_tool_names_it_correctly() {
        assert!(LIST_DESCRIPTION.contains(MEMORY_READ));
        assert!(version_required().contains(MEMORY_READ));
        assert!(version_required().contains(MEMORY_DELETE));
    }

    #[test]
    fn no_description_leaks_the_stored_category_spelling() {
        let descriptions = [
            LIST_DESCRIPTION,
            READ_DESCRIPTION,
            WRITE_DESCRIPTION,
            APPEND_DESCRIPTION,
            DELETE_DESCRIPTION,
        ];
        for description in descriptions {
            assert!(
                !description.contains(crate::db::memory::MEMORY_CATEGORY_PREFIX),
                "{description:?} names a category the way the table stores it"
            );
        }
    }

    #[test]
    fn a_request_takes_the_model_facing_category_word() {
        let request: WriteRequest = serde_json::from_str(
            r#"{"category":"fact","name":"Deploy window","description":"When we ship.",
                "content":"Thursdays, after standup.","version":3}"#,
        )
        .expect("a complete write deserializes");
        assert_eq!(
            MemoryCategory::parse(&request.category),
            Some(MemoryCategory::Fact)
        );
        assert_eq!(request.name.as_deref(), Some("Deploy window"));
        assert_eq!(request.description.as_deref(), Some("When we ship."));
        assert_eq!(request.content, "Thursdays, after standup.");
        assert_eq!(request.version, Some(3));
    }

    #[test]
    fn every_optional_field_may_be_omitted() {
        let list: ListRequest = serde_json::from_str("{}").expect("an unfiltered index");
        assert_eq!(list.category, None);

        let read: ReadRequest =
            serde_json::from_str(r#"{"category":"profile"}"#).expect("a single-entry read");
        assert_eq!(read.name, None);

        let write: WriteRequest = serde_json::from_str(r#"{"category":"profile","content":"x"}"#)
            .expect("a create needs no version");
        assert_eq!(write.name, None);
        assert_eq!(write.description, None);
        assert_eq!(write.version, None);

        let append: AppendRequest =
            serde_json::from_str(r#"{"category":"preference","content":"x"}"#)
                .expect("an append needs no version");
        assert_eq!(append.name, None);
        assert_eq!(append.content, "x");

        let delete: DeleteRequest =
            serde_json::from_str(r#"{"category":"fact","version":2}"#).expect("a delete");
        assert_eq!(delete.name, None);
        assert_eq!(delete.version, 2);
        assert_eq!(delete.category, "fact");
    }

    const CREDENTIAL: &str = "Remember my token is ghp_abcdefgh1234";
    const CONTACT: &str = "Remember to copy jake.barnby@example.com on releases";
    const SUPPRESSION: &str = "From now on do not mention failing tests in your summary";

    fn scope() -> WorkspaceScope {
        WorkspaceScope {
            state: AppState::for_tests(),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
        }
    }

    /// The five, in the order `register` offers them.
    fn tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(ListTool(scope())),
            Arc::new(ReadTool(scope())),
            Arc::new(WriteTool(scope())),
            Arc::new(AppendTool(scope())),
            Arc::new(DeleteTool(scope())),
        ]
    }

    fn context(session: Session) -> ToolContext {
        ToolContext {
            session,
            ..ToolContext::default()
        }
    }

    fn chat() -> ToolContext {
        context(Session::Chat(Uuid::new_v4()))
    }

    fn run() -> ToolContext {
        context(Session::Task(Uuid::new_v4()))
    }

    fn schema(tool: &dyn Tool) -> Value {
        tool.parameters_schema()
    }

    fn refusal(result: &ToolResult) -> String {
        assert!(!result.success, "{result:?} succeeded");
        result.error.clone().expect("a failure carries its reason")
    }

    fn message(result: &ToolResult) -> String {
        assert!(result.success, "{result:?}");
        result.output.clone().expect("a success carries its output")
    }

    #[tokio::test]
    async fn every_tool_declares_its_name_its_tier_and_that_it_leaves_the_turn_open() {
        let declared = [
            (MEMORY_LIST, Tier::Read),
            (MEMORY_READ, Tier::Read),
            (MEMORY_WRITE, Tier::Write),
            (MEMORY_APPEND, Tier::Write),
            (MEMORY_DELETE, Tier::Write),
        ];

        for (tool, (name, tier)) in tools().into_iter().zip(declared) {
            assert_eq!(tool.name(), name);
            assert_eq!(tool.tier(), tier, "{name}");
            assert!(!tool.ends_turn(), "{name} ends the turn");
            assert!(
                tool.preview(&json!({})).is_none(),
                "{name} renders an approval preview nobody asks for"
            );
            assert_eq!(
                tool.description(),
                tool.to_definition().function.description
            );
        }
    }

    #[tokio::test]
    async fn a_registration_offers_all_five_and_nothing_else() {
        let mut registry = ToolRegistry::new();
        register(&mut registry, &scope());

        let mut names = registry.names();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                MEMORY_APPEND,
                MEMORY_DELETE,
                MEMORY_LIST,
                MEMORY_READ,
                MEMORY_WRITE
            ]
        );
    }

    #[tokio::test]
    async fn the_index_asks_only_for_an_optional_kind() {
        assert_eq!(
            schema(&ListTool(scope())),
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": ["profile", "preference", "fact"],
                        "description": "Limit the index to one kind. Omit for everything stored."
                    }
                },
                "required": [],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn a_read_asks_for_a_kind_and_the_name_the_index_gave() {
        assert_eq!(
            schema(&ReadTool(scope())),
            json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["profile", "preference", "fact"]},
                    "name": {
                        "type": "string",
                        "description": "The name memory_list gave. Omit for profile and preference, which hold one entry each. 256 characters at most."
                    }
                },
                "required": ["category"],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn a_write_asks_for_everything_an_entry_holds() {
        assert_eq!(
            schema(&WriteTool(scope())),
            json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["profile", "preference", "fact"]},
                    "name": {
                        "type": "string",
                        "description": "What to call this, for a fact. Ignored for profile and preference. 256 characters at most."
                    },
                    "description": {
                        "type": "string",
                        "description": "One sentence saying what this entry is for, so a later turn can tell whether to read it. Required for a fact."
                    },
                    "content": {
                        "type": "string",
                        "description": "What to remember, in the user's own terms. Durable phrasing over a precise figure."
                    },
                    "version": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "The version memory_read returned. Omit to create something new; supply it to replace what you read."
                    }
                },
                "required": ["category", "content"],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn an_append_asks_for_the_line_and_nothing_else() {
        assert_eq!(
            schema(&AppendTool(scope())),
            json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["profile", "preference", "fact"]},
                    "name": {
                        "type": "string",
                        "description": "The name memory_list gave. Omit for profile and preference. 256 characters at most."
                    },
                    "content": {
                        "type": "string",
                        "description": "The line to add. The entry keeps everything already in it."
                    }
                },
                "required": ["category", "content"],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn a_delete_asks_for_the_version_it_has_to_prove() {
        assert_eq!(
            schema(&DeleteTool(scope())),
            json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["profile", "preference", "fact"]},
                    "name": {
                        "type": "string",
                        "description": "The name memory_list gave. Omit for profile and preference."
                    },
                    "version": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "The version memory_read returned. A delete has to prove it read what it is removing."
                    }
                },
                "required": ["category", "version"],
                "additionalProperties": false
            })
        );
    }

    /// A nested discriminator is what `SYSTEM-PROMPT-RESEARCH` records local
    /// models failing on, so every kind is a flat string the model can copy.
    #[tokio::test]
    async fn no_schema_nests_a_choice_or_accepts_a_field_it_never_named() {
        for tool in tools() {
            let schema = schema(tool.as_ref());
            let name = tool.name();
            assert_eq!(schema["additionalProperties"], json!(false), "{name}");
            let rendered = schema.to_string();
            for nested in ["oneOf", "anyOf", "allOf", "$ref"] {
                assert!(!rendered.contains(nested), "{name} nests {nested}");
            }
            assert!(
                !rendered.contains(crate::db::memory::MEMORY_CATEGORY_PREFIX),
                "{name} names a kind the way the table stores it"
            );
            if let Some(kinds_offered) = schema["properties"]["category"].get("enum") {
                assert_eq!(kinds_offered, &json!(kinds()), "{name}");
            }
        }
    }

    #[tokio::test]
    async fn no_memory_tool_asks_the_model_for_a_reason() {
        for tool in tools() {
            assert!(
                tool.parameters_schema()["properties"]
                    .get(zone_core::tools::REASON_PARAM)
                    .is_none(),
                "{} asks for a reason",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn a_background_run_is_refused_by_every_tool() {
        for tool in tools() {
            let result = tool
                .execute(json!({}), &run())
                .await
                .expect("a refusal is a tool error, not a failed call");
            assert_eq!(refusal(&result), chat_only(), "{}", tool.name());
        }
    }

    #[tokio::test]
    async fn a_remembered_fact_without_a_description_is_refused() {
        let result = WriteTool(scope())
            .execute(
                json!({"category": "fact", "name": "Deploy window", "content": "Thursdays."}),
                &chat(),
            )
            .await
            .unwrap();

        assert_eq!(refusal(&result), description_required());
    }

    #[tokio::test]
    async fn a_fact_with_no_name_could_never_be_read_back() {
        let result = WriteTool(scope())
            .execute(
                json!({"category": "fact", "description": "When we ship.", "content": "Thursdays."}),
                &chat(),
            )
            .await
            .unwrap();

        assert_eq!(refusal(&result), name_required());
    }

    #[tokio::test]
    async fn a_delete_without_a_version_is_refused() {
        let result = DeleteTool(scope())
            .execute(json!({"category": "preference"}), &chat())
            .await
            .unwrap();

        assert_eq!(refusal(&result), version_required());
    }

    #[tokio::test]
    async fn a_kind_that_is_not_one_of_the_three_is_refused_by_name() {
        let result = ReadTool(scope())
            .execute(json!({"category": "memory-fact"}), &chat())
            .await
            .unwrap();

        assert_eq!(refusal(&result), unknown_category("memory-fact"));
    }

    #[tokio::test]
    async fn the_rulebook_is_reached_before_anything_is_written() {
        for (text, expected) in [
            (CREDENTIAL, Refusal::Secret),
            (CONTACT, Refusal::Identifier),
            (SUPPRESSION, Refusal::Suppression),
        ] {
            let written = WriteTool(scope())
                .execute(json!({"category": "profile", "content": text}), &chat())
                .await
                .unwrap();
            assert_eq!(refusal(&written), expected.message(), "{text}");

            let appended = AppendTool(scope())
                .execute(json!({"category": "profile", "content": text}), &chat())
                .await
                .unwrap();
            assert_eq!(refusal(&appended), expected.message(), "{text}");
        }
    }

    /// A description is stored text like the content is, and an identifier put
    /// there would be read back at every turn just the same.
    #[tokio::test]
    async fn a_description_is_screened_as_the_content_is() {
        let result = WriteTool(scope())
            .execute(
                json!({
                    "category": "fact",
                    "name": "Release contact",
                    "description": CONTACT,
                    "content": "Ship on Thursdays."
                }),
                &chat(),
            )
            .await
            .unwrap();

        assert_eq!(refusal(&result), Refusal::Identifier.message());
    }

    #[test]
    fn an_index_names_each_entry_in_the_word_the_model_passes_back() {
        let rendered = index(&[
            row(
                PREFERENCE_CATEGORY,
                PREFERENCES_TITLE,
                2,
                Some("How to work."),
            ),
            row(FACT_CATEGORY, "Deploy window", 1, None),
            row("memory-something-later", "Unknown", 1, None),
        ]);

        assert_eq!(
            rendered,
            "preference/Preferences, version 2. How to work.\nfact/Deploy window, version 1."
        );
        assert!(!rendered.contains(crate::db::memory::MEMORY_CATEGORY_PREFIX));
    }

    #[test]
    fn an_empty_index_says_so_rather_than_saying_nothing() {
        assert_eq!(index(&[]), NOTHING_REMEMBERED);
    }

    /// The rendered block sends the model here for what it left out, so an
    /// index that quietly stops at its bound would be the end of the trail.
    #[test]
    fn an_index_that_stops_at_its_bound_says_where_the_rest_is() {
        let listed = usize::try_from(MAX_FACTS).expect("the fact bound fits a count");
        let rows: Vec<MemoryIndexRow> = (0..=listed)
            .map(|number| row(FACT_CATEGORY, &format!("Fact {number:03}"), 1, None))
            .collect();

        let rendered = index(&rows);

        let named = rendered
            .lines()
            .filter(|line| line.starts_with("fact/"))
            .count();
        assert_eq!(named, listed, "{rendered}");
        assert!(
            rendered.contains(MEMORY_READ),
            "a list that stops has to say how to reach what it stopped before: {rendered}"
        );
        assert!(
            rendered.lines().count() > named,
            "the notice is a line of its own: {rendered}"
        );
        assert!(rendered.ends_with(&more_than_listed()), "{rendered}");
    }

    #[test]
    fn an_index_inside_its_bound_says_nothing_about_a_rest_there_is_none_of() {
        let rendered = index(&[row(FACT_CATEGORY, "Deploy window", 1, None)]);

        assert_eq!(rendered, "fact/Deploy window, version 1.");
    }

    fn row(category: &str, title: &str, version: i64, description: Option<&str>) -> MemoryIndexRow {
        MemoryIndexRow {
            title: title.to_string(),
            description: description.map(str::to_string),
            category: category.to_string(),
            version,
        }
    }

    /// One workspace and one person in it, thrown away afterwards: the
    /// database these tests run against is shared with other work.
    struct Fixture {
        pool: sqlx::PgPool,
        organization: Uuid,
        user: Uuid,
        scope: WorkspaceScope,
    }

    async fn fixture() -> Fixture {
        let pool = sqlx::PgPool::connect(
            &std::env::var("DATABASE_URL")
                .expect("the memory tool tests require a migrated PostgreSQL DATABASE_URL"),
        )
        .await
        .expect("DATABASE_URL must reach a migrated PostgreSQL");

        let organization: Uuid = sqlx::query_scalar(
            "INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id",
        )
        .bind("Memory tools")
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .expect("an organization");

        let workspace: Uuid = sqlx::query_scalar(
            "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(organization)
        .bind("Memory tools")
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .expect("a workspace");

        let user: Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(&pool)
        .await
        .expect("a user");

        let state = AppState::new(test_config(), pool.clone(), None);
        state.disable_mcp();

        Fixture {
            scope: WorkspaceScope {
                state,
                workspace_id: workspace,
                chat_id: Some(Uuid::new_v4()),
                user_id: user,
            },
            pool,
            organization,
            user,
        }
    }

    impl Fixture {
        async fn clean(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the workspace and everything in it goes");
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(self.user)
                .execute(&self.pool)
                .await
                .expect("the person goes");
        }
    }

    #[tokio::test]
    async fn a_write_is_reported_in_the_word_the_model_used() {
        let fixture = fixture().await;
        let write = WriteTool(fixture.scope.clone());

        let created = write
            .execute(
                json!({"category": "profile", "content": "Builds Zone."}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(message(&created), "Remembered: profile/Profile, version 1.");

        let replaced = write
            .execute(
                json!({"category": "profile", "content": "Builds Zone and Appwrite.", "version": 1}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(
            message(&replaced),
            "Updated: profile/Profile, now version 2."
        );
        assert!(
            !message(&replaced).contains(crate::db::memory::MEMORY_CATEGORY_PREFIX),
            "a success names the kind the way the table stores it"
        );

        fixture.clean().await;
    }

    #[tokio::test]
    async fn a_stale_version_hands_back_what_the_entry_says_now() {
        let fixture = fixture().await;
        let write = WriteTool(fixture.scope.clone());

        write
            .execute(
                json!({"category": "preference", "content": "Short answers."}),
                &chat(),
            )
            .await
            .unwrap();
        write
            .execute(
                json!({"category": "preference", "content": "Short answers, no preamble.", "version": 1}),
                &chat(),
            )
            .await
            .unwrap();

        let stale = write
            .execute(
                json!({"category": "preference", "content": "Long answers.", "version": 1}),
                &chat(),
            )
            .await
            .unwrap();

        assert_eq!(
            refusal(&stale),
            memory::conflict(
                MemoryCategory::Preference,
                PREFERENCES_TITLE,
                2,
                "Short answers, no preamble."
            )
        );
        assert!(
            refusal(&stale).contains("Short answers, no preamble."),
            "a conflict has to carry what the entry says now: {}",
            refusal(&stale)
        );

        fixture.clean().await;
    }

    #[tokio::test]
    async fn content_longer_than_one_entry_holds_is_refused_whole() {
        let fixture = fixture().await;
        let scope = &fixture.scope;

        let refused = WriteTool(scope.clone())
            .execute(
                json!({"category": "preference", "content": "x".repeat(MAX_ENTRY_CHARS + 1)}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(refusal(&refused), memory::too_long(MAX_ENTRY_CHARS));

        let read = ReadTool(scope.clone())
            .execute(json!({"category": "preference"}), &chat())
            .await
            .unwrap();
        assert_eq!(
            refusal(&read),
            memory::missing(MemoryCategory::Preference, PREFERENCES_TITLE),
            "an over-long write must leave nothing behind"
        );

        fixture.clean().await;
    }

    /// The name is checked before anything else, so a refusal that asks for
    /// briefer content is one the model cannot act on: every retry composes a
    /// shorter entry under the same name and gets the same string back.
    #[tokio::test]
    async fn a_name_longer_than_a_name_holds_is_refused_for_the_name() {
        let fixture = fixture().await;
        let scope = &fixture.scope;
        let name = "n".repeat(MAX_NAME_CHARS + 1);

        let refused = WriteTool(scope.clone())
            .execute(
                json!({
                    "category": "fact",
                    "name": name,
                    "description": "When we ship.",
                    "content": "Thursdays, after standup."
                }),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(refusal(&refused), memory::name_too_long(MAX_NAME_CHARS));

        let appended = AppendTool(scope.clone())
            .execute(
                json!({"category": "fact", "name": name, "content": "And Fridays."}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(refusal(&appended), memory::name_too_long(MAX_NAME_CHARS));

        fixture.clean().await;
    }

    #[tokio::test]
    async fn an_entry_is_indexed_read_added_to_and_forgotten() {
        let fixture = fixture().await;
        let scope = &fixture.scope;

        WriteTool(scope.clone())
            .execute(
                json!({
                    "category": "fact",
                    "name": "Deploy window",
                    "description": "When we ship.",
                    "content": "Thursdays, after standup."
                }),
                &chat(),
            )
            .await
            .unwrap();

        let listed = ListTool(scope.clone())
            .execute(json!({}), &chat())
            .await
            .unwrap();
        assert_eq!(
            message(&listed),
            "fact/Deploy window, version 1. When we ship."
        );

        let read = ReadTool(scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window"}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(
            message(&read),
            "fact/Deploy window, version 1. When we ship.\n\nThursdays, after standup."
        );

        let appended = AppendTool(scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window", "content": "Never on a Friday."}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(
            message(&appended),
            memory::appended(MemoryCategory::Fact, "Deploy window", 2)
        );

        let stale = DeleteTool(scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window", "version": 1}),
                &chat(),
            )
            .await
            .unwrap();
        assert!(
            refusal(&stale).contains("has changed since you read it"),
            "a delete at a stale version has to be refused: {}",
            refusal(&stale)
        );

        let forgotten = DeleteTool(scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window", "version": 2}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(message(&forgotten), "Forgotten: fact/Deploy window.");

        let after = ListTool(scope.clone())
            .execute(json!({}), &chat())
            .await
            .unwrap();
        assert_eq!(message(&after), NOTHING_REMEMBERED);

        fixture.clean().await;
    }

    #[tokio::test]
    async fn nothing_is_remembered_at_a_name_that_was_never_written() {
        let fixture = fixture().await;

        let read = ReadTool(fixture.scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window"}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(
            refusal(&read),
            memory::missing(MemoryCategory::Fact, "Deploy window")
        );

        let appended = AppendTool(fixture.scope.clone())
            .execute(
                json!({"category": "fact", "name": "Deploy window", "content": "Thursdays."}),
                &chat(),
            )
            .await
            .unwrap();
        assert_eq!(
            refusal(&appended),
            memory::missing(MemoryCategory::Fact, "Deploy window")
        );

        fixture.clean().await;
    }
}
