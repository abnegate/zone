use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use abnegate_http::metadata;
use async_trait::async_trait;
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
use zone_enterprise::audit::destination::{Protocol, SecretName, SyslogSecrets, SyslogSettings};
use zone_enterprise::audit::syslog::{Header, UDP_BUDGET, message, octet_counted};
use zone_enterprise::audit::{Error, Event, FailureCode};

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
        let channel = match settings.protocol {
            Protocol::Tls => Channel::Tls {
                connector: TlsConnector::from(Arc::new(config(secrets)?)),
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
            .map_err(|error| {
                Failure::new(
                    FailureCode::Connect,
                    format!("Could not resolve {authority}: {error}"),
                )
            })?;
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
            .map_err(|error| {
                Failure::new(
                    FailureCode::Tls,
                    format!("The TLS handshake with {authority} failed: {error}"),
                )
            })?;
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

fn config(secrets: &SyslogSecrets) -> Result<ClientConfig, Failure> {
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
    Ok(match credentials.identity {
        Some(identity) => {
            builder.with_client_cert_resolver(Arc::new(SingleCertAndKey::from(identity)))
        }
        None => builder.with_no_client_auth(),
    })
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
        Err(rustls::Error::InconsistentKeys(_)) => {
            Err(Error::CertificateInvalid(SecretName::ClientKey))
        }
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
    let mut failure = refused(authority);
    for address in addresses {
        match timeout(TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(connection)) => return Ok(connection),
            Ok(Err(error)) => {
                failure = Failure::new(
                    FailureCode::Connect,
                    format!("Could not connect to {authority}: {error}"),
                );
            }
            Err(_) => failure = elapsed(authority),
        }
    }
    Err(failure)
}

async fn bounded<T>(authority: &str, future: impl Future<Output = T>) -> Result<T, Failure> {
    timeout(TIMEOUT, future)
        .await
        .map_err(|_| elapsed(authority))
}

fn elapsed(authority: &str) -> Failure {
    Failure::new(
        FailureCode::Timeout,
        format!(
            "{authority} did not answer within {} seconds.",
            TIMEOUT.as_secs()
        ),
    )
}

fn refused(authority: &str) -> Failure {
    Failure::new(
        FailureCode::Target,
        format!(
            "{authority} names or resolves only to link-local or cloud metadata addresses, which audit delivery never reaches."
        ),
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
    Failure::new(code, format!("Could not send to {authority}: {error}"))
}

fn unconfigurable(error: rustls::Error) -> Failure {
    Failure::new(
        FailureCode::Configuration,
        format!("The TLS client could not be configured: {error}"),
    )
}
