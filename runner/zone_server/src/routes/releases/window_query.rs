use serde::Deserialize;

use super::refresh_query::RefreshQuery;

#[derive(Debug, Default, Deserialize)]
pub struct WindowQuery {
    pub days: Option<u32>,
    pub refresh: Option<String>,
}

impl WindowQuery {
    pub fn refresh(&self) -> bool {
        RefreshQuery::requested(self.refresh.as_deref())
    }
}
