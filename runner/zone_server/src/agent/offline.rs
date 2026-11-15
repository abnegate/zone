//! The tools an offline chat may be given. Anything registered under another
//! name is withheld, so a tool added later reaches an offline chat only once
//! it is listed here.
//!
//! Every tool listed works on the server's own state, on the files and
//! commands the chat's network-denied context runs, or through Zone's model
//! infrastructure (embeddings, Ollama, ComfyUI). `list_models`, `get_model`,
//! `start_train` and `wait_for` can also reach a remote host and refuse those
//! calls themselves when the chat is offline.

use abnegate_agent::tool::job::TAIL_JOB;

use super::discovery::RECORD_DISCOVERY;
use super::memory::MEMORY_APPEND;
use super::memory::MEMORY_DELETE;
use super::memory::MEMORY_LIST;
use super::memory::MEMORY_READ;
use super::memory::MEMORY_WRITE;
use super::plan::SUBMIT_PLAN;
use super::question::ASK_USER;
use super::toolbox::LOAD_TOOLS;
use super::toolbox::SEARCH_TOOLS;
use super::wait::WAIT_FOR;

pub const TOOLS: [&str; 51] = [
    "apply_patch",
    "list_files",
    "read_file",
    "run_command",
    "run_shell",
    "search_code",
    "write_file",
    TAIL_JOB,
    ASK_USER,
    WAIT_FOR,
    SEARCH_TOOLS,
    LOAD_TOOLS,
    SUBMIT_PLAN,
    "read_chat_evidence",
    "search_knowledge",
    "search_chat_history",
    "list_sources",
    "list_projects",
    "list_documents",
    "read_document",
    "create_document",
    "update_document",
    RECORD_DISCOVERY,
    "list_tasks",
    "create_task",
    "update_task",
    "list_members",
    "list_chats",
    "send_message",
    "create_reminder",
    "list_reminders",
    "cancel_reminder",
    "get_task_run",
    "tail_task_log",
    MEMORY_LIST,
    MEMORY_READ,
    MEMORY_WRITE,
    MEMORY_APPEND,
    MEMORY_DELETE,
    "generate_image",
    "edit_image",
    "generate_audio",
    "list_models",
    "get_model",
    "get_model_install",
    "cancel_model_install",
    "delete_model",
    "list_train_bases",
    "get_train_job",
    "start_train",
    "dismiss_train",
];

/// Whether an offline chat may be given the tool registered as `name`.
pub fn allows(name: &str) -> bool {
    TOOLS.contains(&name)
}
