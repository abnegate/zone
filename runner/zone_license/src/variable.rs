#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variable {
    PrivateKeyFile,
    PrivateKey,
    KeyId,
}

impl Variable {
    pub const fn name(self) -> &'static str {
        match self {
            Self::PrivateKeyFile => "ZONE_LICENSE_PRIVATE_KEY_FILE",
            Self::PrivateKey => "ZONE_LICENSE_PRIVATE_KEY",
            Self::KeyId => "ZONE_LICENSE_KEY_ID",
        }
    }

    /// Blank values count as unset, so an empty export cannot shadow the other key source.
    pub fn read(self) -> Option<String> {
        std::env::var(self.name())
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variable_keeps_its_documented_name() {
        let names = [
            Variable::PrivateKeyFile,
            Variable::PrivateKey,
            Variable::KeyId,
        ]
        .map(Variable::name);

        assert_eq!(
            names,
            [
                "ZONE_LICENSE_PRIVATE_KEY_FILE",
                "ZONE_LICENSE_PRIVATE_KEY",
                "ZONE_LICENSE_KEY_ID",
            ]
        );
    }
}
