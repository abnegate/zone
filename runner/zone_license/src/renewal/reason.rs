use std::fmt;

pub const MISSING: &str = "missing";
pub const STATUS: &str = "status";
pub const ENDED: &str = "ended";
pub const PRODUCT: &str = "product";
pub const SETTLING: &str = "settling";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Missing,
    Status,
    Ended,
    Product,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => MISSING,
            Self::Status => STATUS,
            Self::Ended => ENDED,
            Self::Product => PRODUCT,
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
