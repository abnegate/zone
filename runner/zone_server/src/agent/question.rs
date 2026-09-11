//! Ask the person a structured question, and end the turn on the spot.
//!
//! A model that asks in prose has already spent its turn guessing what comes
//! after the answer. This tool makes the question the last thing the turn
//! does: the console renders a card, the turn stops, and what the person
//! chooses arrives as the next user message. A task run parks instead, waiting
//! on the registry at the bottom of this file.

use async_trait::async_trait;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

pub const ASK_USER: &str = "ask_user";
pub const MAX_QUESTIONS: usize = 4;
pub const MIN_CHOICES: usize = 2;
pub const MAX_CHOICES: usize = 4;
pub const OTHER_LABEL: &str = "Other";
pub const OTHER_DESCRIPTION: &str = "Something else — type it below.";

/// What the model reads back from its own call.
///
/// It is not a result: nothing has been answered yet. It exists so a model
/// that sees a tool result for every other call is not left inferring what
/// happened to this one.
const ACKNOWLEDGEMENT: &str = "The question card was shown. Your turn ends here; the answer arrives as the next user message.";

const DESCRIPTION: &str = "Ask the user to decide something you cannot decide for them. Send one \
    to four questions, each with two to four options, and put the option you recommend first. \
    Your turn ends the moment you call this, so call it alone and do not plan past the answer: \
    what the user chooses arrives as the next message.";

/// One option on a question card.
///
/// `recommended` and `free_text` are the server's, not the model's: the first
/// option is the recommendation by position, and the free-text option is
/// appended here so every card has one whether or not the model thought of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub description: String,
    pub recommended: bool,
    pub free_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub header: String,
    pub question: String,
    pub choices: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub multi_select: bool,
    pub required: bool,
}

/// What came back for one question, keyed by the header that asked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub header: String,
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
}

#[derive(Deserialize)]
struct Request {
    questions: Vec<Asked>,
}

#[derive(Deserialize)]
struct Asked {
    header: String,
    question: String,
    options: Vec<Offered>,
    #[serde(default)]
    preview: Option<String>,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    required: bool,
}

#[derive(Deserialize)]
struct Offered {
    label: String,
    description: String,
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

fn same(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

/// Turn the model's arguments into cards, or into the sentence that tells it
/// what to fix.
///
/// Every rejection names the offending question, because a model handed only
/// "invalid arguments" re-sends the same call.
pub fn parse(arguments: &str) -> Result<Vec<Question>, String> {
    let trimmed = arguments.trim();
    let value: Value = if trimmed.is_empty() {
        json!({})
    } else {
        serde_json::from_str(trimmed)
            .map_err(|error| format!("`{ASK_USER}` arguments were not valid JSON: {error}."))?
    };
    questions(value)
}

fn questions(value: Value) -> Result<Vec<Question>, String> {
    let request: Request = serde_json::from_value(value).map_err(|error| {
        format!(
            "`{ASK_USER}` arguments did not match the schema: {error}. Each question needs a \
             `header`, a `question`, and an `options` array of objects with `label` and \
             `description`."
        )
    })?;

    if request.questions.is_empty() {
        return Err(format!(
            "`{ASK_USER}` needs between 1 and {MAX_QUESTIONS} questions, and none were given."
        ));
    }
    if request.questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "`{ASK_USER}` takes at most {MAX_QUESTIONS} questions, and {} were given. Ask only \
             the ones you cannot proceed without.",
            request.questions.len()
        ));
    }

    let mut parsed: Vec<Question> = Vec::with_capacity(request.questions.len());
    for (index, asked) in request.questions.into_iter().enumerate() {
        let position = index + 1;
        if blank(&asked.header) {
            return Err(format!(
                "Question {position} has a blank `header`. Give each question a short header the \
                 user can tell it apart by."
            ));
        }
        let header = asked.header.trim().to_string();
        if blank(&asked.question) {
            return Err(format!("Question '{header}' has a blank `question`."));
        }
        if parsed.iter().any(|earlier| same(&earlier.header, &header)) {
            return Err(format!(
                "Two questions share the header '{header}'. Headers identify a question in the \
                 answer, so each one has to be unique."
            ));
        }
        if asked.options.len() < MIN_CHOICES || asked.options.len() > MAX_CHOICES {
            return Err(format!(
                "Question '{header}' has {} options; give it between {MIN_CHOICES} and \
                 {MAX_CHOICES}, with the one you recommend first.",
                asked.options.len()
            ));
        }

        let mut choices: Vec<Choice> = Vec::with_capacity(asked.options.len() + 1);
        for offered in asked.options {
            if blank(&offered.label) {
                return Err(format!(
                    "Question '{header}' has an option with a blank `label`."
                ));
            }
            let label = offered.label.trim().to_string();
            if blank(&offered.description) {
                return Err(format!(
                    "Option '{label}' of question '{header}' has a blank `description`. Say what \
                     choosing it means."
                ));
            }
            if same(&label, OTHER_LABEL) {
                return Err(format!(
                    "Question '{header}' offers an option labelled '{label}'. The card adds an \
                     '{OTHER_LABEL}' option itself, so offer only the concrete choices."
                ));
            }
            if choices.iter().any(|earlier| same(&earlier.label, &label)) {
                return Err(format!(
                    "Question '{header}' offers two options labelled '{label}'. An answer names \
                     the option it chose, so each label has to be unique."
                ));
            }
            choices.push(Choice {
                recommended: choices.is_empty(),
                label,
                description: offered.description.trim().to_string(),
                free_text: false,
            });
        }
        choices.push(Choice {
            label: OTHER_LABEL.to_string(),
            description: OTHER_DESCRIPTION.to_string(),
            recommended: false,
            free_text: true,
        });

        parsed.push(Question {
            header,
            question: asked.question.trim().to_string(),
            choices,
            preview: asked
                .preview
                .map(|preview| preview.trim().to_string())
                .filter(|preview| !preview.is_empty()),
            multi_select: asked.multi_select,
            required: asked.required,
        });
    }
    Ok(parsed)
}

