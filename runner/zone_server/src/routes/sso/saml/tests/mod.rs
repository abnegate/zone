use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, FromRequest};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, REFERRER_POLICY};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use tower::ServiceExt;
use tracing::Level;
use uuid::Uuid;
use zone_enterprise::sso::saml::{Document, Refusal};
use zone_enterprise::sso::{ASSERTION_INVALID, Error, SIGN_IN_REFUSED};

use super::*;
use crate::db::sso::ProviderError;
use crate::services::sso::console::{
    SAML_ACS_PATH, SAML_METADATA_PATH, saml_acs_url, saml_entity_id,
};
use crate::state::{test_config, unreachable_pool};

const CALLBACK: &str = "https://zone.example.com/sso/callback";
const FORM: &str = "application/x-www-form-urlencoded";

fn query(location: &str) -> Vec<(String, String)> {
    let (_, query) = location.split_once('?').expect("a redirect has a query");
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

async fn parsed(content_type: &str, body: &str) -> Result<AcsForm, FormRejection> {
    let request = Request::post(SAML_ACS_PATH)
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap();
    Form::<AcsForm>::from_request(request, &())
        .await
        .map(|Form(form)| form)
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("the captured log")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured<T>(run: impl FnOnce() -> T) -> (T, String) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(Level::DEBUG)
        .with_ansi(false)
        .finish();
    let result = tracing::subscriber::with_default(subscriber, run);
    let logged =
        String::from_utf8_lossy(&captured.0.lock().expect("the captured log")).into_owned();
    (result, logged)
}

fn router() -> Router {
    Router::new()
        .route(SAML_METADATA_PATH, get(metadata))
        .route(
            SAML_ACS_PATH,
            post(acs).layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .with_state(AppState::new(test_config(), unreachable_pool(), None))
}

async fn posted(content_type: &str, body: &str) -> Response {
    router()
        .oneshot(
            Request::post(SAML_ACS_PATH)
                .header(CONTENT_TYPE, content_type)
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn uncached_redirect(response: &Response) -> String {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[CACHE_CONTROL], NO_STORE);
    assert_eq!(response.headers()[REFERRER_POLICY], NO_REFERRER);
    response.headers()[LOCATION].to_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_acs_form_carries_the_response_and_relay_state() {
    let form = parsed(
        FORM,
        "SAMLResponse=PHNhbWxwOlJlc3BvbnNl%2Bcg%3D%3D&RelayState=a%2Bb",
    )
    .await
    .unwrap();
    assert_eq!(form.response, "PHNhbWxwOlJlc3BvbnNl+cg==");
    assert_eq!(form.relay_state.as_deref(), Some("a+b"));
}

#[tokio::test]
async fn either_field_may_be_missing_from_the_acs_form() {
    let form = parsed(FORM, "RelayState=state-1").await.unwrap();
    assert_eq!(form.response, "");
    assert_eq!(form.relay_state.as_deref(), Some("state-1"));

    let form = parsed(FORM, "SAMLResponse=PHNhbWw%3D").await.unwrap();
    assert_eq!(form.relay_state, None);
}

#[tokio::test]
async fn only_a_form_is_an_acs_post() {
    assert!(parsed("application/json", "{}").await.is_err());
}

#[test]
fn debug_never_shows_the_response_or_relay_state() {
    let form = AcsForm {
        response: "PHNhbWxwOlJlc3BvbnNl-secret".into(),
        relay_state: Some("state-secret".into()),
    };
    let shown = format!("{form:?}");
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains(REDACTED), "{shown}");
}

#[test]
fn a_handoff_sends_the_state_and_code_to_the_callback() {
    let location = handed_off(
        CALLBACK,
        &Handoff {
            state: "a+b&c=d e".into(),
            code: "code/1?#".into(),
        },
    );
    assert!(location.starts_with(&format!("{CALLBACK}?")), "{location}");
    assert_eq!(
        location,
        format!("{CALLBACK}?state=a%2Bb%26c%3Dd+e&code=code%2F1%3F%23")
    );
    assert_eq!(
        query(&location),
        pairs(&[("state", "a+b&c=d e"), ("code", "code/1?#")])
    );
}

#[test]
fn a_refusal_sends_the_state_and_error_code_to_the_callback() {
    let location = refused(CALLBACK, Some("a&b"), ASSERTION_INVALID);
    assert_eq!(
        location,
        format!("{CALLBACK}?state=a%26b&error=sso_assertion_invalid")
    );
    assert_eq!(
        query(&location),
        pairs(&[("state", "a&b"), ("error", ASSERTION_INVALID)])
    );
}

#[test]
fn a_refusal_without_a_relay_state_sends_only_the_error() {
    assert_eq!(
        refused(CALLBACK, None, STATE_INVALID),
        format!("{CALLBACK}?error=sso_state_invalid")
    );
}

#[tokio::test]
async fn a_refusal_names_its_code_and_every_other_failure_is_a_failed_sign_in() {
    let cancelled = tokio::spawn(std::future::pending::<()>());
    cancelled.abort();
    let interrupted = cancelled.await.unwrap_err();
    let cases = [
        (
            SignInFailure::from(Error::AssertionInvalid(Refusal::Replayed)),
            ASSERTION_INVALID,
        ),
        (
            SignInFailure::from(Error::SignInRefused(
                "urn:oasis:names:tc:SAML:2.0:status:Responder".into(),
            )),
            SIGN_IN_REFUSED,
        ),
        (SignInFailure::from(Error::StateInvalid), STATE_INVALID),
        (
            SignInFailure::from(Error::ProviderNotFound),
            Error::ProviderNotFound.code(),
        ),
        (
            SignInFailure::from(sqlx::Error::PoolTimedOut),
            SIGN_IN_FAILED,
        ),
        (
            SignInFailure::Unreadable(Uuid::nil(), Error::MetadataInvalid("garbage".into())),
            SIGN_IN_FAILED,
        ),
        (
            SignInFailure::from(ProviderError::Mismatch(Uuid::nil())),
            SIGN_IN_FAILED,
        ),
        (SignInFailure::Stale, SIGN_IN_FAILED),
        (SignInFailure::Busy, SIGN_IN_FAILED),
        (SignInFailure::Interrupted(interrupted), SIGN_IN_FAILED),
    ];
    for (failure, expected) in cases {
        let (code, logged) = captured(|| refusal_code(&failure));
        assert_eq!(code, expected, "{failure:?}");
        assert_eq!(logged, "", "naming the code logs nothing: {failure:?}");
    }
}

#[test]
fn a_refused_status_is_logged_escaped() {
    let forged = "urn:x\n2026-11-22T00:00:00Z  INFO zone_server: Signed in admin@acme.com\u{1b}[2K";
    let ((), logged) = captured(|| log(&SignInFailure::from(Error::SignInRefused(forged.into()))));
    assert_eq!(logged.lines().count(), 1, "{logged}");
    assert!(!logged.contains('\u{1b}'), "{logged}");
    assert!(logged.contains("urn:x\\n2026"), "{logged}");
    assert!(logged.contains(SIGN_IN_REFUSED), "{logged}");
}

#[test]
fn any_other_failure_is_logged_as_an_error() {
    let ((), logged) = captured(|| log(&SignInFailure::Busy));
    assert!(logged.contains("ERROR"), "{logged}");
    assert!(logged.contains("Too many sign-ins"), "{logged}");
}

#[test]
fn a_redirect_is_a_see_other_that_is_never_cached_or_referred() {
    let response = redirect(CALLBACK);
    assert_eq!(uncached_redirect(&response), CALLBACK);
}

#[test]
fn a_redirect_that_is_not_a_header_value_fails_uncached() {
    let response = redirect("https://zone.example.com/\n");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[CACHE_CONTROL], NO_STORE);
    assert!(response.headers().get(LOCATION).is_none());
}

#[tokio::test]
async fn a_post_without_a_relay_state_is_an_invalid_state() {
    let callback = redirect_uri(&test_config().app_base_url);
    for (content_type, body) in [
        (FORM, "SAMLResponse=PHNhbWw%3D"),
        (FORM, "SAMLResponse=PHNhbWw%3D&RelayState="),
        (FORM, ""),
        ("application/json", "{\"RelayState\":\"state-1\"}"),
    ] {
        let response = posted(content_type, body).await;
        assert_eq!(
            uncached_redirect(&response),
            format!("{callback}?error=sso_state_invalid"),
            "{content_type} {body}"
        );
    }
}

#[tokio::test]
async fn a_relay_state_longer_than_the_binding_allows_is_an_invalid_state_sent_back_without_it() {
    let callback = redirect_uri(&test_config().app_base_url);
    let longest = "s".repeat(RELAY_STATE_LIMIT);
    let response = posted(
        FORM,
        &format!("SAMLResponse=PHNhbWw%3D&RelayState={longest}"),
    )
    .await;
    assert_eq!(
        uncached_redirect(&response),
        format!("{callback}?state={longest}&error=sso_sign_in_failed")
    );

    let longer = "s".repeat(RELAY_STATE_LIMIT + 1);
    let response = posted(
        FORM,
        &format!("SAMLResponse=PHNhbWw%3D&RelayState={longer}"),
    )
    .await;
    assert_eq!(
        uncached_redirect(&response),
        format!("{callback}?error=sso_state_invalid")
    );
}

#[tokio::test]
async fn a_body_over_the_limit_is_an_invalid_state_sent_back_without_one() {
    let callback = redirect_uri(&test_config().app_base_url);
    let form = |length: usize| {
        let prefix = "RelayState=state-1&SAMLResponse=";
        format!("{prefix}{}", "A".repeat(length - prefix.len()))
    };

    let response = posted(FORM, &form(BODY_LIMIT)).await;
    assert_eq!(
        uncached_redirect(&response),
        format!("{callback}?state=state-1&error=sso_sign_in_failed")
    );

    let response = posted(FORM, &form(BODY_LIMIT + 1)).await;
    assert_eq!(
        uncached_redirect(&response),
        format!("{callback}?error=sso_state_invalid")
    );
}

#[tokio::test]
async fn a_post_the_server_cannot_check_is_a_failed_sign_in_for_that_state() {
    let callback = redirect_uri(&test_config().app_base_url);
    let response = posted(FORM, "SAMLResponse=PHNhbWw%3D&RelayState=state-1").await;
    assert_eq!(
        uncached_redirect(&response),
        format!("{callback}?state=state-1&error=sso_sign_in_failed")
    );
}

#[tokio::test]
async fn the_metadata_describes_this_server_as_a_service_provider() {
    let response = router()
        .oneshot(
            Request::get(SAML_METADATA_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[CONTENT_TYPE],
        service_metadata::CONTENT_TYPE
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    Document::parse(&body).expect("the metadata is XML");
    let body = String::from_utf8(body.to_vec()).unwrap();
    let base = test_config().app_base_url;
    assert!(
        body.contains(&format!("entityID=\"{}\"", saml_entity_id(&base))),
        "{body}"
    );
    assert!(
        body.contains(&format!("Location=\"{}\"", saml_acs_url(&base))),
        "{body}"
    );
}
