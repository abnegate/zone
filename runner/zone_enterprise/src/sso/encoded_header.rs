use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct EncodedHeader {
    pub(crate) alg: String,
    #[serde(default)]
    pub(crate) kid: Option<String>,
}
