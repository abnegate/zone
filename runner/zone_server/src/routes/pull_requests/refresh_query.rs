use serde::Deserialize;

const REQUESTED: [&str; 2] = ["1", "true"];

#[derive(Debug, Default, Deserialize)]
pub struct RefreshQuery {
    #[serde(default)]
    pub refresh: Option<String>,
}

impl RefreshQuery {
    pub fn requested(&self) -> bool {
        self.refresh
            .as_deref()
            .is_some_and(|value| REQUESTED.contains(&value.to_ascii_lowercase().as_str()))
    }
}
