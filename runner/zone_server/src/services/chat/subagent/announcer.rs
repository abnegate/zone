use abnegate_agent_cli::SubagentStep;
use async_trait::async_trait;

use super::ChatSubagent;

/// Tells a chat's readers of each change a turn's tracker stored.
#[async_trait]
pub trait Announcer: Send + Sync {
    /// `steps` holds only the steps this write changed.
    async fn announce(&self, subagent: ChatSubagent, steps: Vec<SubagentStep>);
}

#[cfg(test)]
#[async_trait]
impl Announcer for tokio::sync::mpsc::UnboundedSender<(ChatSubagent, Vec<SubagentStep>)> {
    async fn announce(&self, subagent: ChatSubagent, steps: Vec<SubagentStep>) {
        let _ = self.send((subagent, steps));
    }
}
