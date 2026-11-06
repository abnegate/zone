//! What one line of a coding agent's output means.

use abnegate_llm::{Limit, ToolCall, Usage, Window};
use abnegate_secret::sanitize;
use serde_json::Value;

use super::occupancy::Occupancy;
use super::output::Output;
use super::settings::Toolset;
use super::subagent::Subagent;

/// What separates two blocks of an agent's answer.
const PARAGRAPH_BREAK: &str = "\n\n";

/// The arguments that say what a call of an agent's own tool is about, the
/// likeliest first.
const SUBJECTS: [&str; 8] = [
    "command",
    "file_path",
    "pattern",
    "path",
    "query",
    "url",
    "description",
    "prompt",
];

/// How much of a call's subject its line shows.
const SUBJECT_CHARS: usize = 80;

/// A normalised event, whichever agent produced it.
///
/// A turn a usage limit refused is [`AgentEvent::Limited`] rather than
/// [`AgentEvent::Failed`], so whoever routes turns between sign-ins can tell
/// an exhausted account from a broken turn without reading prose. Its message
/// is still the agent's own wording, word for word as a failure would carry
/// it, because the task worker's retry policy reads that wording.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A whole block of the answer, never a fragment of one: neither agent is
    /// asked to stream partial messages.
    Text(String),
    Tool(ToolCall),
    Output(Output),
    Usage(Usage),
    /// How much of a usage window the account has spent, reported while the
    /// turn goes on.
    Window(Window),
    Limited(Limit),
    /// The id of the session the agent runs this turn under, as it announced
    /// it: the one a later turn resumes.
    Session(String),
    /// The model's context window and how much of it the turn filled.
    Occupancy(Occupancy),
    /// News of a subagent the agent started for itself. It goes to the turn's
    /// observer rather than the reader: a subagent runs on after the main
    /// answer, so it is shown beside the answer, not in it. A turn nothing
    /// observes hands it on, and lists the subagent's own calls as reasoning.
    Subagent(Subagent),
    Failed(String),
    Finished {
        finish_reason: Option<String>,
    },
}

impl AgentEvent {
    /// Whether this event ends the run, successfully or not.
    ///
    /// An agent that exits zero without emitting one of these never finished
    /// its turn, and treating that as success would hand the caller a silently
    /// truncated answer.
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Failed(_) | Self::Limited(_) | Self::Finished { .. }
        )
    }
}

/// Keeps the blocks of an agent's answer apart.
///
/// An agent writes a block between each round of tools, every one ending as a
/// sentence does. Run together they read as one paragraph with no space after
/// each full stop.
#[derive(Debug, Default)]
pub(super) struct Paragraphs {
    spoken: bool,
}

impl Paragraphs {
    pub(super) fn separated(&mut self, event: AgentEvent) -> AgentEvent {
        let AgentEvent::Text(text) = event else {
            return event;
        };
        if text.trim().is_empty() {
            return AgentEvent::Text(text);
        }
        if std::mem::replace(&mut self.spoken, true) {
            return AgentEvent::Text(format!("{PARAGRAPH_BREAK}{text}"));
        }
        AgentEvent::Text(text)
    }
}

/// The markdown list item a reader is shown for a tool the agent reached for.
///
/// A call of zone's own tools has none: it arrives at zone over MCP, and the
/// turn shows it as zone's own card, so it would otherwise be shown twice.
pub fn activity(call: &ToolCall) -> Option<String> {
    listed(
        &call.function.name,
        subject(&call.function.arguments).as_deref(),
    )
}

/// [`activity`]'s line for a tool `name` used on `subject`.
pub(super) fn listed(name: &str, subject: Option<&str>) -> Option<String> {
    if name.starts_with(&Toolset::qualified(Toolset::SERVER, "")) {
        return None;
    }
    Some(match subject {
        Some(subject) => format!("- {name} {}\n", code_span(subject)),
        None => format!("- {name}\n"),
    })
}

/// The first line of what a call is about, cut short once it is clean: a token
/// cut in half no longer looks like a token, and a carriage return left in
/// would end the line a code span has to stay on.
pub(crate) fn subject(arguments: &str) -> Option<String> {
    let arguments: Value = serde_json::from_str(arguments).ok()?;
    let text = SUBJECTS.iter().find_map(|key| {
        arguments
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
    })?;
    let text = sanitize(text);
    let line = text.lines().next()?.trim_end();
    let shown: String = line.chars().take(SUBJECT_CHARS).collect();
    if shown.len() == text.len() {
        Some(shown)
    } else {
        Some(format!("{shown}\u{2026}"))
    }
}

