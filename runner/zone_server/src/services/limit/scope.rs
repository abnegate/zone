use std::fmt;

/// What a shared count limits, named in its Valkey key so different limits on the same subject
/// never count into one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Gather,
    Sso,
    Scim,
    ScimFailures,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gather => "limit",
            Self::Sso => "sso",
            Self::Scim => "scim",
            Self::ScimFailures => "scim-failures",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
