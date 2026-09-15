//! The tools that fetch other tools.
//!
//! A chat profile carries forty-odd tools plus whatever MCP adds, and every
//! one of their schemas is sent on every round of every turn. On a 32k
//! window that is the single largest fixed cost in the context, and most of it
//! is never used: a turn that answers a question about a document does not need
//! the schema for cancelling a reminder.
//!
//! So the set is split. A core set is always present, and the rest are listed
//! by name and one line of purpose — cheap enough to keep in front of the model
//! at all times, so it knows what exists — and their schemas arrive only when
//! asked for. [`SEARCH_TOOLS`] finds them by keyword and [`LOAD_TOOLS`] brings
//! them in.
//!
//! The cost of the trade is real and worth naming: the registry sorts
//! definitions so the tools prefix is byte-identical between turns and a local
//! server can reuse its prompt cache. A load changes that prefix, so the round
//! after one pays a cache miss. That is the argument for a generous core set
//! rather than a minimal one — the saving should come from tools a turn was
//! never going to touch, not from making every turn fetch what it always needs.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use zone_core::tools::{Tool, ToolContext, ToolError, ToolResult};

pub const SEARCH_TOOLS: &str = "search_tools";
pub const LOAD_TOOLS: &str = "load_tools";

/// How many matches a search returns before it starts hiding them.
///
/// A search that answers with the whole catalog has spent the context the
/// catalog was deferred to save.
const MAX_MATCHES: usize = 10;

/// One deferred tool, as the catalog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub purpose: String,
}

const MAX_PURPOSE: usize = 160;

/// The first sentence of a tool's description, which is what the catalog shows.
///
/// Taken from the description the tool already declares rather than from a
/// second list written beside it, because two lists drift and only one of them
/// is the one the model is handed when the schema finally arrives.
///
/// Whitespace collapses before the sentence is found, because these
/// descriptions are written as wrapped string literals: a stop followed by a
/// newline rather than a space is the common case, and looking for ". " alone
/// took the whole description as one sentence and deferred nothing.
pub fn purpose(description: &str) -> String {
    let collapsed = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let end = collapsed
        .match_indices(". ")
        .find(|(at, _)| ends_a_sentence(&collapsed[..*at]))
        .map(|(at, _)| at + 1)
        .unwrap_or(collapsed.len());
    let line = &collapsed[..end];
    if line.chars().count() <= MAX_PURPOSE {
        return line.to_string();
    }
    let cut: String = line.chars().take(MAX_PURPOSE).collect();
    let trimmed = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{trimmed}\u{2026}")
}

/// Whether a full stop after this text closes a sentence rather than an
/// abbreviation. "e.g" and "i.e" end in a lone letter; a sentence does not.
fn ends_a_sentence(before: &str) -> bool {
    before
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .is_none_or(|word| word.chars().count() > 1)
}

/// What is deferred and what has been asked for, shared between the tool set
/// and the two tools that reach into it.
///
/// The catalog is filled once, after assembly, because the tools are
/// registered before the registry is complete and a tool cannot list its own
/// siblings until they are all there. The loaded set is a `Mutex` rather than
/// anything threaded through the loop because `execute` is handed `&self`: a
/// tool that changes which tools exist has to do it from behind a shared
/// reference.
#[derive(Debug, Default)]
pub struct Toolbox {
    catalog: OnceLock<Vec<Listed>>,
    loaded: Mutex<HashSet<String>>,
}

impl Toolbox {
    /// Record what is deferred. Called once, after the registry is complete.
    pub fn publish(&self, listed: Vec<Listed>) {
        let _ = self.catalog.set(listed);
    }

    /// Every deferred tool, whether or not it has since been loaded.
    pub fn catalog(&self) -> &[Listed] {
        self.catalog.get().map_or(&[], Vec::as_slice)
    }

    /// The deferred tools still waiting to be asked for.
    pub fn unloaded(&self) -> Vec<&Listed> {
        let loaded = self.loaded();
        self.catalog()
            .iter()
            .filter(|listed| !loaded.contains(&listed.name))
            .collect()
    }

    pub fn loaded(&self) -> HashSet<String> {
        self.loaded
            .lock()
            .map(|set| set.clone())
            .unwrap_or_default()
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded
            .lock()
            .map(|set| set.contains(name))
            .unwrap_or(false)
    }

