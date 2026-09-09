//! What a caller receives when it asks for a source's credentials.

use zone_core::secret::SecretValue;

/// A bearer credential for one source, whatever minted it.
///
/// A personal access token read from the database and an installation token
/// minted seconds ago are the same thing to the caller: a string to put in an
/// `Authorization` header. Nothing on this type says which one it holds, so no
/// caller can grow a branch on the answer and no caller has to change when a
/// source moves from one to the other.
#[derive(Debug, Clone)]
pub struct Credential {
    token: SecretValue,
}

impl Credential {
    pub fn new(token: SecretValue) -> Self {
        Self { token }
    }

    /// Read the credential. Call this at the point of use and nowhere else.
    pub fn expose(&self) -> &str {
        self.token.expose()
    }

    pub fn into_secret(self) -> SecretValue {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_personal_access_token_and_an_installation_token_are_the_same_type() {
        let personal = Credential::new(SecretValue::new("ghp_personalaccesstoken"));
        let installation = Credential::new(SecretValue::new("ghs_installationtoken"));

        assert_eq!(personal.expose(), "ghp_personalaccesstoken");
        assert_eq!(installation.expose(), "ghs_installationtoken");
        assert_eq!(
            format!("{personal:?}"),
            format!("{installation:?}"),
            "the two origins must be indistinguishable to a caller"
        );
    }

    #[test]
    fn the_credential_never_renders_itself() {
        let credential = Credential::new(SecretValue::new("ghp_personalaccesstoken"));
        let rendered = format!("{credential:?}");

        assert!(!rendered.contains("ghp_personalaccesstoken"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn the_secret_survives_being_taken_out() {
        let credential = Credential::new(SecretValue::new("ghs_installationtoken"));
        assert_eq!(credential.into_secret().expose(), "ghs_installationtoken");
    }
}
