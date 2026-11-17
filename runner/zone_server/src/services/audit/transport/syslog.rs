use std::fmt::Display;
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use abnegate_http::metadata;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::sign::{CertifiedKey, SingleCertAndKey};
use rustls::{ClientConfig, InconsistentKeys, RootCertStore};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket, lookup_host};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use uuid::Uuid;
use zone_enterprise::audit::destination::{Protocol, SecretName, SyslogSecrets, SyslogSettings};
use zone_enterprise::audit::syslog::{Header, UDP_BUDGET, message, octet_counted};
use zone_enterprise::audit::{Error, Event, FailureCode};

use super::tls::Configurations;
use super::{Batch, Failure, Transport};

pub const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Syslog {
    settings: SyslogSettings,
    channel: Channel,
}

enum Channel {
    Tls {
        connector: TlsConnector,
        server: ServerName<'static>,
    },
    Udp,
}

impl Syslog {
    pub fn new(settings: &SyslogSettings, secrets: &SyslogSecrets) -> Result<Self, Failure> {
        Self::configured(settings, || config(secrets))
    }

    /// As `new`, reusing the TLS configuration built for this version of destination `id`.
    pub fn reusing(
        settings: &SyslogSettings,
        secrets: &SyslogSecrets,
        configurations: &Configurations,
        id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, Failure> {
        Self::configured(settings, || {
            configurations.get(id, updated_at, || config(secrets))
        })
    }

    fn configured(
        settings: &SyslogSettings,
        config: impl FnOnce() -> Result<Arc<ClientConfig>, Failure>,
    ) -> Result<Self, Failure> {
        let channel = match settings.protocol {
            Protocol::Tls => Channel::Tls {
                connector: TlsConnector::from(config()?),
                server: ServerName::try_from(settings.host.clone()).map_err(|_| {
                    Failure::new(
                        FailureCode::Configuration,
                        format!(
                            "{} is not a name a TLS certificate can carry.",
                            settings.host
                        ),
                    )
                })?,
            },
            Protocol::Udp => Channel::Udp,
        };
        Ok(Self {
            settings: settings.clone(),
            channel,
        })
    }

    fn header(&self) -> Header<'_> {
        Header {
            facility: self.settings.facility.value(),
            hostname: &self.settings.hostname,
        }
    }

    async fn resolve(&self, authority: &str) -> Result<Vec<SocketAddr>, Failure> {
        if metadata::is_host(&self.settings.host) {
            return Err(refused(authority));
        }
        let resolved = bounded(authority, lookup_host(authority))
            .await?
            .map_err(|error| logged(FailureCode::Connect, authority, error))?;
        let addresses: Vec<SocketAddr> = resolved
            .filter(|address| !metadata::is_address(address.ip()))
            .collect();
        if addresses.is_empty() {
            return Err(refused(authority));
        }
        Ok(addresses)
    }

    async fn stream(
        &self,
        authority: &str,
        connector: &TlsConnector,
        server: &ServerName<'static>,
        addresses: &[SocketAddr],
        events: &[Event],
    ) -> Result<(), Failure> {
        let connection = connect(authority, addresses).await?;
        let mut tls = bounded(authority, connector.connect(server.clone(), connection))
            .await?
            .map_err(|error| logged(FailureCode::Tls, authority, error))?;
        let header = self.header();
        let mut frames = Vec::new();
        for event in events {
            frames.extend(octet_counted(&message(event, &header, None)));
        }
        bounded(authority, async {
            tls.write_all(&frames).await?;
            tls.flush().await?;
            tls.shutdown().await
        })
        .await?
        .map_err(|error| transfer(authority, error))
    }

    async fn send(
        &self,
        authority: &str,
        addresses: &[SocketAddr],
        events: &[Event],
    ) -> Result<(), Failure> {
        let target = addresses[0];
        let local: SocketAddr = if target.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(local)
            .await
            .map_err(|error| transfer(authority, error))?;
        socket
            .connect(target)
            .await
            .map_err(|error| transfer(authority, error))?;
        let header = self.header();
        for event in events {
            let datagram = message(event, &header, Some(UDP_BUDGET));
            bounded(authority, socket.send(datagram.as_bytes()))
                .await?
                .map_err(|error| transfer(authority, error))?;
        }
        Ok(())
    }
}

#[async_trait]
impl Transport for Syslog {
    async fn deliver(&self, batch: &Batch) -> Result<(), Failure> {
        if batch.events.is_empty() {
            return Ok(());
        }
        let authority = self.settings.authority();
        let addresses = self.resolve(&authority).await?;
        match &self.channel {
            Channel::Tls { connector, server } => {
                self.stream(&authority, connector, server, &addresses, &batch.events)
                    .await
            }
            Channel::Udp => self.send(&authority, &addresses, &batch.events).await,
        }
    }
}

