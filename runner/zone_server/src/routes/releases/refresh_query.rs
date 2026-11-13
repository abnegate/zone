use serde::Deserialize;

const REQUESTED: [&str; 2] = ["1", "true"];

#[derive(Debug, Default, Deserialize)]
pub struct RefreshQuery {
    pub refresh: Option<String>,
}

impl RefreshQuery {
    pub fn requested(refresh: Option<&str>) -> bool {
        refresh.is_some_and(|refresh| REQUESTED.contains(&refresh))
    }

    pub fn refresh(&self) -> bool {
        Self::requested(self.refresh.as_deref())
    }
}
