use abnegate_agent_cli::CliSettings;

use super::TURN_TIMEOUT;

/// The settings every agent run Zone starts begins from: the crate's
/// defaults, given a chat turn's [`TURN_TIMEOUT`] rather than the crate's
/// budget for one command, and read until the work Claude left running in
/// the background has finished.
pub fn settings() -> CliSettings {
    CliSettings::default()
        .with_timeout(TURN_TIMEOUT)
        .await_background()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use abnegate_llm::Credential;

    use super::*;

    #[test]
    fn an_agents_turn_gets_longer_than_a_single_command_does() {
        let command = abnegate_exec::executor::DEFAULT_TIMEOUT;
        let turn = settings().timeout;

        assert!(
            turn > command,
            "an agent runs a loop of commands, so {turn:?} must exceed one command's {command:?}"
        );
    }

    /// The crate gives a run five minutes, which a chat turn that reads,
    /// edits and tests a repository outlives.
    #[test]
    fn zone_settings_allow_thirty_minutes() {
        const THIRTY_MINUTES: Duration = Duration::from_secs(30 * 60);

        assert_eq!(settings().timeout, THIRTY_MINUTES);
        assert!(settings().timeout > abnegate_agent_cli::DEFAULT_TIMEOUT);
    }

    #[test]
    fn zone_settings_inherit_the_hosts_session_and_directory() {
        let settings = settings();

        assert!(matches!(settings.credential, Credential::Inherited));
        assert!(settings.executable.is_none());
        assert!(settings.working_directory.is_none());
        assert_eq!(settings.timeout, TURN_TIMEOUT);
    }

    #[test]
    fn zone_settings_wait_for_the_agents_background_work() {
        assert!(
            settings().await_background,
            "a run that stops once claude first answers kills the subagent or monitor it left running"
        );
    }
}
