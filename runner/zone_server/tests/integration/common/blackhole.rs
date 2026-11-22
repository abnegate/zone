use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

/// A TCP proxy that can stop forwarding while keeping every socket open, the way a network
/// partition looks to both ends: nothing arrives, and nothing says the peer is gone.
pub struct Blackhole {
    port: u16,
    silent: Arc<AtomicBool>,
    _tasks: JoinSet<()>,
}

impl Blackhole {
    pub async fn start(host: String, port: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the proxy binds a port");
        let local = listener
            .local_addr()
            .expect("the proxy has an address")
            .port();
        let silent = Arc::new(AtomicBool::new(false));
        let mut tasks = JoinSet::new();
        let accepting = Arc::clone(&silent);
        tasks.spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                let Ok(upstream) = TcpStream::connect((host.as_str(), port)).await else {
                    continue;
                };
                let (client_read, client_write) = client.into_split();
                let (upstream_read, upstream_write) = upstream.into_split();
                connections.spawn(forward(client_read, upstream_write, Arc::clone(&accepting)));
                connections.spawn(forward(upstream_read, client_write, Arc::clone(&accepting)));
            }
        });
        Self {
            port: local,
            silent,
            _tasks: tasks,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn silence(&self) {
        self.silent.store(true, Ordering::SeqCst);
    }
}

async fn forward(mut from: OwnedReadHalf, mut to: OwnedWriteHalf, silent: Arc<AtomicBool>) {
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        let read = from.read(&mut buffer).await.unwrap_or(0);
        if silent.load(Ordering::SeqCst) {
            if read == 0 {
                pending::<()>().await;
            }
            continue;
        }
        if read == 0 || to.write_all(&buffer[..read]).await.is_err() {
            return;
        }
    }
}
