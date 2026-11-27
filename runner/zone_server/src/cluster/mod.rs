//! The cross-replica bus: sealed envelopes over Valkey pub/sub, or an
//! in-process loopback when no Valkey is configured.

mod bus;
mod channel;
mod clock;
mod command;
mod ended;
mod envelope;
mod kind;
mod loopback;
mod malformed;
mod namespace;
mod pacer;
mod packet;
mod pending;
#[cfg(test)]
mod recording;
mod rejection;
mod sealer;
mod seen;
mod shared;
mod status;
mod subkey;
mod subscription;
mod topic;
mod transport;
mod unverified;
mod valkey;

pub use bus::Bus;
pub use envelope::Envelope;
pub use kind::Kind;
pub use loopback::Loopback;
pub use malformed::Malformed;
pub use namespace::Namespace;
pub use pacer::Pacer;
pub use subscription::Subscription;
pub use topic::Topic;
pub use valkey::LOG_INTERVAL;

/// Makes aws-lc-rs the process's rustls provider. The dependency tree enables
/// both of rustls's providers, so without this the first TLS connection that
/// relies on the default, such as a `rediss://` Valkey, panics.
pub fn install_tls_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_tls_valkey_on_a_closed_port_fails_to_connect_instead_of_panicking() {
        install_tls_provider();
        install_tls_provider();
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let client = redis::Client::open(format!("rediss://127.0.0.1:{port}")).unwrap();

        let error = client
            .get_multiplexed_async_connection()
            .await
            .expect_err("nothing listens on the port");

        assert!(
            error.is_connection_refusal() || error.is_io_error(),
            "expected a connection error, got {error}"
        );
    }
}