pub fn check(secrets: &SyslogSecrets) -> Result<(), Error> {
    Credentials::parse(secrets, &provider())?;
    Ok(())
}

struct Credentials {
    roots: Option<RootCertStore>,
    identity: Option<CertifiedKey>,
}

impl Credentials {
    fn parse(secrets: &SyslogSecrets, provider: &CryptoProvider) -> Result<Self, Error> {
        let roots = secrets.ca_certificate.as_deref().map(roots).transpose()?;
        let identity = match (
            secrets.client_certificate.as_deref(),
            secrets.client_key.as_deref(),
        ) {
            (Some(certificate), Some(key)) => Some(identity(certificate, key, provider)?),
            (None, None) => None,
            _ => return Err(Error::CertificateUnpaired),
        };
        Ok(Self { roots, identity })
    }
}

fn provider() -> CryptoProvider {
    aws_lc_rs::default_provider()
}

fn config(secrets: &SyslogSecrets) -> Result<Arc<ClientConfig>, Failure> {
    let provider = provider();
    let credentials = Credentials::parse(secrets, &provider)
        .map_err(|error| Failure::new(FailureCode::Configuration, error.to_string()))?;
    let builder = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(unconfigurable)?;
    let builder = match credentials.roots {
        Some(roots) => builder.with_root_certificates(roots),
        None => builder.with_platform_verifier().map_err(unconfigurable)?,
    };
    Ok(Arc::new(match credentials.identity {
        Some(identity) => {
            builder.with_client_cert_resolver(Arc::new(SingleCertAndKey::from(identity)))
        }
        None => builder.with_no_client_auth(),
    }))
}

fn roots(pem: &str) -> Result<RootCertStore, Error> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates(pem, SecretName::CaCertificate)? {
        roots
            .add(certificate)
            .map_err(|_| Error::CertificateInvalid(SecretName::CaCertificate))?;
    }
    Ok(roots)
}

fn identity(
    certificate: &str,
    key: &str,
    provider: &CryptoProvider,
) -> Result<CertifiedKey, Error> {
    let chain = certificates(certificate, SecretName::ClientCertificate)?;
    let key = PrivateKeyDer::from_pem_slice(key.as_bytes())
        .ok()
        .and_then(|key| provider.key_provider.load_private_key(key).ok())
        .ok_or(Error::CertificateInvalid(SecretName::ClientKey))?;
    let identity = CertifiedKey::new(chain, key);
    match identity.keys_match() {
        Ok(()) | Err(rustls::Error::InconsistentKeys(InconsistentKeys::Unknown)) => Ok(identity),
        Err(rustls::Error::InconsistentKeys(_)) => Err(Error::CertificateMismatched),
        Err(_) => Err(Error::CertificateInvalid(SecretName::ClientCertificate)),
    }
}

fn certificates(pem: &str, name: SecretName) -> Result<Vec<CertificateDer<'static>>, Error> {
    let certificates = CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::CertificateInvalid(name))?;
    if certificates.is_empty() {
        return Err(Error::CertificateInvalid(name));
    }
    Ok(certificates)
}

async fn connect(authority: &str, addresses: &[SocketAddr]) -> Result<TcpStream, Failure> {
    let mut failure = None;
    for address in addresses {
        match timeout(TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(connection)) => return Ok(connection),
            Ok(Err(error)) => failure = Some(logged(FailureCode::Connect, authority, error)),
            Err(_) => failure = Some(elapsed(authority)),
        }
    }
    Err(failure.unwrap_or_else(|| refused(authority)))
}

async fn bounded<T>(authority: &str, future: impl Future<Output = T>) -> Result<T, Failure> {
    timeout(TIMEOUT, future)
        .await
        .map_err(|_| elapsed(authority))
}

fn elapsed(authority: &str) -> Failure {
    logged(
        FailureCode::Timeout,
        authority,
        format_args!("no answer within {} seconds", TIMEOUT.as_secs()),
    )
}

fn refused(authority: &str) -> Failure {
    logged(
        FailureCode::Target,
        authority,
        "only link-local or cloud metadata addresses",
    )
}

fn transfer(authority: &str, error: io::Error) -> Failure {
    let code = if error
        .get_ref()
        .is_some_and(|inner| inner.is::<rustls::Error>())
    {
        FailureCode::Tls
    } else {
        FailureCode::Connect
    };
    logged(code, authority, error)
}

/// The fixed failure an organization's admins see; the cause, naming the address and port Zone
/// tried, goes only to the server log.
fn logged(code: FailureCode, authority: &str, cause: impl Display) -> Failure {
    tracing::warn!(%code, authority, %cause, "An audit syslog delivery failed");
    Failure::of(code)
}

fn unconfigurable(error: rustls::Error) -> Failure {
    tracing::error!(%error, "Could not configure the audit syslog TLS client");
    Failure::of(FailureCode::Configuration)
}
