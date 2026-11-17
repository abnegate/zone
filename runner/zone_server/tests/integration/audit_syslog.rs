use std::fs;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;
use zone_enterprise::audit::destination::{
    Facility, Protocol, SecretName, SyslogSecrets, SyslogSettings,
};
use zone_enterprise::audit::{Error, Event, FailureCode, VERSION};
use zone_server::db::audit::scope::Scope;
use zone_server::services::audit::transport::syslog::{Syslog, check};
use zone_server::services::audit::transport::{Batch, Transport};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audit");
const LOOPBACK: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";
const HOSTNAME: &str = "zone-test";
const BOM: char = '\u{FEFF}';
const WAIT: Duration = Duration::from_secs(15);
const PRIORITIES: [&str; 3] = ["<110>", "<109>", "<108>"];
const GARBAGE: &str =
    "-----BEGIN CERTIFICATE-----\nnot base64 at all!\n-----END CERTIFICATE-----\n";
const NOT_DER: &str = "-----BEGIN CERTIFICATE-----\nAAECAwQF\n-----END CERTIFICATE-----\n";
const NOT_KEY: &str = "-----BEGIN PRIVATE KEY-----\nAAECAwQF\n-----END PRIVATE KEY-----\n";

struct Received {
    bytes: Vec<u8>,
    peer: Option<Vec<CertificateDer<'static>>>,
}

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("{FIXTURES}/{name}")).expect("the audit TLS fixture exists")
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(aws_lc_rs::default_provider())
}

fn certificates(name: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(fixture(name).as_bytes())
        .collect::<Result<_, _>>()
        .expect("the fixture holds PEM certificates")
}

fn authority() -> Arc<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates("ca.pem") {
        roots
            .add(certificate)
            .expect("the test CA is a trust anchor");
    }
    Arc::new(roots)
}

fn settings(host: &str, port: u16, protocol: Protocol) -> SyslogSettings {
    SyslogSettings {
        host: host.to_owned(),
        port,
        protocol,
        facility: Facility::default(),
        hostname: HOSTNAME.to_owned(),
    }
}

fn trusting(ca: &str) -> SyslogSecrets {
    SyslogSecrets {
        ca_certificate: Some(fixture(ca)),
        ..SyslogSecrets::default()
    }
}

fn identified() -> SyslogSecrets {
    SyslogSecrets {
        client_certificate: Some(fixture("client.pem")),
        client_key: Some(fixture("client.key")),
        ..trusting("ca.pem")
    }
}

fn events() -> Vec<Event> {
    [
        "user.login",
        "audit_destination.deleted",
        "agent_policy.tool_denied",
    ]
    .into_iter()
    .zip(0..)
    .map(|(action, second)| Event {
        id: Uuid::new_v4(),
        version: VERSION,
        created_at: Utc.with_ymd_and_hms(2026, 11, 17, 9, 30, second).unwrap(),
        organization_id: Some(Uuid::new_v4()),
        workspace_id: None,
        actor_id: Some(Uuid::new_v4()),
        actor_email: Some(format!("actor{second}@example.com")),
        action: action.to_owned(),
        resource_type: "audit_destination".to_owned(),
        resource_id: Some(Uuid::new_v4()),
        old_values: None,
        new_values: Some(json!({"name": format!("SIEM \"{second}\"]")})),
        ip_address: Some("192.0.2.10".to_owned()),
        user_agent: Some("caf\u{e9}".to_owned()),
    })
    .collect()
}

fn batch(events: Vec<Event>) -> Batch {
    Batch {
        destination_id: Uuid::new_v4(),
        scope: Scope::Server,
        events,
    }
}

async fn receiver(clients: Option<Arc<RootCertStore>>) -> (u16, JoinHandle<io::Result<Received>>) {
    let builder = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("the provider supports the default versions");
    let builder = match clients {
        Some(roots) => builder.with_client_cert_verifier(
            WebPkiClientVerifier::builder_with_provider(roots, provider())
                .build()
                .expect("the client verifier builds"),
        ),
        None => builder.with_no_client_auth(),
    };
    let key = PrivateKeyDer::from_pem_slice(fixture("server.key").as_bytes())
        .expect("the server key is PEM");
    let config = builder
        .with_single_cert(certificates("server.pem"), key)
        .expect("the server certificate matches its key");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind((LOOPBACK, 0))
        .await
        .expect("a loopback port");
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (connection, _) = listener.accept().await?;
        let mut stream = acceptor.accept(connection).await?;
        let peer = stream
            .get_ref()
            .1
            .peer_certificates()
            .map(<[CertificateDer<'static>]>::to_vec);
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await?;
        Ok(Received { bytes, peer })
    });
    (port, handle)
}

