//! Reverse-proxy the manager API and WebSockets to a Zone server.

use axum::{
    Json,
    body::Body,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as AxumMessage, WebSocket},
    },
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

use crate::serve::AppState;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

pub fn upstream_url(base: &str, uri: &Uri) -> String {
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());
    let path = if path.is_empty() { "/" } else { path };
    format!("{}{path}", effective_proxy_target(base))
}

/// Prefer cleartext for loopback HTTPS.
///
/// Local Traefik serves `manager.localhost` on port 80. Port 443 uses an
/// untrusted default certificate when `SECURITY_GENERATE_CERTIFICATE=false`,
/// which reqwest rejects (`unable to get local issuer certificate`) and the
/// UI surfaces as "Registration failed".
pub fn effective_proxy_target(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let Some(rest) = base.strip_prefix("https://") else {
        return base.to_string();
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    match cleartext_loopback_authority(authority) {
        Some(authority) => format!("http://{authority}"),
        None => base.to_string(),
    }
}

fn cleartext_loopback_authority(authority: &str) -> Option<String> {
    let (host, port) = split_authority(authority)?;
    if !is_loopback_host(&host) {
        return None;
    }
    match port {
        None | Some(443) => Some(http_authority(&host)),
        Some(_) => None,
    }
}

fn split_authority(authority: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = match tail {
            "" => None,
            tail => Some(tail.strip_prefix(':')?.parse().ok()?),
        };
        return Some((host.to_ascii_lowercase(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => Some((
            host.trim_end_matches('.').to_ascii_lowercase(),
            Some(port.parse().ok()?),
        )),
        _ => Some((authority.trim_end_matches('.').to_ascii_lowercase(), None)),
    }
}

fn http_authority(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.to_ascii_lowercase().ends_with(".localhost")
}

pub fn ws_upstream_url(base: &str, uri: &Uri) -> String {
    let http = upstream_url(base, uri);
    if let Some(rest) = http.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = http.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        http
    }
}

pub async fn proxy_http(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let url = upstream_url(&state.proxy_target(), &uri);
    let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(error = %err, "Failed to read proxy request body");
            return (StatusCode::BAD_GATEWAY, "Failed to read request body").into_response();
        }
    };

    let mut request = state.http.request(method, &url).body(bytes);
    for (name, value) in headers.iter() {
        if HOP_BY_HOP
            .iter()
            .any(|h| name.as_str().eq_ignore_ascii_case(h))
        {
            continue;
        }
        request = request.header(name, value);
    }

    match request.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = Response::builder().status(status);
            for (name, value) in upstream.headers() {
                if HOP_BY_HOP
                    .iter()
                    .any(|h| name.as_str().eq_ignore_ascii_case(h))
                {
                    continue;
                }
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_ref()),
                    HeaderValue::from_bytes(value.as_bytes()),
                ) {
                    response = response.header(name, value);
                }
            }
            match upstream.bytes().await {
                Ok(body) => response
                    .body(Body::from(body))
                    .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()),
                Err(err) => {
                    tracing::error!(error = %err, "Failed to read proxy response body");
                    (StatusCode::BAD_GATEWAY, "Upstream response failed").into_response()
                }
            }
        }
        Err(err) => {
            tracing::error!(error = %err, url, "Failed to proxy HTTP request");
            let target = state.proxy_target();
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": format!("Failed to reach Zone server at {target}")
                })),
            )
                .into_response()
        }
    }
}

pub async fn proxy_ws(State(state): State<AppState>, uri: Uri, ws: WebSocketUpgrade) -> Response {
    let url = ws_upstream_url(&state.proxy_target(), &uri);
    ws.on_upgrade(move |socket| bridge_ws(socket, url))
        .into_response()
}

