use axum::Router;
use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, REFERRER_POLICY};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use tower::ServiceExt;
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

fn router() -> Router {
    Router::new()
        .route(SAML_METADATA_PATH, get(metadata))
        .route(SAML_ACS_PATH, post(acs))
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

#[test]
fn a_refusal_names_its_code_and_every_other_failure_is_a_failed_sign_in() {
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
    ];
    for (failure, expected) in cases {
        assert_eq!(refusal_code(&failure), expected, "{failure:?}");
    }
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
