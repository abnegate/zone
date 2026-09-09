//! When and where this turn runs, who it runs for, and how long an answer runs.
//!
//! Every field renders at a fixed width. A preview and the generation that
//! follows it each build their own `Environment`, and the context estimate is
//! byte-length based, so two renders a few seconds apart must measure the same
//! or the preview stops predicting the request it is previewing. Only the
//! day name varies, and only across midnight.

use crate::agent::prompt::{Context, Verbosity};

const HEADING: &str = "Session context:";

/// Zero-padded throughout, with a numeric offset, so the width never moves.
const TIMESTAMP: &str = "%A %Y-%m-%d %H:%M:%S %:z";

const CLOCK: &str = "Work out every date, time and duration the user asks about from this clock \
     and this zone. Do not assume UTC, and do not reuse a date from training.";

const RUNTIME: &str = "You are on Zone's own runtime rather than the user's computer, so the \
     files, tools and software here are Zone's and none of them are on their machine.";

const SNAPSHOT: &str = "That, and any file listing already in this conversation, was captured as \
     the turn began and does not change while the turn runs. Read the current state with a tool \
     before you depend on either.";

const NAME: &str = "Never address anyone by a name they have not given you: a name lifted from an \
     email address, a handle or a commit author is a guess at who someone is, not a fact about \
     them. Identity here is for telling whose work is whose, and is never something to send on to \
     another service.";

const LOCATION: &str = "Give a location only when the request turns on one, and never volunteer \
     it. A location worked out from the network can be wrong.";

const BRIEF: &str = "brief: answer, then stop";

const STANDARD: &str = "standard: enough that whoever reads this run afterwards can follow it";

const DEFAULT: &str = "That is a default, and what the user asks for overrides it.";

