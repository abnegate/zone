use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) id_token: String,
}
