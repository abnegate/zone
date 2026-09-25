//! The page a browser shows when Zone's callback listener cannot send it on to the console. It
//! repeats nothing from the request: every word is Zone's own, or claude.com's, and all of it is
//! escaped.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::super::error::Error;

const FAILED: &str = "Claude sign-in failed";
const BUSY: &str = "Zone is busy";
const RETURN: &str = "You can close this tab and return to Zone.";
const UNREADABLE: &str = "claude.com sent Zone something it could not read. Start the sign-in \
                          again in Zone.";
const MISDIRECTED: &str = "Zone takes Claude's sign-in back only at the localhost address the \
                           sign-in named. Start the sign-in again in Zone.";
const OCCUPIED: &str = "Zone is finishing other sign-ins. Reload this page in a moment.";
const HTML: &str = "text/html; charset=utf-8";
const NO_SNIFF: &str = "nosniff";
const DENY: &str = "DENY";
const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; \
                      form-action 'none'; frame-ancestors 'none'";
const STYLE: &str = ":root{color-scheme:light dark;font-family:system-ui,-apple-system,\
                     \"Segoe UI\",sans-serif}body{margin:0;min-height:100vh;display:grid;\
                     place-items:center;background:Canvas;color:CanvasText}main{max-width:30rem;\
                     padding:2rem 1rem;text-align:center}h1{margin:0 0 .5rem;font-size:1.25rem}\
                     p{margin:0;line-height:1.5}";

pub const NO_STORE: &str = "no-store";
pub const NO_REFERRER: &str = "no-referrer";

#[derive(Debug, PartialEq, Eq)]
pub struct Page {
    status: StatusCode,
    title: &'static str,
    message: String,
}

impl Page {
    /// Why the sign-in did not finish. Zone's own failures say only that it failed.
    pub fn failed(error: &Error) -> Self {
        Self {
            status: status(error),
            title: FAILED,
            message: format!("{} {RETURN}", error.shown()),
        }
    }

    /// For a request that is not a reply claude.com sends.
    pub fn unreadable() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            title: FAILED,
            message: UNREADABLE.to_string(),
        }
    }

    /// For a request addressed to any host but the one the sign-in named.
    pub fn misdirected() -> Self {
        Self {
            status: StatusCode::MISDIRECTED_REQUEST,
            title: FAILED,
            message: MISDIRECTED.to_string(),
        }
    }

    /// For a request that came while the listener was answering as many as it may.
    pub fn busy() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            title: BUSY,
            message: OCCUPIED.to_string(),
        }
    }

    fn render(&self) -> String {
        let title = escape(self.title);
        let message = escape(&self.message);
        format!(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <meta name=\"robots\" content=\"noindex\">\n<title>{title} · Zone</title>\n\
             <style>{STYLE}</style>\n</head>\n<body>\n<main>\n<h1>{title}</h1>\n\
             <p>{message}</p>\n</main>\n</body>\n</html>\n"
        )
    }
}

impl IntoResponse for Page {
    fn into_response(self) -> Response {
        let body = self.render();
        (
            self.status,
            [
                (header::CONTENT_TYPE, HTML),
                (header::CACHE_CONTROL, NO_STORE),
                (header::REFERRER_POLICY, NO_REFERRER),
                (header::X_CONTENT_TYPE_OPTIONS, NO_SNIFF),
                (header::X_FRAME_OPTIONS, DENY),
                (header::CONTENT_SECURITY_POLICY, POLICY),
            ],
            body,
        )
            .into_response()
    }
}

fn status(error: &Error) -> StatusCode {
    match error {
        Error::Unreadable(_) | Error::Invalid(_) => StatusCode::BAD_REQUEST,
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::Deleted => StatusCode::NOT_FOUND,
        Error::Refused(_) | Error::Unreachable(_) => StatusCode::BAD_GATEWAY,
        Error::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        Error::Internal(_) | Error::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// `text` as HTML text or an attribute value, with every character that could end either
/// written as an entity.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use zone_core::llm::AgentKind;

    use super::*;
    use crate::services::login::error::UNSAVED;

    const HOSTILE: &str = "<script>alert(\"x\")</script> & <img src=x onerror='alert(1)'>";

    async fn sent(page: Page) -> (StatusCode, axum::http::HeaderMap, String) {
        let response = page.into_response();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body");
        (
            status,
            headers,
            String::from_utf8(body.to_vec()).expect("UTF-8"),
        )
    }

    #[test]
    fn every_character_that_could_end_text_or_an_attribute_is_escaped() {
        assert_eq!(
            escape(HOSTILE),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; \
             &lt;img src=x onerror=&#39;alert(1)&#39;&gt;"
        );
        assert_eq!(escape("Signed in · Zone"), "Signed in · Zone");
    }

    #[tokio::test]
    async fn a_refusal_is_shown_escaped_on_a_page_that_loads_nothing() {
        let (status, headers, body) =
            sent(Page::failed(&Error::Refused(HOSTILE.to_string()))).await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            body.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"),
            "{body}"
        );
        for raw in ["<script>", "<img", "onerror='"] {
            assert!(!body.contains(raw), "{raw} reached the page: {body}");
        }
        assert!(
            body.contains("<title>Claude sign-in failed · Zone</title>"),
            "{body}"
        );
        for (name, value) in [
            (header::CONTENT_TYPE, HTML),
            (header::CACHE_CONTROL, NO_STORE),
            (header::REFERRER_POLICY, NO_REFERRER),
            (header::X_CONTENT_TYPE_OPTIONS, NO_SNIFF),
            (header::X_FRAME_OPTIONS, DENY),
            (header::CONTENT_SECURITY_POLICY, POLICY),
        ] {
            assert_eq!(
                headers.get(&name).and_then(|value| value.to_str().ok()),
                Some(value),
                "{name}"
            );
        }
        assert!(
            !body.contains("http"),
            "the page loads or links nothing: {body}"
        );
    }

    #[test]
    fn each_failure_has_its_own_status_and_zones_own_say_only_that_it_failed() {
        for (error, status, reason) in [
            (
                Error::Invalid("expired"),
                StatusCode::BAD_REQUEST,
                "expired",
            ),
            (
                Error::Forbidden("demoted"),
                StatusCode::FORBIDDEN,
                "demoted",
            ),
            (
                Error::Deleted,
                StatusCode::NOT_FOUND,
                "Organization not found",
            ),
            (
                Error::Refused("Claude refused".to_string()),
                StatusCode::BAD_GATEWAY,
                "Claude refused",
            ),
            (
                Error::Unreachable("Could not reach Claude".to_string()),
                StatusCode::BAD_GATEWAY,
                "Could not reach Claude",
            ),
            (
                Error::Unavailable(AgentKind::Codex),
                StatusCode::SERVICE_UNAVAILABLE,
                "The codex CLI is not installed on this server",
            ),
            (
                Error::Internal("/app/agent-state is read-only".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                UNSAVED,
            ),
            (
                Error::Database(sqlx::Error::PoolTimedOut),
                StatusCode::INTERNAL_SERVER_ERROR,
                UNSAVED,
            ),
        ] {
            let page = Page::failed(&error);

            assert_eq!(page.status, status, "{error:?}");
            assert_eq!(page.title, FAILED);
            assert_eq!(page.message, format!("{reason} {RETURN}"), "{error:?}");
        }
    }

    #[test]
    fn a_request_to_another_host_or_past_the_limit_is_told_so() {
        assert_eq!(Page::misdirected().status, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(Page::busy().status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
