use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct TokenRefusal {
    pub(crate) error: String,
}
