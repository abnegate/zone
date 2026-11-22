use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A TCP proxy to Valkey a test can cut: cutting closes every connection
/// through it and refuses new ones until it is restored, as a network
/// partition between one replica and Valkey would.
pub struct Cutoff {
    port: u16,
    open: watch::Sender<bool>,
    listener: JoinHandle<()>,
}

impl Cutoff {
    pub async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port for the proxy");
        let port = listener.local_addr().expect("the proxy's address").port();
        let (open, _) = watch::channel(true);
        let gate = open.clone();
        let listener = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                if !*gate.borrow() {
                    continue;
                }
                let mut closed = gate.subscribe();
                tokio::spawn(async move {
                    let Ok(mut upstream) = TcpStream::connect(target).await else {
                        return;
                    };
                    let mut client = client;
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {}
                        _ = closed.wait_for(|open| !*open) => {}
                    }
                });
            }
        });
        Self {
            port,
            open,
            listener,
        }
    }

    pub fn url(&self) -> String {
        format!("redis://127.0.0.1:{}", self.port)
    }

    pub fn cut(&self) {
        self.open.send_replace(false);
    }

    pub fn restore(&self) {
        self.open.send_replace(true);
    }
}

impl Drop for Cutoff {
    fn drop(&mut self) {
        self.listener.abort();
    }
}