    /// Take in the named tools, answering with what happened to each.
    ///
    /// A poisoned lock fails rather than answering: reporting every name as
    /// unknown would tell the model a tool it can see listed does not exist,
    /// and it would stop asking for the one thing that could have worked.
    fn take(&self, names: &[String]) -> Result<Taken, ToolError> {
        let mut taken = Taken::default();
        let known: HashSet<&str> = self
            .catalog()
            .iter()
            .map(|listed| listed.name.as_str())
            .collect();
        let Ok(mut loaded) = self.loaded.lock() else {
            return Err(ToolError::Execution(
                "The tool catalog is unavailable for this turn. The tools already in front of                  you still work."
                    .into(),
            ));
        };
        for name in names {
            if !known.contains(name.as_str()) {
                taken.unknown.push(name.clone());
            } else if !loaded.insert(name.clone()) {
                taken.already.push(name.clone());
            } else {
                taken.accepted.push(name.clone());
            }
        }
        Ok(taken)
    }
}

#[derive(Default)]
struct Taken {
    accepted: Vec<String>,
    already: Vec<String>,
    unknown: Vec<String>,
}

const SEARCH_DESCRIPTION: &str = "Find a deferred tool by what it does. Most tools are listed by \
    name and purpose in your instructions but their schemas are not loaded; this searches those \
    listings and returns the closest matches. Search for the action you want to take, not for \
    \"what tools are available\" — \"cancel a reminder\" finds something, \"tools\" does not. \
    Finding a tool does not load it: call load_tools with the names you want.";

const LOAD_DESCRIPTION: &str = "Load the schemas for deferred tools so you can call them. Pass the \
    exact names, from your instructions or from search_tools. The schemas appear on your next \
    round, so call this and then stop — do not try to call the tool in the same message. A tool \
    whose schema you can already see needs no loading.";

pub struct SearchToolsTool(pub Arc<Toolbox>);
pub struct LoadToolsTool(pub Arc<Toolbox>);

#[derive(Deserialize)]
struct Query {
    query: String,
}

#[derive(Deserialize)]
struct Names {
    names: Vec<String>,
}

#[async_trait]
impl Tool for SearchToolsTool {
    fn name(&self) -> &str {
        SEARCH_TOOLS
    }

    fn description(&self) -> &str {
        SEARCH_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "description": "What you are trying to do, in a few words."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let Query { query } = serde_json::from_value(params)
            .map_err(|_| ToolError::InvalidParams("Provide a nonblank query".into()))?;
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Err(ToolError::InvalidParams("Provide a nonblank query".into()));
        }
        let terms: Vec<&str> = needle.split_whitespace().collect();
        let mut matches: Vec<&Listed> = self
            .0
            .catalog()
            .iter()
            .filter(|listed| {
                let haystack = format!("{} {}", listed.name, listed.purpose).to_lowercase();
                terms.iter().any(|term| haystack.contains(term))
            })
            .collect();
        // Most terms matched first, so a two-word query puts the tool that
        // answers both above the one that happens to share a common word.
        matches.sort_by_key(|listed| {
            let haystack = format!("{} {}", listed.name, listed.purpose).to_lowercase();
            std::cmp::Reverse(terms.iter().filter(|term| haystack.contains(*term)).count())
        });
        if matches.is_empty() {
            return Ok(ToolResult::success(format!(
                "No deferred tool matches \"{query}\". Every tool whose schema you can already \
                 see is loaded; the rest are listed by name in your instructions."
            )));
        }
        let hidden = matches.len().saturating_sub(MAX_MATCHES);
        let mut lines: Vec<String> = matches
            .iter()
            .take(MAX_MATCHES)
            .map(|listed| {
                let loaded = if self.0.is_loaded(&listed.name) {
                    " (already loaded)"
                } else {
                    ""
                };
                format!("{}{loaded}: {}", listed.name, listed.purpose)
            })
            .collect();
        if hidden > 0 {
            lines.push(format!(
                "{hidden} further match{} not shown; narrow the query if none of these is right.",
                if hidden == 1 { "" } else { "es" }
            ));
        }
        Ok(ToolResult::success(format!(
            "{}\n\nCall load_tools with the names you want.",
            lines.join("\n")
        )))
    }
}

#[async_trait]
impl Tool for LoadToolsTool {
    fn name(&self) -> &str {
        LOAD_TOOLS
    }

