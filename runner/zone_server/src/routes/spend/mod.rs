//! An organization's spend reports, their CSV export, and its monthly budgets. Only the
//! organization's owners and admins see any of it; changing a budget also takes a Team license
//! that is active or in its grace period.

pub mod access;
pub mod audited_budget;
pub mod budget_response;
pub mod budgets;
pub mod budgets_response;
pub mod exports;
pub mod query;
pub mod refusal;
pub mod report;
pub mod report_response;
pub mod reports;

#[cfg(test)]
mod tests;

use axum::extract::Path;
use axum::extract::rejection::PathRejection;
use uuid::Uuid;

use crate::auth::AuthUser;
use access::ORGANIZATION_MISSING;
use refusal::Refusal;

fn caller(auth: &AuthUser) -> Result<Uuid, Refusal> {
    auth.0.user_id().map_err(|_| Refusal::Unauthenticated)
}

fn organization(path: Result<Path<Uuid>, PathRejection>) -> Result<Uuid, Refusal> {
    path.map(|Path(organization)| organization)
        .map_err(|_| Refusal::Missing(ORGANIZATION_MISSING))
}
