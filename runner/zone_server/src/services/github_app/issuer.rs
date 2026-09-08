//! The exchange of an App JWT for an installation access token.

use std::time::Duration;

use reqwest::{Client, Url};
use zone_core::secret::SecretValue;

use super::error::{GithubAppError, transport};
use super::identifier::InstallationId;
use super::token::InstallationToken;

pub const GITHUB_API_ORIGIN: &str = "https://api.github.com";

const API_VERSION: &str = "2022-11-28";
const ACCEPT: &str = "application/vnd.github+json";
const USER_AGENT: &str = "Zone-github-app";
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Issuer {
    http: Client,
    origin: Url,
}

impl Issuer {
    pub fn new() -> Result<Self, GithubAppError> {
        Self::at(GITHUB_API_ORIGIN)
    }

    /// An issuer pointed at `origin`, for GitHub Enterprise and for tests.
    pub fn at(origin: &str) -> Result<Self, GithubAppError> {
        let origin = Url::parse(origin).map_err(|_| GithubAppError::Configuration)?;

        Ok(Self {
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(TIMEOUT)
                .user_agent(USER_AGENT)
                .build()
                .map_err(|_| GithubAppError::Configuration)?,
            origin,
        })
    }

    /// Trade `jwt` for a token scoped to one installation.
    ///
    /// Redirects are refused rather than followed: a redirect is the one way an
    /// `Authorization` header reaches a host GitHub did not choose. A failure
    /// response yields its status and nothing else, since the body of a refused
    /// authentication attempt routinely quotes what was sent.
    pub async fn issue(
        &self,
        jwt: &SecretValue,
        installation: InstallationId,
    ) -> Result<InstallationToken, GithubAppError> {
        let response = self
            .http
            .post(self.token_url(installation)?)
            .header("Accept", ACCEPT)
            .header("X-GitHub-Api-Version", API_VERSION)
            .bearer_auth(jwt.expose())
            .send()
            .await
            .map_err(transport)?;

        let status = response.status();
        if !status.is_success() {
            return Err(GithubAppError::Status(status.as_u16()));
        }

        InstallationToken::parse(&response.text().await.map_err(transport)?)
    }

    fn token_url(&self, installation: InstallationId) -> Result<Url, GithubAppError> {
        let mut url = self.origin.clone();
        let installation = installation.get().to_string();

        url.path_segments_mut()
            .map_err(|_| GithubAppError::Configuration)?
            .pop_if_empty()
            .extend(["app", "installations", &installation, "access_tokens"]);

        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn jwt() -> SecretValue {
        SecretValue::new("header.payload.signature")
    }

    #[test]
    fn the_token_url_names_the_installation() {
        let issuer = Issuer::at("https://github.example.com/api/v3").expect("issuer");

        assert_eq!(
            issuer
                .token_url(InstallationId::new(67890))
                .expect("url")
                .as_str(),
            "https://github.example.com/api/v3/app/installations/67890/access_tokens"
        );
    }

    #[tokio::test]
    async fn a_successful_exchange_yields_a_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/app/installations/67890/access_tokens"))
            .and(header("Authorization", "Bearer header.payload.signature"))
            .and(header("X-GitHub-Api-Version", API_VERSION))
            .and(header_exists("Accept"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "token": "ghs_installationtokenvalue",
                "expires_at": "2026-01-15T12:00:00Z",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let token = Issuer::at(&server.uri())
            .expect("issuer")
            .issue(&jwt(), InstallationId::new(67890))
            .await
            .expect("issue");

        assert_eq!(token.secret().expose(), "ghs_installationtokenvalue");
    }

    #[tokio::test]
    async fn a_refusal_surfaces_as_a_status_without_the_url_or_the_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "message": "Bad credentials: header.payload.signature",
            })))
            .mount(&server)
            .await;

        let error = Issuer::at(&server.uri())
            .expect("issuer")
            .issue(&jwt(), InstallationId::new(67890))
            .await
            .expect_err("a 401 is a failure");

        let rendered = error.to_string();
        assert!(matches!(error, GithubAppError::Status(401)));
        assert!(!rendered.contains("header.payload.signature"));
        assert!(!rendered.contains("access_tokens"));
        assert!(!rendered.contains(&server.uri()));
    }

    #[tokio::test]
    async fn an_unreachable_host_surfaces_without_the_credentialed_url() {
        let origin = "http://127.0.0.1:1";

        let error = Issuer::at(origin)
            .expect("issuer")
            .issue(&jwt(), InstallationId::new(67890))
            .await
            .expect_err("a closed port is a failure");

        let rendered = error.to_string();
        assert!(matches!(error, GithubAppError::Transport(_)));
        assert!(
            !rendered.contains("127.0.0.1") && !rendered.contains("access_tokens"),
            "the request URL survived into the error: {rendered}"
        );
        assert!(!rendered.contains("header.payload.signature"));
    }

    #[tokio::test]
    async fn a_redirect_is_refused_rather_than_followed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "https://example.invalid/"),
            )
            .mount(&server)
            .await;

        let error = Issuer::at(&server.uri())
            .expect("issuer")
            .issue(&jwt(), InstallationId::new(67890))
            .await
            .expect_err("a redirect must not carry the JWT onward");

        assert!(matches!(error, GithubAppError::Status(302)));
    }
}
