use serde::Serialize;

use crate::services::github::releases::Created;

#[derive(Debug, Serialize)]
pub struct CreatedResponse {
    pub release: Created,
}
