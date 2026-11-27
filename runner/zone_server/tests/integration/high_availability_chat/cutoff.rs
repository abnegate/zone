use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const CHUNK: usize = 16 * 1024;

/// A TCP proxy to Valkey a test can cut or hold. Cutting closes every
/// connection through it and refuses new ones until it is restored, as a
/// network partition between one replica and Valkey would. Holding keeps every
/// connection open but stops what Valkey sends the replica until it is
/// released, as a slow link would: nothing is lost, only late.
pub struct Cutoff {
    port: u16,
    open: watch::Sender<bool>,
    flowing: watch::Sender<bool>,
    listener: JoinHandle<()>,
}

impl Cutoff {
    pub async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port for the proxy");
        let port = listener.local_addr().expect("the proxy's address").port();
        let (open, _) = watch::channel(true);
        let (flowing, _) = watch::channel(true);
        let gate = open.clone();
        let valve = flowing.clone();
        let listener = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                if !*gate.borrow() {
                    continue;
                }
                let mut closed = gate.subscribe();
                let flowing = valve.subscribe();
                tokio::spawn(async move {
                    let Ok(upstream) = TcpStream::connect(target).await else {
                        return;
                    };
                    tokio::select! {
                        () = relay(client, upstream, flowing) => {}
                        _ = closed.wait_for(|open| !*open) => {}
                    }
                });
            }
        });
        Self {
            port,
            open,
            flowing,
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

    pub fn hold(&self) {
        self.flowing.send_replace(false);
    }

    pub fn release(&self) {
        self.flowing.send_replace(true);
    }
}

impl Drop for Cutoff {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

/// Copies bytes both ways until either side closes, passing what Valkey sends
/// on only while `flowing` says so.
async fn relay(client: TcpStream, upstream: TcpStream, mut flowing: watch::Receiver<bool>) {
    let (mut client_read, mut client_write) = client.into_split();
    let (mut upstream_read, mut upstream_write) = upstream.into_split();
    let up = tokio::io::copy(&mut client_read, &mut upstream_write);
    let down = async {
        let mut buffer = vec![0; CHUNK];
        loop {
            let read = match upstream_read.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => read,
            };
            if flowing.wait_for(|flowing| *flowing).await.is_err()
                || client_write.write_all(&buffer[..read]).await.is_err()
            {
                return;
            }
        }
    };
    tokio::select! {
        _ = up => {}
        () = down => {}
    }
}
