use axum::response::Response;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::{License, Plan, Status};

use crate::gate::refusal::Refusal;

/// Why the gate refused, with the fields a route logs: `license`, `plan` and
/// `status` once a key has verified, and the verify code as `error` before.
/// It holds no key, customer or email, so logging every field is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Denial {
    pub refusal: Refusal,
    pub license: Option<Uuid>,
    pub plan: Option<Plan>,
    pub status: Option<Status>,
    pub error: Option<&'static str>,
}

impl Denial {
    pub(crate) fn new(refusal: Refusal) -> Self {
        Self {
            refusal,
            license: None,
            plan: None,
            status: None,
            error: None,
        }
    }

    pub(crate) fn unverified(error: &'static str) -> Self {
        Self {
            error: Some(error),
            ..Self::new(Refusal::Unverified)
        }
    }

    pub(crate) fn verified(refusal: Refusal, license: &License, now: DateTime<Utc>) -> Self {
        Self {
            license: Some(license.id()),
            plan: Some(license.plan()),
            status: Some(license.status(now)),
            ..Self::new(refusal)
        }
    }

    pub fn respond(self) -> Response {
        self.refusal.respond()
    }
}
