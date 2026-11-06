//! What a caller receives when it asks for a source's or project's credential.

use std::fmt;

use abnegate_secret::SecretValue;
use uuid::Uuid;

/// A bearer credential, whatever minted it.
///
/// A personal access token read from the database and an installation token
/// minted seconds ago are the same thing to the caller: a secret to put in an
/// `Authorization` header. Nothing a caller can read says which one it holds,
/// so no caller can grow a branch on the answer.
#[derive(Clone)]
pub struct Credential {
    secret: SecretValue,
    installation: Option<Uuid>,
}

impl Credential {
    pub(crate) fn stored(secret: SecretValue) -> Self {
        Self {
            secret,
            installation: None,
        }
    }

    pub(crate) fn installed(secret: SecretValue, installation: Uuid) -> Self {
        Self {
            secret,
            installation: Some(installation),
        }
    }

    pub(super) fn installation(&self) -> Option<Uuid> {
        self.installation
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
    use super::*;

    #[test]
    fn two_credentials_render_identically() {
        let personal = Credential::stored(SecretValue::new("ghp_personalaccesstoken"));
        let installation =
            Credential::installed(SecretValue::new("ghs_installationtoken"), Uuid::new_v4());

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
        let installation = Uuid::new_v4();

        assert_eq!(
            Credential::installed(SecretValue::new("ghs_installationtoken"), installation)
                .installation(),
            Some(installation)
        );
        assert_eq!(
            Credential::stored(SecretValue::new("ghp_personalaccesstoken")).installation(),
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
