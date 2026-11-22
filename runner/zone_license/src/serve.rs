use std::future::Future;
use std::io;
use std::sync::Arc;

use zone_enterprise::Keyring;

use crate::bundle::{BundleSettings, Presigner};
use crate::config::Config;
use crate::gate::{self, Gate, Limits, Throttle};
use crate::http::{Context, Distributor, Registrar, Renewer, Server, router};
use crate::mail::Relay;
use crate::polar::{self, Proxy};
use crate::registry::{RegistrySettings, TokenKey};
use crate::serve_error::ServeError;
use crate::signer::Signer;

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

/// Builds the relay and the Polar client and binds the listener, without
/// serving yet. Renewal is on only when `POLAR_ACCESS_TOKEN` is set, registry
/// tokens only when `ZONE_REGISTRY_SERVICE` is, and bundles only when
/// `ZONE_BUNDLE_BUCKET` is.
pub async fn start(config: Config) -> Result<Server, ServeError> {
    let relay = Relay::new(&config.smtp)?;
    let gate = Gate::new(
        config.keyring.clone(),
        config.revoked.clone(),
        config.proxies.clone(),
    );
    let registrar = config
        .registry
        .map(|settings| registrar(settings, &config.signer, gate.clone(), config.limits));
    let distributor = config
        .bundles
        .map(|settings| distributor(settings, gate, config.limits))
        .transpose()?;
    if registrar.is_some() || distributor.is_some() {
        gate::announce(&config.proxies);
    }
    let renewer = match config.access_token {
        Some(token) => Some(Renewer::new(
            config.keyring,
            polar::Client::new(token, polar::API, Proxy::System)?,
        )),
        None => None,
    };
    let renewal = renewer.is_some();
    let key_id = config.signer.key_id().to_owned();
    let products = config.products.len();
    for product in config.products.iter() {
        tracing::info!(
            product = product.id.as_str(),
            plan = %product.plan,
            "Issuing licenses for a Polar product"
        );
    }
    let context = Context::new(
        config.signer,
        config.secret,
        config.products,
        Arc::new(relay),
    );
    let context = match renewer {
        Some(renewer) => context.with_renewer(renewer),
        None => context,
    };
    let context = match registrar {
        Some(registrar) => context.with_registrar(registrar),
        None => context,
    };
    let context = match distributor {
        Some(distributor) => context.with_distributor(distributor),
        None => context,
    };
    let server = Server::bind(config.listen, router(context)).await?;
    tracing::info!(
        address = %config.listen,
        key_id,
        products,
        relay = %config.smtp.host,
        renewal,
        "Listening for Polar webhooks"
    );
    Ok(server)
}

fn registrar(settings: RegistrySettings, signer: &Signer, gate: Gate, limits: Limits) -> Registrar {
    let key = TokenKey::derive(signer, settings.generation);
    let throttle = Throttle::new(limits);
    drop(throttle.spawn_cleanup());
    let registrar = Registrar::new(gate, key, settings, throttle);
    tracing::info!(
        service = registrar.service(),
        key_id = registrar.key_id(),
        "Issuing registry tokens"
    );
    registrar
}

fn distributor(
    settings: BundleSettings,
    gate: Gate,
    limits: Limits,
) -> Result<Distributor, ServeError> {
    tracing::info!(
        bucket = settings.bucket.name(),
        prefix = settings.prefix.as_str(),
        "Serving air-gapped bundles"
    );
    let presigner = Presigner::new(settings, Proxy::System).map_err(ServeError::Storage)?;
    let throttle = Throttle::new(limits);
    drop(throttle.spawn_cleanup());
    Ok(Distributor::new(gate, presigner, throttle))
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

#[cfg(test)]
mod tests;
