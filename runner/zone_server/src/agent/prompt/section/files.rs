//! Where the file and shell tools act, and what they may do there.

use crate::agent::ToolProfile;
use crate::agent::prompt::Context;

const HOST: &str = "run_shell, run_command, read_file, write_file, apply_patch, list_files and search_code act in \
             the server runtime with its process permissions. In Docker they access the container \
             and mounted paths, not the Docker host. Changes persist after the turn ends.\n\
             - Look before you change: read a file before rewriting it, and check what a \
             directory holds before writing into it.\n\
             - Prefer apply_patch for edits to existing files. Use write_file only to create a file or when the user asked for a full rewrite.\n\
             - Keep each command narrow and inspectable, prefer a dry run where one exists, and \
             bound a long log with max_output_chars.\n\
             - Do not delete, move or overwrite anything the user did not ask you to, and do not \
             touch anything outside what the request is about.\n\
             - Say what you changed on disk in your reply. The user sees the tool trace, but the \
             consequences are yours to explain.\n\
             - write_file, apply_patch, run_command and run_shell ";

const AUTO_APPROVED: &str = "run without waiting for confirmation in this chat.";

const APPROVAL_REQUIRED: &str = "wait for the user to approve before they run.";

const SANDBOX: &str = "read_file, write_file, apply_patch, list_files, search_code and run_command stay inside the \
             sandboxed working directory. Environment is allowlisted. There is no unrestricted shell.\n\
             - Prefer apply_patch for edits. Use write_file to create a file or when a full rewrite is required.\n\
             - Keep each command narrow and inspectable, and bound a long log with max_output_chars.";

/// The one place either surface is taught the convention. Seven schemas carry
/// the parameter and nothing validates their `required` array at dispatch, so
/// this sentence is what actually asks for it.
///
/// It is scoped to where the parameter exists rather than to what a call
/// changes. The workspace and document writes take no `reason` and set
/// `additionalProperties: false` over structs that deny unknown fields, so a
/// broader rule would talk a model into losing those calls before the write.
const REASON: &str = "Where a tool takes a reason, give one: a sentence on why, which the user reads when \
             reviewing or approving it.";

const READING: &str = "Read only the part of a file you need when you already know where it is, and do not read a \
             file back to check a write you just made: the tool result already said it applied. \
             Prefer plain-text formats over ones that inflate the context.";

const GIT: &str = "Git: interactive flags such as rebase -i are not supported here. Commit or push only when \
             the user asks, and branch first when the checkout is on the default branch. Do not \
             force-push, hard reset, or rewrite history unless the user asked for it. Write a pull \
             request body to a file and pass it by path so its newlines survive.";

