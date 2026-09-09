//! Where the file and shell tools act, and what they may do there.

use crate::agent::ToolProfile;
use crate::agent::prompt::Context;

const HOST: &str = "run_shell, run_command, read_file, write_file, apply_patch, list_files and search_code act in \
             the server runtime with its process permissions. In Docker they access the container \
             and mounted paths, not the Docker host. Changes persist after the turn ends.\n\
             - Look before you change: read a file before rewriting it, and check what a \
             directory holds before writing into it.\n\
             - Prefer apply_patch for edits to existing files. Use write_file only to create a file or when the user asked for a full rewrite.\n\
             - Keep each command narrow and inspectable, and prefer a dry run where one exists.\n\
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
             - Keep each command narrow and inspectable.";

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
        return Some(format!("{SANDBOX}\n- {READING}"));
    }
    let approval = if context.auto_approve {
        AUTO_APPROVED
    } else {
        APPROVAL_REQUIRED
    };
    Some(format!("{HOST}{approval}\n- {READING}\n- {GIT}"))
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

    #[test]
    fn a_catalog_without_file_tools_renders_nothing() {
        let tools = ChatTools::with_names(ToolProfile::Chat, &["web_search"], None);
        let environment = environment();
        assert!(render(&chat_context(&tools, false, &environment)).is_none());
    }
}