    fn description(&self) -> &str {
        LOAD_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "names": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string"},
                    "description": "Exact tool names to load."
                }
            },
            "required": ["names"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let Names { names } = serde_json::from_value(params)
            .map_err(|_| ToolError::InvalidParams("Provide names as an array of strings".into()))?;
        if names.is_empty() {
            return Err(ToolError::InvalidParams(
                "Provide at least one tool name".into(),
            ));
        }
        let taken = self.0.take(&names)?;
        let mut said = Vec::new();
        if !taken.accepted.is_empty() {
            said.push(format!(
                "Loaded {}. Their schemas are in your next round — call them there, not in this \
                 message.",
                taken.accepted.join(", ")
            ));
        }
        if !taken.already.is_empty() {
            said.push(format!(
                "Already loaded: {}. Call them directly.",
                taken.already.join(", ")
            ));
        }
        if !taken.unknown.is_empty() {
            said.push(format!(
                "No deferred tool is named {}. A tool whose schema you can already see does not \
                 need loading; otherwise find the name with search_tools.",
                taken.unknown.join(", ")
            ));
        }
        Ok(ToolResult::success(said.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog shows one line, and it comes from the description the tool
    /// already declares — so a tool whose description changes cannot end up
    /// advertising something it no longer does.
    #[test]
    fn a_purpose_is_the_first_sentence_of_the_description_it_already_has() {
        assert_eq!(
            purpose("Cancel a reminder. Takes its id, which list_reminders shows."),
            "Cancel a reminder."
        );
        assert_eq!(purpose("One sentence only."), "One sentence only.");
        assert_eq!(purpose("No full stop at all"), "No full stop at all");
        // A description wrapped over several lines is one line in the catalog:
        // the catalog's whole value is being cheap enough to always carry.
        assert_eq!(
            purpose("Send a message.\n   Continues here."),
            "Send a message."
        );
        assert_eq!(
            purpose("Wrapped across\n    lines with no stop"),
            "Wrapped across lines with no stop"
        );
        // An abbreviation does not end a sentence, or half the catalog would
        // read "Repeat it, e.g."
        assert_eq!(
            purpose("Repeat it, e.g. every Monday. Then stop."),
            "Repeat it, e.g. every Monday."
        );
        // And one sentence can still run long, so the line is bounded.
        let long = format!("{} and on it goes. Second.", "word ".repeat(60));
        let cut = purpose(&long);
        assert!(cut.chars().count() <= MAX_PURPOSE + 1, "{cut}");
        assert!(cut.ends_with('\u{2026}'), "{cut}");
    }

    /// Loading is the only thing that changes what the model can call, so it
    /// answers precisely: what it took, what it already had, and what does not
    /// exist — a model told "done" for a name that was never there would go on
    /// to call it and get a not-found from the dispatcher instead.
    #[tokio::test]
    async fn loading_says_which_names_it_took_and_which_it_did_not_know() {
        let toolbox = Arc::new(Toolbox::default());
        toolbox.publish(vec![
            Listed {
                name: "cancel_reminder".into(),
                purpose: "Stop a schedule.".into(),
            },
            Listed {
                name: "list_chats".into(),
                purpose: "List the chats.".into(),
            },
        ]);

        let taken = toolbox
            .take(&["cancel_reminder".into(), "nonesuch".into()])
            .expect("an unpoisoned toolbox loads");
        assert_eq!(taken.accepted, vec!["cancel_reminder".to_string()]);
        assert_eq!(taken.unknown, vec!["nonesuch".to_string()]);
        assert!(taken.already.is_empty());
        assert!(toolbox.is_loaded("cancel_reminder"));

        // Asked for twice, it is not taken twice: the answer distinguishes the
        // two so a model does not read a repeat as a fresh load.
        let again = toolbox
            .take(&["cancel_reminder".into()])
            .expect("an unpoisoned toolbox loads");
        assert!(again.accepted.is_empty());
        assert_eq!(again.already, vec!["cancel_reminder".to_string()]);

        // And what is still deferred shrinks as things are taken, which is what
        // the instructions list from.
        let waiting: Vec<&str> = toolbox
            .unloaded()
            .iter()
            .map(|listed| listed.name.as_str())
            .collect();
        assert_eq!(waiting, vec!["list_chats"]);
    }
}
