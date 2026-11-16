//! Whether this host can keep an offline chat's commands off the network.
//!
//! An offline chat runs every command with [`Network::Denied`], which needs
//! seccomp and Landlock on Linux or the sandbox library on macOS. A host
//! without them fails to start the command rather than run it with a
//! network, so every command an offline chat runs there fails.

use std::process::Stdio;

use abnegate_exec::Network;

pub const UNENFORCED: &str = "This host cannot deny a command the network, so every command an \
                              offline chat runs will fail to start. Linux needs seccomp and \
                              Landlock, macOS its sandbox.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDenial {
    Enforced,
    Unenforced,
}

impl NetworkDenial {
    /// Start a trivial command denied the network and see whether it runs.
    pub async fn probe() -> Self {
        let status = Network::Denied
            .command("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        match status {
            Ok(status) if status.success() => Self::Enforced,
            Ok(status) => {
                tracing::debug!(%status, "A network-denied command did not succeed");
                Self::Unenforced
            }
            Err(error) => {
                tracing::debug!(%error, "A network-denied command did not start");
                Self::Unenforced
            }
        }
    }

    pub fn is_enforced(self) -> bool {
        self == Self::Enforced
    }

    pub fn announce(self) {
        match self {
            Self::Enforced => tracing::info!("Offline chats' commands are denied the network"),
            Self::Unenforced => tracing::warn!("{UNENFORCED}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_probe_agrees_with_abnegate_on_this_host() {
        let enforced = abnegate_exec::testing::network_denial_enforced().await;

        assert_eq!(NetworkDenial::probe().await.is_enforced(), enforced);
    }
}
