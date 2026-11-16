use crate::sso::unread::Unread;

pub(crate) const LIMIT: usize = 1024 * 1024;

pub(crate) async fn read(mut response: reqwest::Response) -> Result<Vec<u8>, Unread> {
    if response
        .content_length()
        .is_some_and(|length| length > LIMIT as u64)
    {
        return Err(Unread::Oversized);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| Unread::Transport(error.without_url()))?
    {
        if body.len() + chunk.len() > LIMIT {
            return Err(Unread::Oversized);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const CHUNK: usize = 64 * 1024;

    async fn sized(length: usize) -> (MockServer, reqwest::Response) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; length]))
            .mount(&server)
            .await;
        let response = reqwest::get(server.uri()).await.unwrap();
        (server, response)
    }

    async fn chunked(length: usize) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            let mut remaining = length;
            while remaining > 0 {
                let size = remaining.min(CHUNK);
                let mut frame = format!("{size:x}\r\n").into_bytes();
                frame.extend(std::iter::repeat_n(b'a', size));
                frame.extend_from_slice(b"\r\n");
                if stream.write_all(&frame).await.is_err() {
                    return;
                }
                remaining -= size;
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
        });
        let response = reqwest::get(format!("http://{address}")).await.unwrap();
        assert_eq!(response.content_length(), None);
        response
    }

    #[tokio::test]
    async fn a_body_up_to_the_limit_is_read_whole() {
        let (_server, response) = sized(LIMIT).await;
        assert_eq!(read(response).await.unwrap().len(), LIMIT);
        assert_eq!(read(chunked(LIMIT).await).await.unwrap().len(), LIMIT);
    }

    #[tokio::test]
    async fn a_declared_length_over_the_limit_is_refused_before_reading() {
        let (_server, response) = sized(LIMIT + 1).await;
        assert!(matches!(read(response).await, Err(Unread::Oversized)));
    }

    #[tokio::test]
    async fn a_chunked_body_is_refused_once_it_passes_the_limit() {
        assert!(matches!(
            read(chunked(LIMIT + CHUNK).await).await,
            Err(Unread::Oversized)
        ));
    }

    #[test]
    fn an_oversized_body_names_the_limit() {
        assert_eq!(
            Unread::Oversized.to_string(),
            "the response is larger than 1048576 bytes"
        );
    }
}