/// The answer as the model will read it, one line per question answered.
///
/// Question order, not answer order: the model asked in a sequence and reads
/// the reply against it.
pub fn render(questions: &[Question], answers: &[Answer]) -> Result<String, String> {
    validate(questions, answers)?;
    Ok(questions
        .iter()
        .filter_map(|question| {
            answers
                .iter()
                .find(|answer| answer.header == question.header)
                .filter(|answer| !answer.labels.is_empty())
                .map(|answer| line(question, answer))
        })
        .collect::<Vec<String>>()
        .join("\n"))
}

fn line(question: &Question, answer: &Answer) -> String {
    let chosen = answer
        .labels
        .iter()
        .map(|label| {
            if label == OTHER_LABEL {
                format!(
                    "{OTHER_LABEL}: {}",
                    answer.other.as_deref().unwrap_or_default().trim()
                )
            } else {
                label.clone()
            }
        })
        .collect::<Vec<String>>()
        .join(", ");
    format!("{}: {chosen}", question.header)
}

fn validate(questions: &[Question], answers: &[Answer]) -> Result<(), String> {
    let mut answered: HashSet<&str> = HashSet::new();
    for answer in answers {
        let Some(question) = questions
            .iter()
            .find(|question| question.header == answer.header)
        else {
            return Err(format!("No question is headed '{}'.", answer.header));
        };
        if !answered.insert(answer.header.as_str()) {
            return Err(format!("Question '{}' was answered twice.", answer.header));
        }
        if answer.labels.len() > 1 && !question.multi_select {
            return Err(format!(
                "Question '{}' takes one option, and {} were chosen.",
                answer.header,
                answer.labels.len()
            ));
        }
        let mut chosen: HashSet<&str> = HashSet::new();
        for label in &answer.labels {
            if !question.choices.iter().any(|choice| &choice.label == label) {
                return Err(format!(
                    "Question '{}' has no option labelled '{label}'.",
                    answer.header
                ));
            }
            if !chosen.insert(label.as_str()) {
                return Err(format!(
                    "Question '{}' chose the option '{label}' twice.",
                    answer.header
                ));
            }
        }
        let picked_other = answer.labels.iter().any(|label| label == OTHER_LABEL);
        match (&answer.other, picked_other) {
            (Some(text), true) if !blank(text) => {}
            (_, true) => {
                return Err(format!(
                    "Question '{}' chose '{OTHER_LABEL}' without saying what it is.",
                    answer.header
                ));
            }
            (Some(_), false) => {
                return Err(format!(
                    "Question '{}' carries free text without choosing '{OTHER_LABEL}'.",
                    answer.header
                ));
            }
            (None, false) => {}
        }
    }
    for question in questions.iter().filter(|question| question.required) {
        if !answers
            .iter()
            .any(|answer| answer.header == question.header && !answer.labels.is_empty())
        {
            return Err(format!(
                "Question '{}' has to be answered.",
                question.header
            ));
        }
    }
    Ok(())
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(Arc::new(AskUserTool));
}

