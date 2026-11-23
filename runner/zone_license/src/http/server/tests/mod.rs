use axum::extract::ConnectInfo;
use axum::routing::get;
use tokio::sync::oneshot;

use super::*;

const PEER_PATH: &str = "/peer";

async fn peer(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> String {
    peer.ip().to_string()
}

#[tokio::test]
async fn a_handler_reads_the_peer_address() {
    let router = Router::new().route(PEER_PATH, get(peer));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), router)
        .await
        .unwrap();
    let address = server.address().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run(async {
        stopped.await.ok();
    }));

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}{PEER_PATH}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "127.0.0.1");

    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
