use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;

use crate::services::github::releases::Offered;
use crate::services::github::releases::SourceOptions;

#[derive(Debug, Serialize)]
pub struct OptionsResponse {
    pub generated_at: DateTime<Utc>,
    pub sources: Vec<SourceOptions>,
    pub warnings: Vec<String>,
}

impl From<Offered> for OptionsResponse {
    fn from(offered: Offered) -> Self {
        Self {
            generated_at: offered.generated_at,
            sources: offered.sources,
            warnings: offered.warnings,
        }
    }
}
