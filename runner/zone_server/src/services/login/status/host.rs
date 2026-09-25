//! The host's own sign-in to each agent, which every organization without one of its own shares.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::Mutex;
use zone_core::llm::AgentKind;

use super::super::probe::Probe;

pub struct Host {
    lifetime: Duration,
    answers: DashMap<(&'static str, PathBuf), Arc<Mutex<Option<Answer>>>>,
}

struct Answer {
    asked: Instant,
    probe: Option<Probe>,
}

impl Host {
    pub fn new(lifetime: Duration) -> Self {
        Self {
            lifetime,
            answers: DashMap::new(),
        }
    }

    /// What `ask` finds for `agent` at `executable`. One answer serves everyone who asks within
    /// its lifetime, and whoever asks while it is being found waits for it.
    pub async fn check(
        &self,
        agent: AgentKind,
        executable: PathBuf,
        ask: impl Future<Output = Option<Probe>>,
    ) -> Option<Probe> {
        let slot = self
            .answers
            .entry((agent.as_str(), executable))
            .or_default()
            .value()
            .clone();
        let mut answer = slot.lock().await;
        if let Some(fresh) = answer
            .as_ref()
            .filter(|answer| answer.asked.elapsed() < self.lifetime)
        {
            return fresh.probe.clone();
        }
        let probe = ask.await;
        *answer = Some(Answer {
            asked: Instant::now(),
            probe: probe.clone(),
        });
        probe
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::future::join_all;

    use super::*;

    const ASKERS: usize = 8;
    const LASTING: Duration = Duration::from_secs(60);
    const CHECKING: Duration = Duration::from_millis(50);

    fn signed_in() -> Probe {
        Probe {
            signed_in: true,
            label: Some("ChatGPT".to_string()),
        }
    }

    fn executable(name: &str) -> PathBuf {
        PathBuf::from("/usr/local/bin").join(name)
    }

    async fn counted(checks: &AtomicUsize) -> Option<Probe> {
        checks.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(CHECKING).await;
        Some(signed_in())
    }

    #[tokio::test]
    async fn one_check_answers_everyone_who_asks_at_once_and_after_until_it_goes_stale() {
        let host = Host::new(LASTING);
        let checks = AtomicUsize::new(0);

        let answers = join_all(
            (0..ASKERS)
                .map(|_| host.check(AgentKind::Codex, executable("codex"), counted(&checks))),
        )
        .await;
        let later = host
            .check(AgentKind::Codex, executable("codex"), counted(&checks))
            .await;

        assert_eq!(checks.load(Ordering::SeqCst), 1);
        for answer in answers.into_iter().chain([later]) {
            assert_eq!(answer, Some(signed_in()));
        }
    }

    #[tokio::test]
    async fn a_stale_answer_is_checked_again() {
        let host = Host::new(Duration::ZERO);
        let checks = AtomicUsize::new(0);

        for _ in 0..2 {
            host.check(AgentKind::Claude, executable("claude"), counted(&checks))
                .await;
        }

        assert_eq!(checks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn each_agent_and_executable_is_checked_on_its_own() {
        let host = Host::new(LASTING);
        let checks = AtomicUsize::new(0);

        for (agent, name) in [
            (AgentKind::Codex, "codex"),
            (AgentKind::Claude, "codex"),
            (AgentKind::Codex, "codex-nightly"),
            (AgentKind::Codex, "codex"),
        ] {
            host.check(agent, executable(name), counted(&checks)).await;
        }

        assert_eq!(checks.load(Ordering::SeqCst), 3);
    }
}