pub(in crate::agent::prompt) fn render(context: &Context<'_>) -> Option<String> {
    if !context.tools.has("read_file") {
        return None;
    }
    if context.tools.profile() != ToolProfile::Chat {
        return Some(format!("{SANDBOX}\n- {REASON}\n- {READING}"));
    }
    let approval = if context.auto_approve {
        AUTO_APPROVED
    } else {
        APPROVAL_REQUIRED
    };
    Some(format!(
        "{HOST}{approval}\n- {REASON}\n- {READING}\n- {GIT}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ChatTools;
    use crate::agent::prompt::test_support::{chat_context, environment, task_context};

    fn chat(auto_approve: bool) -> String {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["read_file"], None);
        let environment = environment();
        render(&chat_context(&tools, auto_approve, &environment)).unwrap()
    }

    fn task() -> String {
        let tools = ChatTools::with_names(ToolProfile::Task, &["read_file"], None);
        let environment = environment();
        render(&task_context(&tools, &environment)).unwrap()
    }

    #[test]
    fn the_chat_profile_reaches_the_server_runtime_and_gates_on_approval() {
        let required = chat(false);
        assert!(required.contains("act in the server runtime"), "{required}");
        assert!(required.contains("Changes persist after the turn ends."));
        assert!(required.contains(APPROVAL_REQUIRED), "{required}");
        assert!(!required.contains(AUTO_APPROVED), "{required}");

        let automatic = chat(true);
        assert!(automatic.contains(AUTO_APPROVED), "{automatic}");
        assert!(!automatic.contains(APPROVAL_REQUIRED), "{automatic}");
    }

    #[test]
    fn the_task_profile_stays_in_the_sandbox_and_never_mentions_approval() {
        let rendered = task();

        assert!(rendered.starts_with("read_file, write_file"), "{rendered}");
        assert!(
            rendered.contains("There is no unrestricted shell."),
            "{rendered}"
        );
        assert!(!rendered.contains("run_shell,"), "{rendered}");
        assert!(!rendered.contains(APPROVAL_REQUIRED), "{rendered}");
    }

    #[test]
    fn git_is_only_driven_on_the_chat_surface_and_only_when_asked() {
        let rendered = chat(false);

        assert!(
            rendered.contains("interactive flags such as rebase -i are not supported here"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Commit or push only when the user asks"),
            "{rendered}"
        );
        assert!(
            rendered.contains("branch first when the checkout is on the default branch"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "Do not force-push, hard reset, or rewrite history unless the user asked for it."
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "Write a pull request body to a file and pass it by path so its newlines survive."
            ),
            "{rendered}"
        );

        assert!(
            !task().contains("Git:"),
            "the task surface owns its own git rule"
        );
        assert!(!task().contains("force-push"), "{}", task());
    }

    #[test]
    fn both_surfaces_are_told_to_read_narrowly_and_not_to_read_a_write_back() {
        for rendered in [chat(false), chat(true), task()] {
            assert!(
                rendered.contains(
                    "Read only the part of a file you need when you already know where it is"
                ),
                "{rendered}"
            );
            assert!(
                rendered.contains(
                    "do not read a file back to check a write you just made: the tool result \
                     already said it applied"
                ),
                "{rendered}"
            );
            assert!(
                rendered.contains("Prefer plain-text formats over ones that inflate the context."),
                "{rendered}"
            );
        }
    }

    /// The parameter is advertised as required and nothing enforces that, so
    /// the only thing asking for it is this sentence. A sandboxed run's
    /// run_command changes the checkout exactly as the host one changes disk,
    /// so it is taught on both surfaces rather than only where a person waits.
    #[test]
    fn both_surfaces_are_asked_to_say_why_a_changing_call_is_needed() {
        for rendered in [chat(false), chat(true), task()] {
            assert!(
                rendered.contains("Where a tool takes a reason, give one"),
                "{rendered}"
            );
            assert!(
                rendered.contains(
                    "a sentence on why, which the user reads when reviewing or approving it"
                ),
                "{rendered}"
            );
        }
    }

    /// The mutating tools that take no `reason` — create_task, create_document,
    /// create_reminder and their siblings — set `additionalProperties: false`
    /// over structs that deny unknown fields, so a model told to reason on
    /// everything it changes loses the call before the write runs. The rule has
    /// to key off the parameter being offered, never off the call mutating.
    #[test]
    fn the_reason_rule_keys_off_the_parameter_not_off_changing_something() {
        for rendered in [chat(false), chat(true), task()] {
            assert!(
                !rendered.contains("any call that changes something"),
                "{rendered}"
            );
        }
    }

    #[test]
    fn both_surfaces_can_bound_a_long_command_log() {
        for rendered in [chat(false), chat(true), task()] {
            assert!(
                rendered.contains("Keep each command narrow and inspectable"),
                "{rendered}"
            );
            assert!(
                rendered.contains("bound a long log with max_output_chars"),
                "{rendered}"
            );
        }
    }

    /// A dry run is a host affordance. The sandbox keeps the narrowness rule
    /// and the new bound without inheriting the rest of the chat bullet.
    #[test]
    fn a_dry_run_is_only_suggested_where_an_unrestricted_shell_exists() {
        assert!(
            chat(false).contains("prefer a dry run where one exists"),
            "{}",
            chat(false)
        );
        assert!(!task().contains("dry run"), "{}", task());
    }

    #[test]
    fn a_catalog_without_file_tools_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["web_search"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