struct AskUserTool;

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        ASK_USER
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_QUESTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "header": {"type": "string"},
                            "question": {"type": "string"},
                            "options": {
                                "type": "array",
                                "minItems": MIN_CHOICES,
                                "maxItems": MAX_CHOICES,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string"},
                                        "description": {"type": "string"}
                                    },
                                    "required": ["label", "description"]
                                }
                            },
                            "preview": {"type": "string"},
                            "multi_select": {"type": "boolean"},
                            "required": {"type": "boolean"}
                        },
                        "required": ["header", "question", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    fn tier(&self) -> Tier {
        Tier::Read
    }

    fn ends_turn(&self) -> bool {
        true
    }

    /// Asking costs nothing and waits for nothing: the card is delivered by
    /// the event the loop yields once this returns, and the person answers
    /// into the next turn rather than into this call.
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(match questions(params) {
            Ok(_) => ToolResult::success(ACKNOWLEDGEMENT),
            Err(rejection) => ToolResult::error(rejection),
        })
    }
}

static WAITING: Lazy<DashMap<Uuid, oneshot::Sender<Vec<Answer>>>> = Lazy::new(DashMap::new);

/// A task run's claim on the next answer for it.
///
/// Dropping it withdraws the claim, so a cancelled or panicking run leaves
/// nothing behind for a later answer to resolve.
pub struct Waiter {
    run: Uuid,
    receiver: Option<oneshot::Receiver<Vec<Answer>>>,
}

impl Drop for Waiter {
    fn drop(&mut self) {
        forget(self.run);
    }
}

/// Claim the answer before the card is published.
///
/// A fast answer can arrive before the caller has finished emitting the
/// question, so the claim has to exist first or the answer resolves nothing.
pub fn expect(run: Uuid) -> Waiter {
    let (sender, receiver) = oneshot::channel();
    WAITING.insert(run, sender);
    Waiter {
        run,
        receiver: Some(receiver),
    }
}

/// Wait on a claim made by [`expect`], up to `window` if one is given.
pub async fn awaited(mut waiter: Waiter, window: Option<Duration>) -> Option<Vec<Answer>> {
    let receiver = waiter.receiver.take()?;
    match window {
        Some(window) => tokio::select! {
            received = receiver => received.ok(),
            _ = tokio::time::sleep(window) => None,
        },
        None => receiver.await.ok(),
    }
}

/// Resolve a waiting run. Returns whether anything was waiting.
pub fn answer(run: Uuid, answers: Vec<Answer>) -> bool {
    WAITING
        .remove(&run)
        .is_some_and(|(_, sender)| sender.send(answers).is_ok())
}

