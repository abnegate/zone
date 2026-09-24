//! The listener claude.com sends an admin's browser back to, `GET /callback?code=…&state=…`. It
//! parks the code under a one-time receipt and sends the browser on to the console that started
//! the sign-in; it never exchanges a code itself.

mod limits;
mod page;

use std::env;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, Request};
use axum::http::header::{self, HOST};
use axum::http::uri::Authority;
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::claude::{CALLBACK_PATH, LOOPBACK_HOST, Reply};
use super::oauth;
use crate::config::Callback;
use limits::Limits;
use page::{NO_REFERRER, NO_STORE, Page};

const DEFAULT_PORT: u16 = 80;
const CONTAINER_MARKERS: [&str; 2] = ["/.dockerenv", "/run/.containerenv"];
const KUBERNETES: &str = "KUBERNETES_SERVICE_HOST";

/// Listens where `callback` says.
pub async fn bind(callback: &Callback) -> std::io::Result<TcpListener> {
    TcpListener::bind(callback.bind).await
}

/// Whether the listener answers other machines outside a container, where no port mapping needs
/// it to.
pub fn exposed(callback: &Callback) -> bool {
    beyond_loopback(callback, contained())
}

/// Answers the browsers claude.com sends to `localhost:<port>` of `callback` on `listener`, until
/// the server stops.
pub async fn serve(listener: TcpListener, callback: Callback) {
    serve_with(listener, callback.port, Limits::default()).await;
}

async fn serve_with(listener: TcpListener, port: u16, limits: Limits) {
    let open = router(port);
    let busy = Router::new().fallback(|| async { Page::busy() });
    let connections = Arc::new(Semaphore::new(limits.connections));
    let refusals = Arc::new(Semaphore::new(limits.refusals));
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                tracing::warn!(%error, "The Claude sign-in callback could not accept a connection");
                tokio::time::sleep(limits.backoff).await;
                continue;
            }
        };
        if let Ok(permit) = Arc::clone(&connections).try_acquire_owned() {
            tokio::spawn(answer(stream, open.clone(), limits, permit));
        } else if let Ok(permit) = Arc::clone(&refusals).try_acquire_owned() {
            tokio::spawn(answer(stream, busy.clone(), limits, permit));
        }
    }
}

/// Serves one request on `stream`, holding `permit` until the connection closes.
async fn answer(stream: TcpStream, router: Router, limits: Limits, permit: OwnedSemaphorePermit) {
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_read)
        .keep_alive(false)
        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(router));
    match tokio::time::timeout(limits.lifetime, connection).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::debug!(%error, "A Claude sign-in callback connection failed");
        }
        Err(_) => tracing::debug!("A Claude sign-in callback connection outlived its limit"),
    }
    drop(permit);
}

fn router(port: u16) -> Router {
    Router::new()
        .route(CALLBACK_PATH, get(receive).head(refuse))
        .layer(middleware::from_fn(
            move |request: Request, next: Next| async move {
                if addressed(&request, port) {
                    next.run(request).await
                } else {
                    Page::misdirected().into_response()
                }
            },
        ))
}

/// A `HEAD` would otherwise run the `GET` handler and spend the sign-in without showing anyone
/// the page.
async fn refuse() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn receive(uri: Uri) -> Response {
    let reply = Query::<Vec<(String, String)>>::try_from_uri(&uri)
        .ok()
        .and_then(|Query(pairs)| Reply::read(&pairs));
    let Some(reply) = reply else {
        tracing::warn!("The Claude sign-in callback was sent something claude.com does not send");
        return Page::unreadable().into_response();
    };
    match oauth::receive(reply).await {
        Ok(onward) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, onward.as_str()),
                (header::CACHE_CONTROL, NO_STORE),
                (header::REFERRER_POLICY, NO_REFERRER),
            ],
        )
            .into_response(),
        Err(error) => Page::failed(&error).into_response(),
    }
}

/// Whether `request` is addressed to `localhost:<port>`, as the sign-in's redirect names it.
fn addressed(request: &Request, port: u16) -> bool {
    request.uri().authority().is_none()
        && request
            .headers()
            .get(HOST)
            .and_then(|host| host.to_str().ok())
            .and_then(|host| host.parse::<Authority>().ok())
            .is_some_and(|authority| {
                authority.host().eq_ignore_ascii_case(LOOPBACK_HOST)
                    && authority.port_u16().unwrap_or(DEFAULT_PORT) == port
            })
}

