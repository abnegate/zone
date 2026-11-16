use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) id_token: String,
    #[serde(default)]
    pub(crate) access_token: Option<String>,
}
