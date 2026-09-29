use super::*;
use std::io::{Read, Write};

#[cfg(feature = "comet-rpc")]
pub(crate) mod endpoint {
    //! An in-process channel endpoint, as the node's adapter answers.
    use dytallix_client_channel::{
        endpoint_offer, handshake_payload_len, EndpointPin, HandshakeType, Identity, MessageReader,
        Request, Response, Session, HANDSHAKE_HEADER_LEN, MAX_REQUEST_MESSAGE,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    async fn read(stream: &mut tokio::net::TcpStream, len: usize) -> Option<Vec<u8>> {
        let mut buffer = vec![0; len];
        stream.read_exact(&mut buffer).await.ok()?;
        Some(buffer)
    }

    /// Serves one exchange with the key from `seed`, answering `status` and
    /// `reply`. Returns the pin a client would receive and the request the
    /// endpoint saw, if the handshake completed.
    pub(crate) async fn serve(
        seed: u8,
        network: &'static str,
        status: u16,
        reply: Vec<u8>,
    ) -> (EndpointPin, JoinHandle<Option<Request>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let identity = Identity::from_seed(&[seed; 32]);
        let pin = EndpointPin::new(
            network,
            &listener.local_addr().unwrap().to_string(),
            identity.public_key(),
        )
        .unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let header: [u8; HANDSHAKE_HEADER_LEN] = read(&mut stream, HANDSHAKE_HEADER_LEN)
                .await?
                .try_into()
                .unwrap();
            let len = handshake_payload_len(&header, HandshakeType::Hello).ok()?;
            let hello = read(&mut stream, len).await?;
            let (pending, offer) = endpoint_offer(&identity, network, &hello).ok()?;
            stream.write_all(&offer).await.ok()?;
            let header: [u8; HANDSHAKE_HEADER_LEN] = read(&mut stream, HANDSHAKE_HEADER_LEN)
                .await?
                .try_into()
                .unwrap();
            let len = handshake_payload_len(&header, HandshakeType::Finish).ok()?;
            let finish = read(&mut stream, len).await?;
            let Session { mut sealer, opener } = pending.finish(&finish).ok()?;
            let mut reader = MessageReader::new(opener, MAX_REQUEST_MESSAGE);
            let request = loop {
                let bytes = read(&mut stream, reader.wanted()).await?;
                if let Some(message) = reader.feed(&bytes).ok()? {
                    break Request::decode(&message).ok()?;
                }
            };
            let response = Response {
                status,
                content_type: "application/json".into(),
                cache_control: String::new(),
                body: reply,
            };
            let sealed = sealer.seal_message(&response.encode().unwrap()).unwrap();
            stream.write_all(&sealed).await.ok()?;
            Some(request)
        });
        (pin, task)
    }
}

#[cfg(feature = "comet-rpc")]
mod channel {
    use super::endpoint::serve;
    use super::*;
    use dytallix_client_channel::{Identity, Method};

    #[tokio::test]
    async fn a_request_crosses_the_channel_to_the_pinned_endpoint() {
        let (pin, task) = serve(1, "chain-a", 200, b"{\"result\":1}".to_vec()).await;
        let body = post(
            &Endpoint::Channel(pin),
            b"{\"method\":\"status\"}".to_vec(),
            1024,
        )
        .await
        .unwrap();
        assert_eq!(body, b"{\"result\":1}");
        let request = task.await.unwrap().unwrap();
        assert_eq!((request.method, request.path.as_str()), (Method::Post, "/"));
        assert_eq!(request.body, b"{\"method\":\"status\"}");
    }

    #[tokio::test]
    async fn another_key_or_network_gets_no_answer() {
        let (pin, task) = serve(1, "chain-a", 200, vec![]).await;
        let other = EndpointPin::new(
            "chain-a",
            &pin.address,
            Identity::from_seed(&[2; 32]).public_key(),
        )
        .unwrap();
        let error = post(&Endpoint::Channel(other), b"{}".to_vec(), 1024)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("closed the channel"), "{error}");
        assert!(task.await.unwrap().is_none());

        let (pin, task) = serve(1, "chain-a", 200, vec![]).await;
        let wrong = EndpointPin::new("chain-b", &pin.address, &pin.public_key).unwrap();
        assert!(post(&Endpoint::Channel(wrong), b"{}".to_vec(), 1024)
            .await
            .is_err());
        assert!(task.await.unwrap().is_none());
    }

    #[tokio::test]
    async fn statuses_and_bounds_are_checked() {
        let (pin, _task) = serve(1, "chain-a", 503, b"{}".to_vec()).await;
        let error = post(&Endpoint::Channel(pin), b"{}".to_vec(), 1024)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("RPC HTTP status 503"), "{error}");

        let (pin, _task) = serve(1, "chain-a", 200, vec![b'x'; 2048]).await;
        let error = post(&Endpoint::Channel(pin), b"{}".to_vec(), 1024)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("exceeds bound"), "{error}");
    }

    #[test]
    fn a_channel_endpoint_describes_its_key() {
        let key = Identity::from_seed(&[1; 32]).public_key().to_vec();
        let pin = EndpointPin::new("chain-a", "node.example:26670", &key).unwrap();
        assert_eq!(
            Endpoint::Channel(pin).describe(),
            format!("node.example:26670 (key {})", fingerprint(&key))
        );
    }
}

/// One HTTP exchange on a thread: reads the request, answers `status`.
fn http_server(status: &'static str, reply: &'static str) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut raw = Vec::new();
        let mut buffer = [0; 1024];
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buffer).unwrap();
            raw.extend_from_slice(&buffer[..n]);
        }
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reply.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(reply.as_bytes());
    });
    address
}

#[tokio::test]
async fn loopback_http_checks_the_status_and_the_bound() {
    let address = http_server("200 OK", "{\"ok\":true}");
    assert_eq!(
        post(&Endpoint::Loopback(address), b"{}".to_vec(), 1024)
            .await
            .unwrap(),
        b"{\"ok\":true}"
    );
    let address = http_server("503 Service Unavailable", "{}");
    let error = post(&Endpoint::Loopback(address), b"{}".to_vec(), 1024)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("RPC HTTP status 503"), "{error}");
    let address = http_server("200 OK", "{\"too\":\"long\"}");
    let error = post(&Endpoint::Loopback(address), b"{}".to_vec(), 4)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("exceeds bound"), "{error}");
}

#[test]
fn only_loopback_urls_are_plain_http_endpoints() {
    assert_eq!(
        Endpoint::loopback("http://127.0.0.1:26657/").unwrap(),
        Endpoint::Loopback("127.0.0.1:26657".parse().unwrap())
    );
    assert_eq!(
        Endpoint::loopback("http://[::1]:1").unwrap().describe(),
        "http://[::1]:1"
    );
    for url in [
        "https://127.0.0.1:1",
        "http://10.0.0.1:1",
        "http://localhost:1",
        "127.0.0.1:1",
    ] {
        assert!(Endpoint::loopback(url).is_err(), "{url}");
    }
}
