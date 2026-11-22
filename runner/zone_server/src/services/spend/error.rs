use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("LITELLM_HOST is not a URL")]
    Host,
    #[error("Could not reach LiteLLM: {0}")]
    Http(#[from] reqwest::Error),
    #[error("LiteLLM refused the request with {0}")]
    Status(StatusCode),
    #[error("LiteLLM registered a key other than the one Zone derived")]
    Token,
    #[error("Could not check {failed} of {checked} spend keys with LiteLLM")]
    Unverified { failed: usize, checked: usize },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
