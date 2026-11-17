use uuid::Uuid;

/// What a new session was signed in with. A password sign-in carries the hash it verified, so a
/// reclaim that replaced the password since refuses the session instead of leaving the old owner
/// signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential<'a> {
    Registration,
    Password(&'a str),
    Provider(Uuid),
}

impl Credential<'_> {
    pub fn password_hash(&self) -> Option<&str> {
        match self {
            Self::Password(hash) => Some(hash),
            Self::Registration | Self::Provider(_) => None,
        }
    }

    pub fn provider(&self) -> Option<Uuid> {
        match self {
            Self::Provider(id) => Some(*id),
            Self::Registration | Self::Password(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_password_sign_in_is_rechecked() {
        assert_eq!(Credential::Password("hash").password_hash(), Some("hash"));
        assert_eq!(Credential::Registration.password_hash(), None);
        assert_eq!(Credential::Provider(Uuid::nil()).password_hash(), None);
    }

    #[test]
    fn only_a_provider_sign_in_records_its_provider() {
        assert_eq!(
            Credential::Provider(Uuid::nil()).provider(),
            Some(Uuid::nil())
        );
        assert_eq!(Credential::Registration.provider(), None);
        assert_eq!(Credential::Password("hash").provider(), None);
    }
}
