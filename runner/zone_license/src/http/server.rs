use std::future::Future;
use std::io;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;

use crate::serve_error::ServeError;

#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    router: Router,
}

impl Server {
    pub async fn bind(address: SocketAddr, router: Router) -> Result<Self, ServeError> {
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| ServeError::Bind { address, source })?;
        Ok(Self { listener, router })
    }

    pub fn address(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until `shutdown` resolves, then lets requests in flight finish,
    /// so an email being sent is not cut off half way. Handlers can read the
    /// peer address through `ConnectInfo<SocketAddr>`.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), ServeError> {
        let service = self
            .router
            .into_make_service_with_connect_info::<SocketAddr>();
        axum::serve(self.listener, service)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(ServeError::Serve)
    }
}

#[cfg(test)]
mod tests;