async fn received(handle: JoinHandle<io::Result<Received>>) -> io::Result<Received> {
    timeout(WAIT, handle)
        .await
        .expect("the receiver finishes")
        .expect("the receiver task does not panic")
}

fn frames(stream: &[u8]) -> Vec<String> {
    let mut frames = Vec::new();
    let mut rest = stream;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|byte| *byte == b' ')
            .expect("every frame starts with its octet count");
        let length: usize = std::str::from_utf8(&rest[..space])
            .unwrap()
            .parse()
            .expect("the octet count is a number");
        let end = space + 1 + length;
        frames.push(String::from_utf8(rest[space + 1..end].to_vec()).expect("frames are UTF-8"));
        rest = &rest[end..];
    }
    frames
}

fn parsed(message: &str) -> Event {
    let (_, body) = message
        .split_once(BOM)
        .expect("the message body follows a BOM");
    serde_json::from_str(body).expect("the message body is an event")
}

fn assert_messages(messages: &[String], events: &[Event]) {
    assert_eq!(messages.len(), events.len());
    for ((message, event), priority) in messages.iter().zip(events).zip(PRIORITIES) {
        assert!(message.starts_with(&format!("{priority}1 ")), "{message}");
        assert_eq!(message.split(' ').nth(2), Some(HOSTNAME));
        assert_eq!(&parsed(message), event);
    }
}

async fn failure(settings: &SyslogSettings, secrets: &SyslogSecrets) -> FailureCode {
    Syslog::new(settings, secrets)
        .expect("the transport builds")
        .deliver(&batch(events()))
        .await
        .expect_err("the delivery fails")
        .code
}

#[tokio::test]
async fn tls_frames_every_event_with_its_octet_count() {
    let (port, handle) = receiver(None).await;
    let events = events();
    Syslog::new(
        &settings(LOOPBACK, port, Protocol::Tls),
        &trusting("ca.pem"),
    )
    .expect("the transport builds")
    .deliver(&batch(events.clone()))
    .await
    .expect("the receiver accepts the batch");

    let received = received(handle).await.expect("the stream closes cleanly");
    assert!(received.peer.is_none());
    assert_messages(&frames(&received.bytes), &events);
}

#[tokio::test]
async fn udp_sends_one_datagram_per_event() {
    let socket = UdpSocket::bind((LOOPBACK, 0))
        .await
        .expect("a loopback port");
    let port = socket.local_addr().unwrap().port();
    let events = events();
    Syslog::new(
        &settings(LOOPBACK, port, Protocol::Udp),
        &SyslogSecrets::default(),
    )
    .expect("the transport builds")
    .deliver(&batch(events.clone()))
    .await
    .expect("the datagrams are sent");

    let mut messages = Vec::new();
    let mut buffer = vec![0; usize::from(u16::MAX)];
    for _ in &events {
        let length = timeout(WAIT, socket.recv(&mut buffer))
            .await
            .expect("a datagram arrives")
            .expect("the datagram is readable");
        messages.push(String::from_utf8(buffer[..length].to_vec()).expect("datagrams are UTF-8"));
    }
    assert_messages(&messages, &events);
}

#[tokio::test]
async fn tls_refuses_a_server_outside_the_configured_ca() {
    let (port, handle) = receiver(None).await;
    assert_eq!(
        failure(
            &settings(LOOPBACK, port, Protocol::Tls),
            &trusting("other-ca.pem")
        )
        .await,
        FailureCode::Tls
    );
    assert!(received(handle).await.is_err());
}

#[tokio::test]
async fn tls_without_a_ca_refuses_a_privately_signed_server() {
    let (port, handle) = receiver(None).await;
    assert_eq!(
        failure(
            &settings(LOOPBACK, port, Protocol::Tls),
            &SyslogSecrets::default()
        )
        .await,
        FailureCode::Tls
    );
    assert!(received(handle).await.is_err());
}

