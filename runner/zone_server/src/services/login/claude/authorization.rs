//! The PKCE authorization a Claude sign-in starts from.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Url;
use sha2::{Digest, Sha256};
use zone_core::secret::SecretValue;

use super::{AUTHORIZE_URL, CLIENT_ID, Redirect, Scope};

const VERIFIER_BYTES: usize = 64;
const STATE_BYTES: usize = 32;
const CHALLENGE_METHOD: &str = "S256";

#[derive(Debug)]
pub struct Authorization {
    pub url: String,
    pub state: String,
    pub verifier: SecretValue,
    pub scope: Scope,
    pub redirect: Redirect,
}

impl Authorization {
    pub fn new(scope: Scope, redirect: Redirect) -> Self {
        let verifier = SecretValue::new(random::<VERIFIER_BYTES>());
        let state = random::<STATE_BYTES>();
        let mut url = Url::parse(AUTHORIZE_URL).expect("AUTHORIZE_URL is an absolute URL");
        url.query_pairs_mut()
            .append_pair("code", "true")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect.uri())
            .append_pair("scope", &scope.parameter())
            .append_pair("code_challenge", &challenge(verifier.expose()))
            .append_pair("code_challenge_method", CHALLENGE_METHOD)
            .append_pair("state", &state);

        Self {
            url: url.into(),
            state,
            verifier,
            scope,
            redirect,
        }
    }
}

fn random<const BYTES: usize>() -> String {
    let mut bytes = [0u8; BYTES];
    rand::fill(&mut bytes);
    encode(bytes)
}

fn challenge(verifier: &str) -> String {
    encode(Sha256::digest(verifier))
}

fn encode(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::login::claude::{Flow, REDIRECT_URL};

    const RFC_7636_VERIFIER_OCTETS: [u8; 32] = [
        116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212, 37,
        77, 105, 214, 191, 240, 91, 88, 5, 88, 83, 132, 141, 121,
    ];
    const INFERENCE_SCOPE: &str = "user:inference";
    const FULL_SCOPE: &str = "org:create_api_key user:profile user:inference \
                              user:sessions:claude_code user:mcp_servers user:file_upload \
                              user:plugins";
    const INFERENCE_QUERY: &str = "user%3Ainference";
    const FULL_QUERY: &str = "org%3Acreate_api_key+user%3Aprofile+user%3Ainference\
                              +user%3Asessions%3Aclaude_code+user%3Amcp_servers\
                              +user%3Afile_upload+user%3Aplugins";

    #[test]
    fn the_rfc_7636_verifier_gives_its_challenge() {
        let verifier = encode(RFC_7636_VERIFIER_OCTETS);

        assert_eq!(verifier, "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            challenge(&verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_authorize_url_carries_the_clis_parameters_in_the_clis_order() {
        for (scope, requested, encoded) in [
            (Scope::Inference, INFERENCE_SCOPE, INFERENCE_QUERY),
            (Scope::Full, FULL_SCOPE, FULL_QUERY),
        ] {
            let authorization = Authorization::new(scope, Redirect::Paste);
            let code_challenge = challenge(authorization.verifier.expose());
            let state = authorization.state.as_str();
            let url = Url::parse(&authorization.url).expect("an absolute URL");
            let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();

            assert_eq!(authorization.scope, scope);
            assert_eq!(
                pairs,
                [
                    ("code", "true"),
                    ("client_id", CLIENT_ID),
                    ("response_type", "code"),
                    ("redirect_uri", REDIRECT_URL),
                    ("scope", requested),
                    ("code_challenge", code_challenge.as_str()),
                    ("code_challenge_method", "S256"),
                    ("state", state),
                ]
                .map(|(key, value)| (key.to_string(), value.to_string()))
            );
            assert_eq!(
                authorization.url,
                format!(
                    "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
                     &redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback\
                     &scope={encoded}&code_challenge={code_challenge}\
                     &code_challenge_method=S256&state={state}"
                )
            );
        }
    }

    #[test]
    fn a_loopback_authorization_sends_the_browser_back_to_zones_listener() {
        let authorization = Authorization::new(Scope::Inference, Redirect::Loopback(54_545));
        let url = Url::parse(&authorization.url).expect("an absolute URL");
        let redirect: Vec<String> = url
            .query_pairs()
            .filter(|(key, _)| key == "redirect_uri")
            .map(|(_, value)| value.into_owned())
            .collect();

        assert_eq!(redirect, ["http://localhost:54545/callback"]);
        assert_eq!(authorization.redirect, Redirect::Loopback(54_545));
        assert_eq!(authorization.redirect.flow(), Flow::Loopback);
        assert!(
            authorization.url.contains(
                "&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&scope="
            ),
            "{}",
            authorization.url
        );
    }

    #[test]
    fn each_authorization_draws_a_fresh_verifier_and_state() {
        let first = Authorization::new(Scope::Inference, Redirect::Paste);
        let second = Authorization::new(Scope::Inference, Redirect::Paste);
        let url_safe = |text: &str| {
            text.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        };

        assert_eq!(
            first.verifier.expose().len(),
            86,
            "64 random bytes in unpadded base64url"
        );
        assert_eq!(
            first.state.len(),
            43,
            "32 random bytes in unpadded base64url"
        );
        assert!(url_safe(first.verifier.expose()) && url_safe(&first.state));
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, second.state);
    }

    #[test]
    fn debug_of_an_authorization_hides_the_verifier() {
        let authorization = Authorization::new(Scope::Full, Redirect::Loopback(54_545));
        let rendered = format!("{authorization:?}");

        assert!(
            !rendered.contains(authorization.verifier.expose()),
            "{rendered}"
        );
        assert!(rendered.contains(&authorization.state), "{rendered}");
    }
}