/// `text` as inline code, fenced by more backticks than it holds in a row so
/// that none of it can close the span and be read as markdown.
fn code_span(text: &str) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let padding = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{padding}{text}{padding}{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::SubagentStatus;

    fn call(name: &str, arguments: &str) -> ToolCall {
        ToolCall::function("toolu_01", name, arguments)
    }

    #[test]
    fn an_agents_own_tool_is_listed_with_what_it_was_used_on() {
        assert_eq!(
            activity(&call("Bash", r#"{"command":"git status"}"#)).as_deref(),
            Some("- Bash `git status`\n")
        );
        assert_eq!(
            activity(&call(
                "ToolSearch",
                r#"{"query":"select:Read","max_results":1}"#
            ))
            .as_deref(),
            Some("- ToolSearch `select:Read`\n")
        );
        assert_eq!(
            activity(&call(
                "command_execution",
                r#"{"command":"/bin/zsh -lc ls"}"#
            ))
            .as_deref(),
            Some("- command_execution `/bin/zsh -lc ls`\n")
        );
    }

    #[test]
    fn a_tool_with_nothing_to_show_is_listed_by_name() {
        for arguments in ["{}", "not json", r#"{"command":"  "}"#, r#"{"todos":[]}"#] {
            assert_eq!(
                activity(&call("TodoWrite", arguments)).as_deref(),
                Some("- TodoWrite\n"),
                "{arguments}"
            );
        }
    }

    #[test]
    fn zones_own_tools_are_left_to_the_card_zone_shows_for_them() {
        assert_eq!(activity(&call("mcp__zone__search_code", "{}")), None);
    }

    #[test]
    fn another_servers_tool_is_not_mistaken_for_zones() {
        assert_eq!(
            activity(&call("mcp__zoneish__search", "{}")).as_deref(),
            Some("- mcp__zoneish__search\n")
        );
    }

    #[test]
    fn a_long_or_multiline_subject_is_cut_short() {
        let long = "x".repeat(SUBJECT_CHARS * 2);
        let shown = activity(&call(
            "Bash",
            &serde_json::json!({ "command": long }).to_string(),
        ))
        .expect("a line");
        assert_eq!(
            shown,
            format!("- Bash `{}\u{2026}`\n", "x".repeat(SUBJECT_CHARS))
        );

        let script = "cd /w\ncargo test";
        assert_eq!(
            activity(&call(
                "Bash",
                &serde_json::json!({ "command": script }).to_string()
            ))
            .as_deref(),
            Some("- Bash `cd /w\u{2026}`\n")
        );
    }

    #[test]
    fn a_secret_in_a_subject_is_redacted_before_it_is_cut() {
        let token = format!("ghp_{}", "a".repeat(36));
        let padded = format!(
            "{}export GITHUB_TOKEN={token}",
            "x".repeat(SUBJECT_CHARS - 40)
        );
        let shown = activity(&call(
            "Bash",
            &serde_json::json!({ "command": padded }).to_string(),
        ))
        .expect("a line");

        assert!(!shown.contains("ghp_"), "{shown}");
    }

    #[test]
    fn a_carriage_return_cannot_end_the_line_a_subject_stays_on() {
        let shown = activity(&call(
            "Bash",
            &serde_json::json!({ "command": "ls\r# ![x](https://evil.example/)" }).to_string(),
        ))
        .expect("a line");

        assert_eq!(shown, "- Bash `ls\u{2026}`\n");
    }

    #[test]
    fn a_subject_holding_backticks_cannot_close_its_span() {
        assert_eq!(code_span("echo `date`"), "`` echo `date` ``");
        assert_eq!(code_span("a ``b`` c"), "```a ``b`` c```");
        assert_eq!(code_span("plain"), "`plain`");
    }

    #[test]
    fn the_blocks_of_an_answer_are_kept_apart() {
        let mut paragraphs = Paragraphs::default();
        let spoken: Vec<AgentEvent> = [
            AgentEvent::Session("6f1".to_string()),
            AgentEvent::Text("  ".to_string()),
            AgentEvent::Text("I'll look first.".to_string()),
            AgentEvent::Usage(Usage::new(1, 1)),
            AgentEvent::Text("Found it.".to_string()),
        ]
        .into_iter()
        .map(|event| paragraphs.separated(event))
        .collect();

        let words: Vec<&str> = spoken
            .iter()
            .filter_map(|event| match event {
                AgentEvent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, ["  ", "I'll look first.", "\n\nFound it."]);
    }

    #[test]
    fn only_a_result_ends_the_run() {
        assert!(!AgentEvent::Text("hello".to_string()).terminal());
        assert!(!AgentEvent::Usage(Usage::new(1, 1)).terminal());
        assert!(!AgentEvent::Window(Window::new("five_hour").with_used_percent(43.0)).terminal());
        assert!(!AgentEvent::Session("6f1".to_string()).terminal());
        assert!(
            !AgentEvent::Subagent(Subagent {
                status: Some(SubagentStatus::Completed),
                ..Subagent::new("toolu_01Agent")
            })
            .terminal()
        );
        assert!(AgentEvent::Failed("nope".to_string()).terminal());
        assert!(AgentEvent::Limited(Limit::new("You've hit your session limit")).terminal());
        assert!(
            AgentEvent::Finished {
                finish_reason: None
            }
            .terminal()
        );
    }
}
