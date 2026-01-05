//! Process group management for clean process tree termination.
//!
//! On Unix systems, we create a new process group for each spawned command,
//! allowing us to send signals to the entire process tree when cancelling.

use crate::error::ExecutorError;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::time::Duration;
use tokio::time::sleep;

/// A handle to a process group for signal management.
#[derive(Debug, Clone)]
pub struct ProcessGroup {
    /// Process group ID (same as the leader process PID)
    pgid: i32,
}

impl ProcessGroup {
    /// Create a new process group handle from a process ID.
    ///
    /// The process should have been started with `process_group(0)` to create
    /// a new process group with itself as the leader.
    pub fn new(pid: u32) -> Self {
        Self { pgid: pid as i32 }
    }

    /// Get the process group ID
    pub fn pgid(&self) -> i32 {
        self.pgid
    }

    /// Send SIGTERM to the entire process group.
    ///
    /// This will attempt to gracefully terminate all processes in the group.
    pub fn terminate(&self) -> Result<(), ExecutorError> {
        // Negative PID signals the entire process group
        match kill(Pid::from_raw(-self.pgid), Signal::SIGTERM) {
            Ok(_) => Ok(()),
            Err(nix::errno::Errno::ESRCH) => {
                // Process/group doesn't exist anymore, that's fine
                Ok(())
            }
            Err(e) => Err(ExecutorError::ProcessGroupFailed(format!(
                "Failed to send SIGTERM to process group {}: {}",
                self.pgid, e
            ))),
        }
    }

    /// Send SIGKILL to the entire process group.
    ///
    /// This forcefully kills all processes in the group.
    pub fn kill(&self) -> Result<(), ExecutorError> {
        match kill(Pid::from_raw(-self.pgid), Signal::SIGKILL) {
            Ok(_) => Ok(()),
            Err(nix::errno::Errno::ESRCH) => {
                // Process/group doesn't exist anymore, that's fine
                Ok(())
            }
            Err(e) => Err(ExecutorError::ProcessGroupFailed(format!(
                "Failed to send SIGKILL to process group {}: {}",
                self.pgid, e
            ))),
        }
    }

    /// Perform a graceful shutdown: SIGTERM, wait, then SIGKILL if needed.
    ///
    /// This first sends SIGTERM and waits for the grace period, then sends
    /// SIGKILL if processes are still running.
    pub async fn graceful_kill(&self, grace_period: Duration) -> Result<(), ExecutorError> {
        // Send SIGTERM first
        self.terminate()?;

        // Wait for grace period
        sleep(grace_period).await;

        // Follow up with SIGKILL (best effort, may already be dead)
        let _ = self.kill();

        Ok(())
    }

    /// Check if the process group is still running.
    ///
    /// Returns true if any process in the group is still alive.
    pub fn is_alive(&self) -> bool {
        // Sending signal 0 checks if the process exists without actually sending a signal
        kill(Pid::from_raw(-self.pgid), None).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_group_new() {
        let pg = ProcessGroup::new(12345);
        assert_eq!(pg.pgid(), 12345);
    }

    #[test]
    fn test_terminate_nonexistent_group() {
        // Using a very high PID that likely doesn't exist
        let pg = ProcessGroup::new(999999);
        // Should not error - ESRCH is handled gracefully
        assert!(pg.terminate().is_ok());
    }

    #[test]
    fn test_kill_nonexistent_group() {
        let pg = ProcessGroup::new(999999);
        assert!(pg.kill().is_ok());
    }

    #[test]
    fn test_is_alive_nonexistent() {
        let pg = ProcessGroup::new(999999);
        assert!(!pg.is_alive());
    }
}
