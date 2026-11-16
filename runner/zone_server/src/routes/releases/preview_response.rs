use serde::Serialize;

use crate::services::github::releases::Preview;

#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub preview: Preview,
}