#[tokio::test]
async fn tls_presents_the_client_certificate_a_server_requires() {
    let (port, handle) = receiver(Some(authority())).await;
    let _ = Syslog::new(
        &settings(LOOPBACK, port, Protocol::Tls),
        &trusting("ca.pem"),
    )
    .expect("the transport builds")
    .deliver(&batch(events()))
    .await;
    assert!(
        received(handle).await.is_err(),
        "the server refuses a client without a certificate"
    );

    let (port, handle) = receiver(Some(authority())).await;
    let events = events();
    Syslog::new(&settings(LOCALHOST, port, Protocol::Tls), &identified())
        .expect("the transport builds")
        .deliver(&batch(events.clone()))
        .await
        .expect("the receiver accepts the batch");

    let received = received(handle).await.expect("the stream closes cleanly");
    assert_eq!(received.peer, Some(certificates("client.pem")));
    assert_messages(&frames(&received.bytes), &events);
}

#[tokio::test]
async fn metadata_hosts_are_never_reached() {
    for host in ["169.254.169.254", "metadata.google.internal", "fe80::1"] {
        for protocol in Protocol::ALL {
            assert_eq!(
                failure(&settings(host, 514, protocol), &SyslogSecrets::default()).await,
                FailureCode::Target,
                "{host} over {}",
                protocol.as_str()
            );
        }
    }
}

#[tokio::test]
async fn a_refused_connection_is_a_connect_failure() {
    let listener = TcpListener::bind((LOOPBACK, 0))
        .await
        .expect("a loopback port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    assert_eq!(
        failure(
            &settings(LOOPBACK, port, Protocol::Tls),
            &trusting("ca.pem")
        )
        .await,
        FailureCode::Connect
    );
}

#[tokio::test]
async fn an_empty_batch_opens_no_connection() {
    Syslog::new(
        &settings("169.254.169.254", 514, Protocol::Tls),
        &trusting("ca.pem"),
    )
    .expect("the transport builds")
    .deliver(&batch(Vec::new()))
    .await
    .expect("an empty batch needs no connection");
}

#[test]
fn check_accepts_the_fixtures() {
    assert_eq!(check(&SyslogSecrets::default()), Ok(()));
    assert_eq!(check(&trusting("ca.pem")), Ok(()));
    assert_eq!(check(&identified()), Ok(()));
}

#[test]
fn check_refuses_a_ca_that_does_not_parse() {
    for ca in [
        GARBAGE.to_owned(),
        NOT_DER.to_owned(),
        String::new(),
        "not pem".to_owned(),
        fixture("client.key"),
    ] {
        let secrets = SyslogSecrets {
            ca_certificate: Some(ca.clone()),
            ..SyslogSecrets::default()
        };
        assert_eq!(
            check(&secrets),
            Err(Error::CertificateInvalid(SecretName::CaCertificate)),
            "{ca:?}"
        );
        let failure = Syslog::new(&settings(LOOPBACK, 6514, Protocol::Tls), &secrets)
            .err()
            .expect("the transport refuses the CA");
        assert_eq!(failure.code, FailureCode::Configuration);
        assert_eq!(
            failure.message,
            Error::CertificateInvalid(SecretName::CaCertificate).to_string()
        );
    }
}

#[test]
fn check_refuses_a_client_identity_that_does_not_parse_or_match() {
    for (secrets, expected) in [
        (
            SyslogSecrets {
                client_certificate: Some(GARBAGE.to_owned()),
                ..identified()
            },
            Error::CertificateInvalid(SecretName::ClientCertificate),
        ),
        (
            SyslogSecrets {
                client_certificate: Some(NOT_DER.to_owned()),
                ..identified()
            },
            Error::CertificateInvalid(SecretName::ClientCertificate),
        ),
        (
            SyslogSecrets {
                client_key: Some(NOT_KEY.to_owned()),
                ..identified()
            },
            Error::CertificateInvalid(SecretName::ClientKey),
        ),
        (
            SyslogSecrets {
                client_key: Some(fixture("server.key")),
                ..identified()
            },
            Error::CertificateInvalid(SecretName::ClientKey),
        ),
        (
            SyslogSecrets {
                client_key: None,
                ..identified()
            },
            Error::CertificateUnpaired,
        ),
    ] {
        assert_eq!(check(&secrets), Err(expected));
    }
}