const NOISE: &str = "This block arrives with every turn and has no bearing on most requests.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    let environment = context.environment;
    let mut lines = vec![
        HEADING.to_string(),
        format!(
            "- Now: {} ({}). {CLOCK}",
            environment.now.format(TIMESTAMP),
            environment.timezone
        ),
        format!(
            "- Working directory {} on {}. {RUNTIME}",
            environment.directory.display(),
            environment.platform
        ),
    ];
    if let Some(workspace) = &environment.workspace {
        lines.push(format!("- Workspace: {workspace}."));
    }
    if let Some(vcs) = &environment.vcs {
        lines.push(format!(
            "- Version control: branch {} at {}. {SNAPSHOT}",
            vcs.branch, vcs.head
        ));
    }
    if let Some(user) = &environment.user {
        lines.push(format!("- The user's display name is {user}. {NAME}"));
    }
    lines.push(format!("- {LOCATION}"));

    let effort = environment
        .effort
        .map(|effort| format!("Reasoning effort for this turn is {}. ", effort.as_str()))
        .unwrap_or_default();
    let length = match Verbosity::of(context.surface) {
        Verbosity::Brief => BRIEF,
        Verbosity::Standard => STANDARD,
    };
    lines.push(format!(
        "- {effort}Default answer length is {length}. {DEFAULT}"
    ));
    lines.push(format!("- {NOISE}"));

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ChatTools;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};
    use crate::agent::prompt::{Environment, Vcs};
    use chrono::DateTime;
    use std::path::PathBuf;
    use zone_core::llm::Effort;

    fn chat(environment: &Environment) -> String {
        let tools = ChatTools::empty();
        render(&chat_context(&tools, false, environment)).unwrap()
    }

    fn task(environment: &Environment) -> String {
        let tools = ChatTools::empty();
        render(&task_context(&tools, environment)).unwrap()
    }

    fn at(moment: &str) -> Environment {
        Environment::at(
            DateTime::parse_from_rfc3339(moment).unwrap(),
            "Pacific/Auckland",
            PathBuf::from("/srv/zone"),
        )
    }

    #[test]
    fn the_clock_line_carries_the_day_the_offset_and_the_zone() {
        let rendered = chat(&environment());

        assert!(
            rendered.contains("- Now: Wednesday 2026-09-09 09:30:00 +12:00 (Pacific/Auckland)."),
            "{rendered}"
        );
        assert!(rendered.contains("Do not assume UTC"), "{rendered}");
        assert!(
            rendered.contains("from this clock and this zone"),
            "{rendered}"
        );
    }

    #[test]
    fn the_place_line_names_the_directory_and_denies_it_is_the_users_machine() {
        let rendered = chat(&environment());

        assert!(
            rendered.contains(&format!(
                "- Working directory /srv/zone on {}.",
                std::env::consts::OS
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains("rather than the user's computer"),
            "{rendered}"
        );
    }

    #[test]
    fn a_checkout_baseline_renders_as_a_snapshot_that_covers_file_listings_too() {
        let with = environment().with_vcs(Vcs {
            branch: "main".into(),
            head: "83ff18b".into(),
        });
        let rendered = chat(&with);

        assert!(
            rendered.contains("- Version control: branch main at 83ff18b."),
            "{rendered}"
        );
        assert!(
            rendered.contains("any file listing already in this conversation"),
            "{rendered}"
        );
        assert!(
            rendered.contains("does not change while the turn runs"),
            "{rendered}"
        );
        assert!(
            !chat(&environment()).contains("Version control:"),
            "a chat turn has no checkout baseline"
        );
    }

    #[test]
    fn a_display_name_renders_with_the_rule_against_inventing_one() {
        let rendered = chat(&environment().with_user("Ari"));

        assert!(
            rendered.contains("- The user's display name is Ari."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Never address anyone by a name they have not given you"),
            "{rendered}"
        );
        assert!(
            rendered.contains("a handle or a commit author"),
            "{rendered}"
        );
        assert!(
            rendered.contains("never something to send on to another service"),
            "{rendered}"
        );
        assert!(
            !chat(&environment()).contains("display name"),
            "an unknown caller must not be named"
        );
    }

    #[test]
    fn a_workspace_name_renders_only_when_it_is_known() {
        assert!(chat(&environment().with_workspace("Zone")).contains("- Workspace: Zone."));
        assert!(!chat(&environment()).contains("- Workspace:"));
    }

    #[test]
    fn the_location_rule_and_the_every_turn_caveat_always_render() {
        for rendered in [chat(&environment()), task(&environment())] {
            assert!(
                rendered.contains("Give a location only when the request turns on one"),
                "{rendered}"
            );
            assert!(
                rendered.contains("never volunteer it"),
                "a location is never offered unasked: {rendered}"
            );
            assert!(
                rendered.contains("arrives with every turn and has no bearing on most requests"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn the_effort_line_names_the_resolved_level_and_is_silent_without_one() {
        let rendered = chat(&environment().with_effort(Effort::High));

        assert!(
            rendered.contains("Reasoning effort for this turn is high."),
            "{rendered}"
        );
        assert!(
            !chat(&environment()).contains("Reasoning effort"),
            "an unresolved effort states nothing"
        );
    }

    #[test]
    fn a_chat_answers_briefly_by_default_and_a_task_run_at_standard_length() {
        let brief = chat(&environment());
        assert!(
            brief.contains("Default answer length is brief: answer, then stop."),
            "{brief}"
        );
        assert!(
            brief.contains("what the user asks for overrides it"),
            "{brief}"
        );

        let standard = task(&environment());
        assert!(
            standard.contains("Default answer length is standard:"),
            "{standard}"
        );
        assert!(!standard.contains("length is brief"), "{standard}");
    }

    /// A preview and the generation after it each read their own clock, and the
    /// context estimate divides byte length, so the two renders must measure the
    /// same. Only the day name is allowed to move the width.
    #[test]
    fn two_instants_of_the_same_weekday_render_to_the_same_width() {
        let early = at("2026-01-04T05:06:07+00:00");
        let late = at("2026-11-15T23:59:59-08:00");

        assert_eq!(chat(&early).len(), chat(&late).len());

        let filled = |environment: Environment| {
            environment
                .with_vcs(Vcs {
                    branch: "main".into(),
                    head: "83ff18b".into(),
                })
                .with_user("Ari")
                .with_workspace("Zone")
                .with_effort(Effort::Medium)
        };
        assert_eq!(chat(&filled(early)).len(), chat(&filled(late)).len());
    }

    #[test]
    fn the_block_renders_on_both_surfaces_and_never_runs_two_bullets_together() {
        for rendered in [chat(&environment()), task(&environment())] {
            assert!(rendered.starts_with(HEADING), "{rendered}");
            assert!(!rendered.contains("\n\n"), "{rendered}");
            assert!(!rendered.ends_with('\n'), "{rendered}");
        }
    }
}