pub fn forget(run: Uuid) {
    WAITING.remove(&run);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(questions: Value) -> String {
        json!({ "questions": questions }).to_string()
    }

    fn one(options: Value) -> String {
        arguments(json!([{"header":"Scope","question":"How far?","options":options}]))
    }

    fn options() -> Value {
        json!([
            {"label":"Backfill","description":"Rewrite the existing rows."},
            {"label":"Forward only","description":"Leave the existing rows alone."}
        ])
    }

    fn fixture() -> Vec<Question> {
        parse(&arguments(json!([
            {
                "header": "Scope",
                "question": "How far back should this run?",
                "options": [
                    {"label":"Backfill","description":"Rewrite the existing rows."},
                    {"label":"Forward only","description":"Leave the existing rows alone."}
                ],
                "required": true
            },
            {
                "header": "Rollout",
                "question": "Where should it land first?",
                "options": [
                    {"label":"All at once","description":"Every workspace together."},
                    {"label":"Per workspace","description":"One workspace at a time."}
                ],
                "multi_select": true
            },
            {
                "header": "Notify",
                "question": "Should members be told?",
                "options": [
                    {"label":"Yes","description":"Post to each workspace."},
                    {"label":"No","description":"Say nothing."}
                ]
            }
        ])))
        .expect("the fixture parses")
    }

    #[test]
    fn the_first_option_is_the_recommendation() {
        let parsed = parse(&one(options())).unwrap();
        assert!(parsed[0].choices[0].recommended);
        assert_eq!(parsed[0].choices[0].label, "Backfill");
        assert!(
            parsed[0].choices[1..]
                .iter()
                .all(|choice| !choice.recommended),
            "only the first option is recommended"
        );
    }

    #[test]
    fn every_question_gains_exactly_one_free_text_choice() {
        for parsed in fixture() {
            let free: Vec<&Choice> = parsed
                .choices
                .iter()
                .filter(|choice| choice.free_text)
                .collect();
            assert_eq!(free.len(), 1, "{}", parsed.header);
            assert_eq!(free[0].label, OTHER_LABEL);
            assert_eq!(free[0].description, OTHER_DESCRIPTION);
            assert!(!free[0].recommended);
            assert_eq!(
                parsed.choices.last().map(|choice| choice.free_text),
                Some(true),
                "the free-text choice is appended last"
            );
        }
    }

    #[test]
    fn multi_select_and_required_default_to_false() {
        let parsed = parse(&one(options())).unwrap();
        assert!(!parsed[0].multi_select);
        assert!(!parsed[0].required);
        assert_eq!(parsed[0].preview, None);
    }

    #[test]
    fn flags_and_preview_are_carried_through() {
        let parsed = parse(&arguments(json!([{
            "header":"Scope",
            "question":"How far?",
            "options": options(),
            "preview": "  UPDATE rows SET tenant = $1  ",
            "multi_select": true,
            "required": true
        }])))
        .unwrap();
        assert!(parsed[0].multi_select);
        assert!(parsed[0].required);
        assert_eq!(
            parsed[0].preview.as_deref(),
            Some("UPDATE rows SET tenant = $1")
        );
    }

    #[test]
    fn a_question_round_trips_and_omits_an_absent_preview() {
        let parsed = parse(&one(options())).unwrap();
        let encoded = serde_json::to_value(&parsed[0]).unwrap();
        assert!(
            encoded.get("preview").is_none(),
            "an absent preview must not be written back as null"
        );
        assert_eq!(encoded["multi_select"], false);
        assert_eq!(encoded["choices"][2]["free_text"], true);
        let decoded: Question = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, parsed[0]);
    }

    #[test]
    fn no_questions_is_rejected() {
        let rejection = parse(&arguments(json!([]))).unwrap_err();
        assert!(rejection.contains("none were given"), "{rejection}");
    }

    #[test]
    fn more_than_four_questions_is_rejected() {
        let asked: Vec<Value> = (0..=MAX_QUESTIONS)
            .map(|index| {
                json!({"header":format!("Q{index}"),"question":"How far?","options":options()})
            })
            .collect();
        let rejection = parse(&arguments(json!(asked))).unwrap_err();
        assert!(rejection.contains("at most 4 questions"), "{rejection}");
    }

    #[test]
    fn fewer_than_two_options_is_rejected() {
        let rejection = parse(&one(
            json!([{"label":"Backfill","description":"Rewrite the rows."}]),
        ))
        .unwrap_err();
        assert!(rejection.contains("has 1 options"), "{rejection}");
    }

    #[test]
    fn more_than_four_options_is_rejected() {
        let offered: Vec<Value> = (0..=MAX_CHOICES)
            .map(|index| json!({"label":format!("Option {index}"),"description":"A choice."}))
            .collect();
        let rejection = parse(&one(json!(offered))).unwrap_err();
        assert!(rejection.contains("has 5 options"), "{rejection}");
    }

    #[test]
    fn a_blank_header_is_rejected() {
        let rejection = parse(&arguments(
            json!([{"header":"  ","question":"How far?","options":options()}]),
        ))
        .unwrap_err();
        assert!(rejection.contains("blank `header`"), "{rejection}");
    }

    #[test]
    fn a_blank_question_is_rejected() {
        let rejection = parse(&arguments(
            json!([{"header":"Scope","question":" ","options":options()}]),
        ))
        .unwrap_err();
        assert!(rejection.contains("blank `question`"), "{rejection}");
    }

    #[test]
    fn a_blank_label_is_rejected() {
        let rejection = parse(&one(json!([
            {"label":"","description":"Rewrite the rows."},
            {"label":"Forward only","description":"Leave them alone."}
        ])))
        .unwrap_err();
        assert!(rejection.contains("blank `label`"), "{rejection}");
    }

    #[test]
    fn a_blank_description_is_rejected() {
        let rejection = parse(&one(json!([
            {"label":"Backfill","description":"  "},
            {"label":"Forward only","description":"Leave them alone."}
        ])))
        .unwrap_err();
        assert!(rejection.contains("blank `description`"), "{rejection}");
    }

    #[test]
    fn a_duplicate_label_within_a_question_is_rejected() {
        let rejection = parse(&one(json!([
            {"label":"Backfill","description":"Rewrite the rows."},
            {"label":" backfill ","description":"Rewrite them again."}
        ])))
        .unwrap_err();
        assert!(rejection.contains("two options labelled"), "{rejection}");
    }

    /// The card appends its own free-text option, so a model-supplied one
    /// would be the duplicate that breaks matching an answer to a choice.
    #[test]
    fn a_model_supplied_other_option_is_rejected() {
        let rejection = parse(&one(json!([
            {"label":"Backfill","description":"Rewrite the rows."},
            {"label":"Other","description":"Something else."}
        ])))
        .unwrap_err();
        assert!(rejection.contains("adds an 'Other' option"), "{rejection}");
    }

    #[test]
    fn a_duplicate_header_within_the_call_is_rejected() {
        let rejection = parse(&arguments(json!([
            {"header":"Scope","question":"How far?","options":options()},
            {"header":" scope ","question":"How far again?","options":options()}
        ])))
        .unwrap_err();
        assert!(rejection.contains("share the header"), "{rejection}");
    }

    #[test]
    fn arguments_that_are_not_the_schema_are_rejected_with_the_shape() {
        for malformed in [
            json!({}).to_string(),
            json!({"questions":"Scope"}).to_string(),
            arguments(json!([{"question":"How far?","options":options()}])),
            "{not json".to_string(),
            String::new(),
        ] {
            let rejection = parse(&malformed).unwrap_err();
            assert!(rejection.starts_with("`ask_user` arguments"), "{rejection}");
        }
    }

    #[test]
    fn the_rendered_answer_reads_in_question_order_with_free_text_in_place() {
        let questions = fixture();
        let answers = vec![
            Answer {
                header: "Scope".to_string(),
                labels: vec!["Backfill".to_string()],
                other: None,
            },
            Answer {
                header: "Rollout".to_string(),
                labels: vec!["Per workspace".to_string(), OTHER_LABEL.to_string()],
                other: Some("Two workspaces first, then the rest".to_string()),
            },
        ];
        assert_eq!(
            render(&questions, &answers).unwrap(),
            "Scope: Backfill\nRollout: Per workspace, Other: Two workspaces first, then the rest"
        );
    }

    #[test]
    fn an_unanswered_optional_question_contributes_no_line() {
        let questions = fixture();
        let answers = vec![
            Answer {
                header: "Notify".to_string(),
                labels: vec!["No".to_string()],
                other: None,
            },
            Answer {
                header: "Scope".to_string(),
                labels: vec!["Backfill".to_string()],
                other: None,
            },
        ];
        assert_eq!(
            render(&questions, &answers).unwrap(),
            "Scope: Backfill\nNotify: No"
        );
    }

    #[test]
    fn free_text_without_choosing_other_is_rejected() {
        let questions = fixture();
        let answers = vec![Answer {
            header: "Scope".to_string(),
            labels: vec!["Backfill".to_string()],
            other: Some("Only the last month".to_string()),
        }];
        let rejection = render(&questions, &answers).unwrap_err();
        assert!(
            rejection.contains("without choosing 'Other'"),
            "{rejection}"
        );
    }

    #[test]
    fn choosing_other_without_free_text_is_rejected() {
        let questions = fixture();
        for other in [None, Some(String::new()), Some("   ".to_string())] {
            let answers = vec![Answer {
                header: "Scope".to_string(),
                labels: vec![OTHER_LABEL.to_string()],
                other,
            }];
            let rejection = render(&questions, &answers).unwrap_err();
            assert!(
                rejection.contains("without saying what it is"),
                "{rejection}"
            );
        }
    }

    #[test]
    fn an_answer_no_question_asked_for_is_rejected() {
        let rejection = render(
            &fixture(),
            &[Answer {
                header: "Budget".to_string(),
                labels: vec!["Backfill".to_string()],
                other: None,
            }],
        )
        .unwrap_err();
        assert!(rejection.contains("No question is headed"), "{rejection}");
    }

    #[test]
    fn an_option_the_question_never_offered_is_rejected() {
        let rejection = render(
            &fixture(),
            &[Answer {
                header: "Scope".to_string(),
                labels: vec!["Everything".to_string()],
                other: None,
            }],
        )
        .unwrap_err();
        assert!(rejection.contains("no option labelled"), "{rejection}");
    }

    #[test]
    fn several_options_on_a_single_select_question_are_rejected() {
        let rejection = render(
            &fixture(),
            &[Answer {
                header: "Scope".to_string(),
                labels: vec!["Backfill".to_string(), "Forward only".to_string()],
                other: None,
            }],
        )
        .unwrap_err();
        assert!(rejection.contains("takes one option"), "{rejection}");
    }

    #[test]
    fn an_unanswered_required_question_is_rejected() {
        let rejection = render(&fixture(), &[]).unwrap_err();
        assert!(rejection.contains("has to be answered"), "{rejection}");
    }

    #[test]
    fn asking_is_a_read_that_ends_the_turn() {
        assert_eq!(AskUserTool.name(), ASK_USER);
        assert_eq!(AskUserTool.tier(), Tier::Read);
        assert!(AskUserTool.ends_turn());
        assert_eq!(AskUserTool.preview(&json!({})), None);
    }

    /// Ending the turn is this tool's alone. A second one would silently
    /// truncate turns wherever it was called.
    #[test]
    fn no_other_catalog_tool_ends_the_turn() {
        for registry in [
            ToolRegistry::with_defaults(),
            ToolRegistry::with_host_tools(),
        ] {
            for name in registry.names() {
                assert_eq!(registry.ends_turn(name), Some(false), "{name}");
            }
        }
        let mut registry = ToolRegistry::with_host_tools();
        register(&mut registry);
        assert_eq!(registry.ends_turn(ASK_USER), Some(true));
        assert_eq!(registry.tier(ASK_USER), Some(Tier::Read));
        assert_eq!(registry.ends_turn("no_such_tool"), None);
    }

    #[test]
    fn the_schema_is_the_one_the_console_and_the_model_agreed_on() {
        assert_eq!(
            AskUserTool.parameters_schema(),
            json!({"type":"object","properties":{"questions":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"object","properties":{"header":{"type":"string"},"question":{"type":"string"},"options":{"type":"array","minItems":2,"maxItems":4,"items":{"type":"object","properties":{"label":{"type":"string"},"description":{"type":"string"}},"required":["label","description"]}},"preview":{"type":"string"},"multi_select":{"type":"boolean"},"required":{"type":"boolean"}},"required":["header","question","options"]}}},"required":["questions"]})
        );
    }

    #[tokio::test]
    async fn a_valid_call_acknowledges_without_waiting() {
        let result = AskUserTool
            .execute(
                serde_json::from_str(&one(options())).unwrap(),
                &ToolContext::default(),
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some(ACKNOWLEDGEMENT));
    }

    #[tokio::test]
    async fn a_rejected_call_fails_without_ending_anything() {
        let result = AskUserTool
            .execute(json!({"questions":[]}), &ToolContext::default())
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("none were given"));
    }

    #[tokio::test]
    async fn an_answer_unblocks_the_waiter() {
        let run = Uuid::new_v4();
        let waiter = expect(run);
        let given = vec![Answer {
            header: "Scope".to_string(),
            labels: vec!["Backfill".to_string()],
            other: None,
        }];
        let expected = given.clone();
        let waiting = tokio::spawn(async move { awaited(waiter, None).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(answer(run, given));
        assert_eq!(waiting.await.unwrap(), Some(expected));
    }

    #[tokio::test(start_paused = true)]
    async fn a_window_that_elapses_gives_up_on_the_answer() {
        let run = Uuid::new_v4();
        let waiter = expect(run);
        assert_eq!(awaited(waiter, Some(Duration::from_secs(600))).await, None);
        assert!(
            !answer(run, Vec::new()),
            "an elapsed wait leaves nothing to answer"
        );
    }

    #[test]
    fn dropping_a_waiter_withdraws_the_claim() {
        let run = Uuid::new_v4();
        drop(expect(run));
        assert!(!answer(run, Vec::new()));
    }

    #[test]
    fn an_unclaimed_run_has_no_answer_to_deliver() {
        assert!(!answer(Uuid::new_v4(), Vec::new()));
        forget(Uuid::new_v4());
    }
}
