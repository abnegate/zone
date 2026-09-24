//! The listener claude.com sends an admin's browser back to when a callback is configured,
//! `GET /callback?code=…&state=…`, as it does for the claude CLI's own sign-in. Zone then finishes
//! the sign-in without anyone pasting a code.
//!
//! The request carries no Zone session, so its state is its only authority: Zone issued it for
//! this flow to one admin of one organization, and it finishes one sign-in within its window. The
//! listener serves nothing else, never redirects, repeats nothing from the request, and logs
//! neither the code nor the state.

mod page;

use std::io;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

pub use page::Page;

use super::claude::{CALLBACK_PATH, Reply};
use super::oauth;
use crate::config::Callback;
use crate::state::AppState;

/// How many callbacks are answered at once. A real one comes from an admin's browser and holds
/// its request open while Claude answers the exchange; any more are turned away, not queued.
const CONCURRENT: usize = 8;

/// Listens where `callback` says.
pub async fn bind(callback: &Callback) -> io::Result<TcpListener> {
    TcpListener::bind(callback.address()).await
}

/// Answers callbacks on `listener` until the server stops.
pub async fn serve(state: AppState, listener: TcpListener) -> io::Result<()> {
    axum::serve(listener, router(state)).await
}

pub fn router(state: AppState) -> Router {
    let permits = Arc::new(Semaphore::new(CONCURRENT));
    Router::new()
        .route(CALLBACK_PATH, get(receive).head(refuse))
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let permits = Arc::clone(&permits);
            async move { admit(&permits, request, next).await }
        }))
        .with_state(state)
}

async fn admit(permits: &Semaphore, request: Request, next: Next) -> Response {
    match permits.try_acquire() {
        Ok(_permit) => next.run(request).await,
        Err(_) => Page::busy().into_response(),
    }
}

/// A `HEAD` would otherwise run the `GET` handler and spend the sign-in without showing anyone
/// the page.
async fn refuse() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn receive(State(state): State<AppState>, uri: Uri) -> Page {
    let reply = Query::<Vec<(String, String)>>::try_from_uri(&uri)
        .ok()
        .and_then(|Query(pairs)| Reply::read(&pairs));
    let Some(reply) = reply else {
        tracing::warn!("The Claude sign-in callback was sent something claude.com does not send");
        return Page::unreadable();
    };
    match oauth::receive(&state, reply).await {
        Ok(organization) => {
            tracing::info!(%organization, "Claude signed in through the sign-in callback");
            Page::signed_in()
        }
        Err(error) => Page::failed(&error),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

    use tokio::net::TcpStream;

    use super::*;

    const TEST_NET: &str = "192.0.2.1:9";

    /// An address of this machine other than loopback, when it has one: the one it would send
    /// from to a documentation address, which sends nothing.
    fn outward() -> Option<IpAddr> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        socket.connect(TEST_NET).ok()?;
        let address = socket.local_addr().ok()?.ip();
        (!address.is_loopback() && !address.is_unspecified()).then_some(address)
    }

    #[tokio::test]
    async fn the_listener_binds_loopback_unless_told_otherwise() {
        let listener = bind(&Callback::loopback(0))
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("a bound address");

        assert_eq!(address.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(
            TcpStream::connect(address).await.is_ok(),
            "loopback cannot reach the listener"
        );
        if let Some(outward) = outward() {
            assert!(
                TcpStream::connect(SocketAddr::new(outward, address.port()))
                    .await
                    .is_err(),
                "the listener answered on {outward}, which other machines reach"
            );
        }
    }
}
