use serde::Deserialize;

use crate::routes::refresh_query::RefreshQuery;

#[derive(Debug, Default, Deserialize)]
pub struct WindowQuery {
    pub days: Option<u32>,
    pub refresh: Option<String>,
}

impl WindowQuery {
    pub fn refresh(&self) -> bool {
        RefreshQuery::asked(self.refresh.as_deref())
    }
}
