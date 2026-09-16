//! The plan a background run submits before it changes anything, when the
//! task asks for one to be approved first.
//!
//! CC 1389-1604 prefers planning for an implementation task unless it is
//! simple, keeps the plan in a file, and puts approval through a dedicated
//! tool; CX 21 adds that a plan is not a stopping point. Here the plan is a
//! tool call rather than a file — a run's checkout is collected when the run
//! ends and a `PLAN.md` in it would be published with the change — and the
//! approval rides the machinery `ask_user` already has: the call parks the
//! run on one required question with the plan as its preview, the person
//! answers it from the console, and the answer arrives as the next user turn.
//! `submit_plan` is registered only for a task that requires approval, so the
//! prompt paragraph that teaches it renders only where it can be called.
//!
//! What the tool adds to a question of its own making is the record: the
//! plan is kept on the run (`task_runs.plan`) once approval was asked for,
//! so a reviewer can read what was agreed to after the question that carried
//! it has been answered and cleared.
use crate::agent::question::{self, Question};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

pub const SUBMIT_PLAN: &str = "submit_plan";

/// The header the approval question is asked under, which is also how the
/// worker tells this park from any other question a run asks.
pub const HEADER: &str = "Plan approval";
pub const APPROVE: &str = "Approve";
pub const REVISE: &str = "Revise";

/// A plan is read by a person before anything happens; past this it is the
/// work itself, written out.
pub const MAX_PLAN_CHARS: usize = 20_000;

const DESCRIPTION: &str = "Submit the plan for this run and wait for it to be approved. Write what you will change and \
    in what order, how you will check it, and what you are leaving out. Call it before you change \
    anything; your turn ends the moment you call it, and the answer arrives as the next message. \
    Approve means carry the plan out without asking again; Revise comes with what to change, so \
    change it and submit again.";

const ACKNOWLEDGEMENT: &str = "Plan submitted. The run pauses here until it is approved.";

/// What a run's attempts share about its plan. Approval is given to the
/// run, once: an attempt retried after a fault starts from it — unheld, and
/// handed the plan it was approved for — rather than asking the person
/// again for what they already answered.
#[derive(Default)]
pub struct Approval {
    plan: std::sync::Mutex<Option<String>>,
}

impl Approval {
    /// The plan the run's approval was given for, once it was.
    pub fn approved(&self) -> Option<String> {
        self.plan.lock().ok().and_then(|plan| plan.clone())
    }

    pub fn approve(&self, plan: &str) {
        if let Ok(mut held) = self.plan.lock() {
            *held = Some(plan.to_string());
        }
    }

    /// The paragraph a retried attempt is handed in place of the plan
    /// phase: the plan, and that it was approved.
    pub fn resumed(plan: &str) -> String {
        format!(
            "# Approved plan\n\nThis plan was approved earlier in this run, before the attempt \
             restarted. Carry it out as written, without submitting it again.\n\n{plan}"
        )
    }
}

/// The one question a submitted plan asks, in the shape `ask_user` asks it,
/// so the card, the store and the answer route need to know nothing new. It
/// is asked under a header `ask_user` is refused, so a question the run asks
/// for itself can never be read as a plan, however it is worded.
pub fn question(arguments: &str) -> Result<Vec<Question>, String> {
    let plan = plan(arguments)?;
    question::parse_reserved(
        &json!({
            "questions": [{
                "header": HEADER,
                "question": "Approve this plan? The run changes nothing until you do.",
                "options": [
                    {"label": APPROVE, "description": "Carry the plan out as written."},
                    {"label": REVISE, "description": "Send it back; say what to change under Other."}
                ],
                "preview": plan,
                "required": true
            }]
        })
        .to_string(),
    )
}

/// Whether the answer that resumed a run approved the plan it parked on:
/// the park was a plan — which only `submit_plan` can make, since the
/// header is reserved for it — and the rendered answer is Approve. The form
/// `question::render` gives one chosen option, so a Revise, or an Other
/// with what to change, leaves the hold in place.
pub fn approved(questions: &[Question], resume: &str) -> bool {
    submitted(questions).is_some() && resume.trim() == format!("{HEADER}: {APPROVE}")
}

/// The plan a parked question carries, when the park is a plan approval and
/// not a question the run asked for itself.
pub fn submitted(questions: &[Question]) -> Option<&str> {
    match questions {
        [only] if only.header == HEADER => only.preview.as_deref(),
        _ => None,
    }
}

fn plan(arguments: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(arguments)
        .map_err(|error| format!("`{SUBMIT_PLAN}` takes a JSON object with `plan`: {error}"))?;
    let plan = value
        .get("plan")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if plan.is_empty() {
        return Err(format!(
            "`{SUBMIT_PLAN}` needs a `plan`: what you will change, in what order, and how you will check it."
        ));
    }
    if plan.chars().count() > MAX_PLAN_CHARS {
        return Err(format!(
            "`{SUBMIT_PLAN}` holds a plan of at most {MAX_PLAN_CHARS} characters; this one is longer. Say \
             what you will do, not how every line will read."
        ));
    }
    Ok(plan.to_string())
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(Arc::new(SubmitPlanTool));
}

