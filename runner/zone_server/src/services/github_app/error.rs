//! Failures on the GitHub App path, worded so none can carry a credential.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GithubAppError {
    #[error("The GitHub App configuration is missing or invalid.")]
    Configuration,
    #[error("The GitHub App private key could not be read.")]
    PrivateKey,
    #[error("The GitHub App private key could not be sealed for storage.")]
    Sealing,
    #[error("The GitHub App private key is not sealed, or could not be unsealed.")]
    Unsealing,
    #[error("The GitHub App JWT could not be signed.")]
    Signing,
    #[error("The installation token request failed: {0}")]
    Transport(String),
    #[error("GitHub returned HTTP {0} for the installation token request.")]
    Status(u16),
    #[error("The installation token response could not be read.")]
    Response,
    #[error("The stored source credential could not be decrypted.")]
    Credential,
}

/// A `reqwest` failure as a [`GithubAppError::Transport`], with the URL removed.
///
/// `reqwest::Error` appends `for url (...)` to its `Display`. A GitHub App
/// request carries its bearer credential in a header rather than the URL, but a
/// redirect, a proxy or a future caller can put one in the URL, and an error
/// string is copied into logs and handed back to users. Stripping the URL costs
/// nothing and removes the whole class of leak.
pub fn transport(error: reqwest::Error) -> GithubAppError {
    GithubAppError::Transport(error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_variant_echoes_key_material() {
        assert_eq!(
            GithubAppError::Status(404).to_string(),
            "GitHub returned HTTP 404 for the installation token request."
        );
        assert!(!GithubAppError::PrivateKey.to_string().contains("BEGIN"));
        assert!(!GithubAppError::Signing.to_string().contains("BEGIN"));
    }

    #[tokio::test]
    async fn transport_errors_drop_the_request_url() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .expect("client");

        let error = client
            .post("http://127.0.0.1:1/app/installations/1/access_tokens?credential=leaked")
            .send()
            .await
            .expect_err("a request to a closed port fails");

        let rendered = transport(error).to_string();
        assert!(
            !rendered.contains("credential=leaked") && !rendered.contains("127.0.0.1"),
            "the request URL survived into the error: {rendered}"
        );
    }
}