async fn bridge_ws(client: WebSocket, upstream_url: String) {
    let (mut client_sink, mut client_stream) = client.split();
    match tokio_tungstenite::connect_async(&upstream_url).await {
        Ok((upstream, _)) => {
            let (mut upstream_sink, mut upstream_stream) = upstream.split();
            let to_upstream = async {
                while let Some(Ok(message)) = client_stream.next().await {
                    let mapped = match message {
                        AxumMessage::Text(text) => {
                            Some(TungsteniteMessage::Text(text.as_str().into()))
                        }
                        AxumMessage::Binary(data) => Some(TungsteniteMessage::Binary(data)),
                        AxumMessage::Ping(data) => Some(TungsteniteMessage::Ping(data)),
                        AxumMessage::Pong(data) => Some(TungsteniteMessage::Pong(data)),
                        AxumMessage::Close(_) => {
                            let _ = upstream_sink.send(TungsteniteMessage::Close(None)).await;
                            break;
                        }
                    };
                    if let Some(mapped) = mapped
                        && upstream_sink.send(mapped).await.is_err()
                    {
                        break;
                    }
                }
            };
            let to_client = async {
                while let Some(Ok(message)) = upstream_stream.next().await {
                    let mapped = match message {
                        TungsteniteMessage::Text(text) => {
                            Some(AxumMessage::Text(text.as_str().into()))
                        }
                        TungsteniteMessage::Binary(data) => Some(AxumMessage::Binary(data)),
                        TungsteniteMessage::Ping(data) => Some(AxumMessage::Ping(data)),
                        TungsteniteMessage::Pong(data) => Some(AxumMessage::Pong(data)),
                        TungsteniteMessage::Close(_) => {
                            let _ = client_sink.send(AxumMessage::Close(None)).await;
                            break;
                        }
                        TungsteniteMessage::Frame(_) => None,
                    };
                    if let Some(mapped) = mapped
                        && client_sink.send(mapped).await.is_err()
                    {
                        break;
                    }
                }
            };
            tokio::select! {
                () = to_upstream => {}
                () = to_client => {}
            }
        }
        Err(err) => {
            tracing::error!(error = %err, upstream_url, "Failed to open upstream WebSocket");
            let _ = client_sink.send(AxumMessage::Close(None)).await;
        }
    }
}

pub fn http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .danger_accept_invalid_certs(false)
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_http_paths() {
        let uri: Uri = "/api/auth/login?x=1".parse().unwrap();
        assert_eq!(
            upstream_url("https://zone.example.com/", &uri),
            "https://zone.example.com/api/auth/login?x=1"
        );
    }

    #[test]
    fn builds_http_client() {
        assert!(http_client().is_ok());
    }

    #[test]
    fn converts_ws_scheme() {
        let uri: Uri = "/ws/chats/1".parse().unwrap();
        assert_eq!(
            ws_upstream_url("https://zone.example.com", &uri),
            "wss://zone.example.com/ws/chats/1"
        );
        assert_eq!(
            ws_upstream_url("http://localhost:8000", &uri),
            "ws://localhost:8000/ws/chats/1"
        );
    }

    #[test]
    fn uses_cleartext_for_loopback_https() {
        let uri: Uri = "/api/auth/register".parse().unwrap();
        assert_eq!(
            upstream_url("https://manager.localhost", &uri),
            "http://manager.localhost/api/auth/register"
        );
        assert_eq!(
            upstream_url("https://manager.localhost:443/", &uri),
            "http://manager.localhost/api/auth/register"
        );
        assert_eq!(
            upstream_url("https://localhost", &uri),
            "http://localhost/api/auth/register"
        );
        assert_eq!(
            upstream_url("https://127.0.0.1", &uri),
            "http://127.0.0.1/api/auth/register"
        );
        assert_eq!(
            upstream_url("https://[::1]", &uri),
            "http://[::1]/api/auth/register"
        );
        assert_eq!(
            upstream_url("https://[::1]:443", &uri),
            "http://[::1]/api/auth/register"
        );
    }

    #[test]
    fn keeps_remote_https_and_custom_local_ports() {
        let uri: Uri = "/api/auth/login".parse().unwrap();
        assert_eq!(
            upstream_url("https://zone.example.com/", &uri),
            "https://zone.example.com/api/auth/login"
        );
        assert_eq!(
            upstream_url("https://localhost:8443", &uri),
            "https://localhost:8443/api/auth/login"
        );
        assert_eq!(
            effective_proxy_target("http://manager.localhost"),
            "http://manager.localhost"
        );
    }

    #[test]
    fn converts_loopback_wss_to_ws() {
        let uri: Uri = "/ws/chats/1".parse().unwrap();
        assert_eq!(
            ws_upstream_url("https://manager.localhost", &uri),
            "ws://manager.localhost/ws/chats/1"
        );
    }
}
