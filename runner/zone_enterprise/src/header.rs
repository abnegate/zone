use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Header {
    pub(crate) key_id: String,
}
