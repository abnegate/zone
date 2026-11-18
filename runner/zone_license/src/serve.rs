use std::future::Future;
use std::io;
use std::sync::Arc;

use zone_enterprise::Keyring;

use crate::config::Config;
use crate::http::{Context, Server, router};
use crate::mail::Relay;
use crate::serve_error::ServeError;

/// Checks the whole configuration before listening, so a bad setting stops the
/// process at startup instead of failing a paying customer's delivery.
pub fn run(keyring: &Keyring) -> Result<(), ServeError> {
    let config = Config::from_environment(keyring)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?
        .block_on(async {
            let stopped = signals().map_err(ServeError::Signal)?;
            serve(config, stopped).await
        })
}

/// Serves until `shutdown` resolves, then lets deliveries in flight finish.
pub async fn serve(
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    start(config).await?.run(shutdown).await?;
    tracing::info!("Stopped");
    Ok(())
}

/// Builds the relay and binds the listener, without serving yet.
pub async fn start(config: Config) -> Result<Server, ServeError> {
    let relay = Relay::new(&config.smtp)?;
    let key_id = config.signer.key_id().to_owned();
    let products = config.products.len();
    let context = Context::new(
        config.signer,
        config.secret,
        config.products,
        Arc::new(relay),
    );
    let server = Server::bind(config.listen, router(context)).await?;
    tracing::info!(
        address = %config.listen,
        key_id,
        products,
        relay = %config.smtp.host,
        "Listening for Polar webhooks"
    );
    Ok(server)
}

/// The container runs the signer as PID 1, which the kernel gives no default
/// SIGTERM handler, so without this `docker stop` waits out its timeout.
#[cfg(unix)]
fn signals() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => tracing::info!("Stopping on SIGTERM"),
            _ = interrupt.recv() => tracing::info!("Stopping on ctrl-c"),
        }
    })
}

#[cfg(not(unix))]
fn signals() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    Ok(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Stopping on ctrl-c");
        }
    })
}
