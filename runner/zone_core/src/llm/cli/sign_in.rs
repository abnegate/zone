/// Whose sign-in a spawned agent runs under: whose account its turns spend,
/// and who is told to sign in when it fails.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SignIn {
    /// The server's own: the host's login, or a key in its environment.
    #[default]
    Instance,
    /// An organization's, renewed from its AI settings.
    Organization,
    /// The host's login, standing in for an organization that has not signed
    /// in. Its account pays, and the organization's own sign-in replaces it.
    Host,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_runs_under_the_servers_own_sign_in_until_told_whose() {
        assert_eq!(SignIn::default(), SignIn::Instance);
    }
}
