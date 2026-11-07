use std::fmt;

use crate::services::github_app::Unauthenticated;

/// A push the remote did not take. Git does not say why a push failed, so
/// whether GitHub refused the token is read from its API, which answers the
/// same token with a 401 when it does.
#[derive(Debug)]
pub(super) struct Rejection {
    pub(super) reason: String,
    pub(super) unauthenticated: bool,
}

impl Unauthenticated for Rejection {
    fn unauthenticated(&self) -> bool {
        self.unauthenticated
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_push_whose_token_github_refuses_is_unauthenticated() {
        let refused = Rejection {
            reason: "Git push failed; verify repository access".to_string(),
            unauthenticated: true,
        };
        let rejected = Rejection {
            reason: "Git push rejected; remote history or policy changed".to_string(),
            unauthenticated: false,
        };

        assert!(refused.unauthenticated());
        assert!(!rejected.unauthenticated());
        assert_eq!(rejected.to_string(), rejected.reason);
    }
}
