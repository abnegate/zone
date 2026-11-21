use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Renewed {
    pub license: String,
}
