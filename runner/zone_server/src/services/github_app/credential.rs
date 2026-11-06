use std::fmt;

use abnegate_secret::SecretValue;

use super::scope::Scope;

/// Nothing a caller can read says whether a stored token or a minted one is
/// held, so no caller can grow a branch on the answer.
#[derive(Clone)]
pub struct Credential {
    secret: SecretValue,
    scope: Option<Scope>,
}

impl Credential {
    pub(crate) fn stored(secret: SecretValue) -> Self {
        Self {
            secret,
            scope: None,
        }
    }

    pub(crate) fn installed(secret: SecretValue, scope: Scope) -> Self {
        Self {
            secret,
            scope: Some(scope),
        }
    }

    pub(super) fn scope(&self) -> Option<Scope> {
        self.scope
    }

    pub fn secret(&self) -> &SecretValue {
        &self.secret
    }

    pub fn into_secret(self) -> SecretValue {
        self.secret
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("secret", &self.secret)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::services::github_app::tenant::Tenant;

    fn scope() -> Scope {
        Scope {
            tenant: Tenant::Workspace(Uuid::new_v4()),
            installation: Uuid::new_v4(),
        }
    }

    #[test]
    fn two_credentials_render_identically() {
        let personal = Credential::stored(SecretValue::new("ghp_personalaccesstoken"));
        let installation =
            Credential::installed(SecretValue::new("ghs_installationtoken"), scope());

        assert_eq!(
            format!("{personal:?}"),
            format!("{installation:?}"),
            "the two origins must be indistinguishable to a caller"
        );
    }

    #[test]
    fn the_credential_never_renders_itself() {
        let credential = Credential::stored(SecretValue::new("ghp_personalaccesstoken"));
        let rendered = format!("{credential:?}");

        assert!(!rendered.contains("ghp_personalaccesstoken"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn only_a_minted_credential_names_its_installation() {
        let scope = scope();

        assert_eq!(
            Credential::installed(SecretValue::new("ghs_installationtoken"), scope).scope(),
            Some(scope)
        );
        assert_eq!(
            Credential::stored(SecretValue::new("ghp_personalaccesstoken")).scope(),
            None
        );
    }

    #[test]
    fn the_secret_survives_being_read_and_taken_out() {
        let credential = Credential::stored(SecretValue::new("ghs_installationtoken"));

        assert_eq!(credential.secret().expose(), "ghs_installationtoken");
        assert_eq!(credential.into_secret().expose(), "ghs_installationtoken");
    }
}
