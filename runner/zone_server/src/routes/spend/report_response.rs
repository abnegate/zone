use serde::Serialize;
use zone_enterprise::{Edition, Status};

use super::report::Report;

#[derive(Debug, Serialize)]
pub struct ReportResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    /// None while the server holds no license, when nothing is attributed.
    pub report: Option<Report>,
}
