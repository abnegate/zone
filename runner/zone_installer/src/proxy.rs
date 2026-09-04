//! Reverse-proxy the manager API and WebSockets to a Zone server.

use axum::{
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
    format!("{}{path}", base.trim_end_matches('/'))
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
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach Zone server at {}", state.proxy_target()),
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

pub fn http_client() -> Client {
    Client::builder()
        .danger_accept_invalid_certs(false)
        .build()
        .expect("HTTP client")
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
}
