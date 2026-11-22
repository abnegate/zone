use crate::sso::saml::testing::{CERTIFICATE, OTHER_CERTIFICATE, OTHER_PRIVATE_KEY};
use crate::sso::testing::PRIVATE_KEY;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Key {
    #[default]
    Primary,
    Other,
}

impl Key {
    pub fn private_key(self) -> &'static str {
        match self {
            Self::Primary => PRIVATE_KEY,
            Self::Other => OTHER_PRIVATE_KEY,
        }
    }

    pub fn certificate(self) -> &'static str {
        match self {
            Self::Primary => CERTIFICATE,
            Self::Other => OTHER_CERTIFICATE,
        }
    }
}
