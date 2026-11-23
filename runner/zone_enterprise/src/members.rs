use chrono::{DateTime, Utc};

use crate::entitlement::Feature;
use crate::license::License;

pub const COMMUNITY_MEMBERS: u32 = 3;
pub const MEMBER_CAP_REACHED: &str = "member_cap_reached";
pub const MEMBER_ADMISSION_BUSY: &str = "member_admission_busy";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Members {
    pub active: u32,
    pub cap: Option<u32>,
}

impl Members {
    pub fn cap(license: Option<&License>, now: DateTime<Utc>) -> Option<u32> {
        if Feature::Members.entitled(license, now) {
            None
        } else {
            Some(COMMUNITY_MEMBERS)
        }
    }

    pub fn full(&self) -> bool {
        self.cap.is_some_and(|cap| self.active >= cap)
    }

    pub fn over(&self) -> bool {
        self.cap.is_some_and(|cap| self.active > cap)
    }
}

#[cfg(test)]
mod tests;
