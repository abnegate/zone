//! A codex or claude child in a process group of its own, so stopping it stops everything it forked.

use std::io;
use std::path::Path;
use std::process::ExitStatus;

use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::timeout;
use tool_runner::executor::{GRACE_PERIOD, ProcessGroup};

use super::Error;

pub(super) struct Process {
    child: Child,
    group: Option<ProcessGroup>,
    reaped: bool,
}

impl Process {
    /// `command` must put the child in its own process group, as [`super::command`] does.
    pub(super) fn spawn(mut command: Command, executable: &Path) -> Result<Self, Error> {
        let child = command.spawn().map_err(|error| Error::Unavailable {
            executable: executable.display().to_string(),
            message: error.to_string(),
        })?;
        let group = child.id().map(ProcessGroup::new);
        Ok(Self {
            child,
            group,
            reaped: false,
        })
    }

    pub(super) fn stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub(super) fn stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait().await;
        self.reaped |= status.is_ok();
        status
    }

    /// Asks the whole group to exit, then kills whatever is still running once the grace period is
    /// up.
    pub(super) async fn stop(&mut self) {
        if self.reaped {
            return;
        }
        if let Some(group) = &self.group {
            let _ = group.terminate();
        }
        if !matches!(timeout(GRACE_PERIOD, self.child.wait()).await, Ok(Ok(_))) {
            if let Some(group) = &self.group {
                let _ = group.kill();
            }
            let _ = self.child.kill().await;
        }
        self.reaped = true;
    }
}

/// A caller that stops waiting -- a request dropped mid-sign-in -- leaves the child and all it
/// forked running unless dropping the handle ends them. Once the child is reaped its group id may
/// belong to someone else, so a reaped child's group is never signalled.
impl Drop for Process {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if let Some(group) = &self.group {
            let _ = group.kill();
        }
    }
}
