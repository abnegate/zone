use std::net::IpAddr;

use uuid::Uuid;
use zone_enterprise::{Plan, Status};

use crate::http::token_outcome::TokenOutcome;

/// What the token route logs about a request the gate let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Admitted {
    pub(crate) license: Uuid,
    pub(crate) address: IpAddr,
    pub(crate) plan: Plan,
    pub(crate) status: Status,
}

impl Admitted {
    pub(crate) fn refuse(&self, outcome: TokenOutcome, message: &'static str) {
        tracing::info!(
            %outcome,
            license = %self.license,
            address = %self.address,
            plan = %self.plan,
            status = %self.status,
            "{message}"
        );
    }
}