struct SubmitPlanTool;

#[async_trait]
impl Tool for SubmitPlanTool {
    fn name(&self) -> &str {
        SUBMIT_PLAN
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The plan, in prose or a short list: what changes, in what order, how it is checked, what is left out."
                }
            },
            "required": ["plan"]
        })
    }

    fn tier(&self) -> Tier {
        Tier::Read
    }

    fn ends_turn(&self) -> bool {
        true
    }

    /// Submitting costs nothing and waits for nothing here: the park is the
    /// event the loop yields once this returns, and the answer comes back as
    /// the next turn rather than into this call.
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(match plan(&params.to_string()) {
            Ok(_) => ToolResult::success(ACKNOWLEDGEMENT),
            Err(rejection) => ToolResult::error(rejection),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "1. Add the column.\n2. Thread it through the API.\n3. Test the route.";

    #[test]
    fn a_submitted_plan_asks_one_required_question_with_the_plan_as_its_preview() {
        let questions = question(&json!({"plan": PLAN}).to_string()).expect("a plan parks");
        assert_eq!(questions.len(), 1);
        let asked = &questions[0];
        assert_eq!(asked.header, HEADER);
        assert!(
            asked.required,
            "approval has no default; a run must not proceed on silence"
        );
        assert_eq!(asked.preview.as_deref(), Some(PLAN));
        assert_eq!(asked.choices[0].label, APPROVE);
        assert!(
            asked.choices[0].recommended,
            "approving is the recommended option"
        );
        assert_eq!(asked.choices[1].label, REVISE);
        assert!(
            asked.choices.last().is_some_and(|choice| choice.free_text),
            "revision notes need somewhere to go: {:?}",
            asked.choices
        );
        assert_eq!(submitted(&questions), Some(PLAN));
    }

    #[test]
    fn a_question_the_run_asked_for_itself_is_not_a_plan() {
        let asked = question::parse(
            r#"{"questions":[{"header":"Scope","question":"Which module?","options":[{"label":"A","description":"a"},{"label":"B","description":"b"}]}]}"#,
        )
        .unwrap();
        assert_eq!(submitted(&asked), None);
        let two = [
            asked[0].clone(),
            question(&json!({"plan": PLAN}).to_string())
                .unwrap()
                .remove(0),
        ];
        assert_eq!(submitted(&two), None, "a plan parks alone");
    }

    #[test]
    fn an_empty_or_oversized_plan_is_refused_with_the_reason() {
        let empty = question(r#"{"plan":"   "}"#).unwrap_err();
        assert!(empty.contains("needs a `plan`"), "{empty}");
        let long = json!({"plan": "x".repeat(MAX_PLAN_CHARS + 1)}).to_string();
        let refused = question(&long).unwrap_err();
        assert!(refused.contains("at most"), "{refused}");
        assert!(question("not json").is_err());
    }

    #[tokio::test]
    async fn the_tool_acknowledges_a_plan_and_names_what_is_wrong_with_one_it_refuses() {
        let tool = SubmitPlanTool;
        let context = ToolContext::default();
        let ok = tool.execute(json!({"plan": PLAN}), &context).await.unwrap();
        assert!(ok.success, "{ok:?}");
        assert!(
            ok.output.as_deref().unwrap_or_default().contains("pauses"),
            "{:?}",
            ok.output
        );
        let refused = tool.execute(json!({"plan": ""}), &context).await.unwrap();
        assert!(!refused.success);
        assert!(tool.ends_turn());
        assert_eq!(tool.tier(), Tier::Read);
    }

    /// Approve, rendered the way the run receives it, releases the hold;
    /// Revise and an Other do not, and neither does an answer to a question
    /// the run asked for itself.
    #[test]
    fn only_an_approve_rendered_as_the_run_receives_it_counts_as_approval() {
        use crate::agent::question::{Answer, OTHER_LABEL, render};
        let questions = question(&serde_json::json!({"plan": "1. Do it."}).to_string()).unwrap();
        let answer = |labels: &[&str], other: Option<&str>| Answer {
            header: HEADER.to_string(),
            labels: labels.iter().map(|label| label.to_string()).collect(),
            other: other.map(str::to_string),
        };
        let approve = render(&questions, &[answer(&[APPROVE], None)]).unwrap();
        assert!(approved(&questions, &approve), "{approve}");
        let revise = render(&questions, &[answer(&[REVISE], None)]).unwrap();
        assert!(!approved(&questions, &revise), "{revise}");
        let other = render(&questions, &[answer(&[OTHER_LABEL], Some("Skip the test"))]).unwrap();
        assert!(!approved(&questions, &other), "{other}");
        let own = crate::agent::question::parse(
            &serde_json::json!({"questions":[{"header":"Scope","question":"How far?","options":[{"label":"Approve","description":"All of it"},{"label":"Some","description":"Part"}]}]}).to_string(),
        )
        .unwrap();
        assert!(!approved(&own, "Scope: Approve"));
    }
}