fn beyond_loopback(callback: &Callback, contained: bool) -> bool {
    !callback.bind.ip().is_loopback() && !contained
}

fn contained() -> bool {
    CONTAINER_MARKERS
        .iter()
        .any(|marker| Path::new(marker).exists())
        || env::var_os(KUBERNETES).is_some()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
    use std::time::{Duration, Instant};

    use reqwest::Url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::timeout;
    use uuid::Uuid;

    use super::*;
    use crate::services::login::caller::Caller;
    use crate::services::login::claude::{Redirect, Scope};
    use crate::services::login::console::{Console, RECEIPT};
    use crate::services::login::pending;

    const TEST_NET: &str = "192.0.2.1:9";
    const PROBE: Duration = Duration::from_secs(2);
    const WAIT: Duration = Duration::from_secs(10);
    const PAUSE: Duration = Duration::from_millis(50);
    const CONSOLE: &str = "http://localhost:3000";
    const CODE: &str = "fake-authorization-code";

    /// An address of this machine other than loopback, when it has one: the one it would send
    /// from to a documentation address, which sends nothing.
    fn outward() -> Option<IpAddr> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        socket.connect(TEST_NET).ok()?;
        let address = socket.local_addr().ok()?.ip();
        (!address.is_loopback() && !address.is_unspecified()).then_some(address)
    }

    /// A listener on a loopback port of its own, answering as `localhost:<port>` within `limits`.
    async fn listening(limits: Limits) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("a loopback port");
        let address = listener.local_addr().expect("a bound address");
        tokio::spawn(serve_with(listener, address.port(), limits));
        address
    }

    /// The state of a new loopback sign-in to a new organization, returning to `address`.
    async fn started(address: SocketAddr) -> String {
        let caller = Caller {
            user: Uuid::new_v4(),
            email: "admin@example.com".to_string(),
            session: Uuid::new_v4(),
        };
        let started = oauth::start(
            Uuid::new_v4(),
            &caller,
            Scope::Inference,
            Redirect::Loopback(address.port()),
            Console::at(CONSOLE),
        )
        .await;
        Url::parse(&started.url)
            .expect("an authorize URL")
            .query_pairs()
            .find(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .expect("the sign-in's state")
    }

    fn callback_path(state: &str) -> String {
        format!("{CALLBACK_PATH}?code={CODE}&state={state}")
    }

    /// The status and the whole response the listener at `address` sends for `GET target`
    /// addressed to `host`.
    async fn ask(address: SocketAddr, host: &str, target: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(address)
            .await
            .expect("the listener accepts");
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
            .await
            .expect("the request is sent");
        let mut response = String::new();
        timeout(WAIT, stream.read_to_string(&mut response))
            .await
            .expect("the listener answers in time")
            .expect("a readable response");
        let status = response
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {response:?}"));
        (status, response)
    }

    fn location(response: &str) -> Option<&str> {
        response
            .lines()
            .find_map(|line| line.strip_prefix("location: "))
    }

    #[tokio::test]
    async fn a_loopback_bind_answers_on_loopback_only() {
        let listener = bind(&Callback {
            port: 54_545,
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        })
        .await
        .expect("a loopback listener");
        let address = listener.local_addr().expect("a bound address");

        assert_eq!(address.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(
            TcpStream::connect(address).await.is_ok(),
            "loopback cannot reach the listener"
        );
        if let Some(outward) = outward() {
            let reached = timeout(
                PROBE,
                TcpStream::connect(SocketAddr::new(outward, address.port())),
            )
            .await;
            assert!(
                !matches!(reached, Ok(Ok(_))),
                "the listener answered on {outward}, which other machines reach"
            );
        }
    }

    #[test]
    fn a_bind_beyond_loopback_is_exposed_only_outside_a_container() {
        let bound = |address: IpAddr| Callback {
            port: 54_545,
            bind: SocketAddr::new(address, 54_545),
        };
        let everywhere = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

        assert!(beyond_loopback(&bound(everywhere), false));
        assert!(!beyond_loopback(&bound(everywhere), true));
        assert!(!beyond_loopback(
            &bound(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            false
        ));
        assert!(!beyond_loopback(
            &bound(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
            false
        ));
    }

    #[tokio::test]
    async fn a_request_addressed_to_any_other_host_is_refused_and_spends_nothing() {
        let address = listening(Limits::default()).await;
        let state = started(address).await;
        let port = address.port();

        for host in [
            format!("127.0.0.1:{port}"),
            format!("attacker.example:{port}"),
            format!("localhost.attacker.example:{port}"),
            format!("localhost:{}", port.wrapping_add(1)),
            "localhost".to_string(),
        ] {
            let (status, response) = ask(address, &host, &callback_path(&state)).await;

            assert_eq!(status, 421, "{host}: {response}");
            assert!(location(&response).is_none(), "{host}: {response}");
        }

        let (status, response) = ask(
            address,
            &format!("LocalHost:{port}"),
            &callback_path(&state),
        )
        .await;
        assert_eq!(status, 303, "{response}");
        let onward = location(&response).expect("where the browser goes next");
        assert!(
            onward.starts_with(&format!("{CONSOLE}/agent-sign-in?")),
            "{onward}"
        );
        assert!(
            !onward.contains(CODE) && !onward.contains(&state),
            "the console is sent the code or the state: {onward}"
        );
        for header in ["cache-control: no-store", "referrer-policy: no-referrer"] {
            assert!(response.contains(header), "{header} is missing: {response}");
        }
    }

    #[tokio::test]
    async fn a_busy_listener_turns_a_callback_away_without_spending_its_sign_in() {
        let address = listening(Limits {
            connections: 1,
            refusals: 1,
            header_read: WAIT,
            ..Limits::default()
        })
        .await;
        let state = started(address).await;
        let host = format!("localhost:{}", address.port());
        let idle = TcpStream::connect(address)
            .await
            .expect("an idle connection");

        let (status, response) = ask(address, &host, &callback_path(&state)).await;
        assert_eq!(status, 503, "{response}");
        assert!(response.contains("Zone is busy"), "{response}");

        drop(idle);
        let deadline = Instant::now() + WAIT;
        let (status, response) = loop {
            let (status, response) = ask(address, &host, &callback_path(&state)).await;
            if status != 503 || Instant::now() > deadline {
                break (status, response);
            }
            tokio::time::sleep(PAUSE).await;
        };
        assert_eq!(
            status, 303,
            "the sign-in turned away while the listener was busy was spent: {response}"
        );
        let onward = location(&response).expect("where the browser goes next");
        let receipt = Url::parse(onward)
            .expect("an absolute URL")
            .query_pairs()
            .find(|(name, _)| name == RECEIPT)
            .map(|(_, value)| value.into_owned())
            .expect("a receipt");
        let (_, code) = crate::services::login::receipts::take(&receipt).expect("the parked code");
        assert_eq!(code.value.expose(), CODE);
        assert!(pending::claim_loopback(&state).is_none());
    }

    #[tokio::test]
    async fn a_connection_past_every_limit_is_closed_unanswered() {
        let address = listening(Limits {
            connections: 1,
            refusals: 0,
            header_read: WAIT,
            ..Limits::default()
        })
        .await;
        let _idle = TcpStream::connect(address)
            .await
            .expect("an idle connection");
        let mut unanswered = TcpStream::connect(address)
            .await
            .expect("a connection past every limit");

        let mut sent = Vec::new();
        timeout(WAIT, unanswered.read_to_end(&mut sent))
            .await
            .expect("a connection past every limit to be closed")
            .expect("a closed connection");

        assert!(
            sent.is_empty(),
            "a connection past every limit was answered: {}",
            String::from_utf8_lossy(&sent)
        );
    }

    #[tokio::test]
    async fn a_connection_that_never_finishes_its_headers_is_closed() {
        let header_read = Duration::from_millis(200);
        let address = listening(Limits {
            header_read,
            ..Limits::default()
        })
        .await;
        let mut stream = TcpStream::connect(address)
            .await
            .expect("the listener accepts");
        let opened = Instant::now();
        stream
            .write_all(b"GET /callback HTTP/1.1\r\nHost: localhost\r\n")
            .await
            .expect("half a request is sent");

        let mut sent = Vec::new();
        timeout(WAIT, stream.read_to_end(&mut sent))
            .await
            .expect("a connection that never finished its headers to be closed")
            .expect("a closed connection");

        assert!(
            opened.elapsed() >= header_read,
            "the connection closed before its headers were due"
        );
        let answered = String::from_utf8_lossy(&sent);
        assert!(
            sent.is_empty() || answered.starts_with("HTTP/1.1 408"),
            "{answered}"
        );
    }
}
