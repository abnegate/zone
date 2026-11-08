use std::collections::HashMap;

use abnegate_agent_cli::{StepStatus, Subagent, SubagentStep};

/// Everything one burst said about one subagent: its fields, each the latest
/// heard, and the steps it touched, each once, in the order first heard of.
pub(super) struct Pending {
    pub(super) update: Subagent,
    pub(super) steps: Vec<SubagentStep>,
    positions: HashMap<String, usize>,
}

impl Pending {
    pub(super) fn new(mut update: Subagent) -> Self {
        let step = update.step.take();
        let mut pending = Self {
            update,
            steps: Vec::new(),
            positions: HashMap::new(),
        };
        pending.step(step);
        pending
    }

    pub(super) fn fold(&mut self, mut later: Subagent) {
        fn newer<T>(earlier: &mut Option<T>, later: Option<T>) {
            if later.is_some() {
                *earlier = later;
            }
        }
        self.step(later.step.take());
        let earlier = &mut self.update;
        newer(&mut earlier.description, later.description);
        newer(&mut earlier.kind, later.kind);
        newer(&mut earlier.model, later.model);
        newer(&mut earlier.background, later.background);
        newer(&mut earlier.status, later.status);
        newer(&mut earlier.tool_uses, later.tool_uses);
        newer(&mut earlier.duration, later.duration);
        newer(&mut earlier.tokens, later.tokens);
        newer(&mut earlier.last_tool, later.last_tool);
        newer(&mut earlier.summary, later.summary);
        newer(&mut earlier.error, later.error);
    }

    /// A settled step never runs again, as its stored copy never does.
    fn step(&mut self, step: Option<SubagentStep>) {
        let Some(step) = step else {
            return;
        };
        match self.positions.get(&step.id) {
            Some(&position) => {
                let known = &mut self.steps[position];
                if known.status == StepStatus::Running || step.status != StepStatus::Running {
                    *known = step;
                }
            }
            None => {
                self.positions.insert(step.id.clone(), self.steps.len());
                self.steps.push(step);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, status: StepStatus) -> SubagentStep {
        SubagentStep::new(id, "Bash", status)
    }

    fn stepped(step: SubagentStep) -> Subagent {
        Subagent::new("toolu_01Agent").with_step(step)
    }

    #[test]
    fn a_burst_keeps_each_step_once_in_the_order_first_heard_of() {
        let mut pending = Pending::new(stepped(step("first", StepStatus::Running)));

        pending.fold(stepped(step("second", StepStatus::Running)));
        pending.fold(stepped(step("first", StepStatus::Succeeded)));

        assert_eq!(
            pending.steps,
            [
                step("first", StepStatus::Succeeded),
                step("second", StepStatus::Running),
            ]
        );
        assert_eq!(pending.update.step, None);
    }

    #[test]
    fn a_settled_step_never_runs_again_within_a_burst() {
        let mut pending = Pending::new(stepped(step("first", StepStatus::Failed)));

        pending.fold(stepped(step("first", StepStatus::Running)));

        assert_eq!(pending.steps, [step("first", StepStatus::Failed)]);
    }

    #[test]
    fn a_later_field_replaces_an_earlier_and_a_missing_one_keeps_it() {
        let mut pending = Pending::new(
            Subagent::new("toolu_01Agent")
                .with_description("Survey")
                .with_tool_uses(1),
        );

        pending.fold(Subagent::new("toolu_01Agent").with_tool_uses(2));

        assert_eq!(pending.update.description.as_deref(), Some("Survey"));
        assert_eq!(pending.update.tool_uses, Some(2));
    }
}
