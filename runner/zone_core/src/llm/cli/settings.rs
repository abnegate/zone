use std::time::Duration;

use abnegate_agent_cli::CliSettings;

/// The settings every agent run Zone starts begins from: the crate's
/// defaults, with no deadline, and read until the work Claude left running in
/// the background has finished. A run ends when the agent finishes or the
/// caller drops it.
pub fn settings() -> CliSettings {
    CliSettings::default()
        .with_timeout(Duration::MAX)
        .await_background()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use abnegate_llm::Credential;

    use super::*;

    /// An agent that compiles, lints and tests a large repository can work
    /// for hours, so a clock that cuts it off throws that work away.
    #[test]
    fn an_agent_run_has_no_deadline() {
        let timeout = settings().timeout;

        assert!(
            Instant::now().checked_add(timeout).is_none(),
            "a run given {timeout:?} is stopped at a deadline"
        );
    }

    #[test]
    fn zone_settings_inherit_the_hosts_session_and_directory() {
        let settings = settings();

        assert!(matches!(settings.credential, Credential::Inherited));
        assert!(settings.executable.is_none());
        assert!(settings.working_directory.is_none());
    }

    #[test]
    fn zone_settings_wait_for_the_agents_background_work() {
        assert!(
            settings().await_background,
            "a run that stops once claude first answers kills the subagent or monitor it left running"
        );
    }
}
