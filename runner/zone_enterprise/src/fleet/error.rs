use crate::fleet::code::{LABEL_INVALID, VERSION_INVALID};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("\"{0}\" is not a valid Zone version. Use MAJOR.MINOR.PATCH, such as 1.4.0.")]
    VersionInvalid(String),
    #[error(
        "\"{0}\" is not a valid runner label. Use 1 to 63 lowercase letters, digits or . _ - = /, starting with a letter or digit."
    )]
    LabelInvalid(String),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::VersionInvalid(_) => VERSION_INVALID,
            Self::LabelInvalid(_) => LABEL_INVALID,
        }
    }
}
