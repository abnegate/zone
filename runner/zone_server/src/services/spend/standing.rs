//! The one place spend reads the license: everything else asks a [`Standing`].

use chrono::{DateTime, Utc};
use zone_enterprise::{Edition, Status, status};

use super::unlicensed::Unlicensed;
use crate::services::license::Stored;
use crate::services::policy;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    /// Budgets may be created, changed and deleted.
    pub entitled: bool,
    /// Spend is attributed, ingested, reported and held to hard limits.
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
}

/// The gate for every spend feature.
pub fn standing(stored: &Stored, now: DateTime<Utc>) -> Standing {
    let license = stored.license();
    let status = status(license, now);
    Standing {
        entitled: matches!(stored, Stored::Verified(_))
            && matches!(status, Status::Active | Status::Grace),
        enforced: policy::enforces(stored),
        edition: Edition::from(license),
        status,
    }
}

impl Standing {
    const UNREAD: Self = Self {
        entitled: false,
        enforced: false,
        edition: Edition::Community,
        status: Status::Community,
    };

    /// The server's standing now, or none at all when the license can't be read.
    pub async fn read(state: &AppState) -> Self {
        match state.stored_license().await {
            Ok(stored) => standing(&stored, Utc::now()),
            Err(error) => {
                tracing::error!(%error, "Could not read the license, so spend is neither attributed nor enforced");
                Self::UNREAD
            }
        }
    }

    pub fn editable(&self, writable: bool) -> bool {
        self.entitled && writable
    }

    pub fn require(&self) -> Result<(), Unlicensed> {
        if self.entitled {
            return Ok(());
        }
        Err(Unlicensed)
    }
}
