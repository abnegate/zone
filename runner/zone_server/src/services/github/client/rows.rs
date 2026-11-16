use abnegate_vcs::CheckRun;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::WorkflowRun;

/// A commit's vetted CI rows, and whether they are every row GitHub holds.
#[derive(Debug, Clone)]
pub(crate) struct Rows {
    pub(crate) workflows: Vec<WorkflowRun>,
    pub(crate) checks: Vec<CheckRun>,
    pub(crate) statuses: Vec<CommitStatus>,
    pub(crate) complete: bool,
}
