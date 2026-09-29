//! How the SDK reaches a node (E04 gap 19; the node's
//! `docs/architecture/client-channel-v1.md`). There are two ways, and no TLS:
//! - plain HTTP, only to a literal loopback address, for a node on this
//!   machine;
//! - the post-quantum client channel, to an endpoint whose pin the caller
//!   supplies (full ML-DSA-65 key, ML-KEM-768 key exchange).
//!
//! Neither falls back to the other.

use crate::ordinary_v2::{error, Error, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;

#[cfg(feature = "comet-rpc")]
pub use dytallix_client_channel::{fingerprint, EndpointPin};

/// Each request, handshake included, ends within this time.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a [`CometClient`](crate::ordinary_client::CometClient) sends its
/// requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// `http://IP:PORT` with a literal loopback IP.
    Loopback(SocketAddr),
    /// The client channel to a pinned endpoint.
    #[cfg(feature = "comet-rpc")]
    Channel(EndpointPin),
}

impl Endpoint {
    /// Parses `http://IP:PORT` or `http://IP:PORT/` with a literal loopback
    /// IP. HTTPS, remote hosts, DNS names, credentials, paths, queries and
    /// fragments are refused.
    pub fn loopback(url: &str) -> Result<Self> {
        if url.starts_with("https://") {
            return Err(Error(
                "TLS is not supported (E04 gap 19): reach a remote node through its client channel pin".into(),
            ));
        }
        let authority = url
            .strip_prefix("http://")
            .map(|rest| rest.strip_suffix('/').unwrap_or(rest))
            .filter(|authority| !authority.contains(['/', '?', '#', '@']))
            .ok_or_else(|| {
                Error("expected http://IP:PORT; URL credentials, paths, queries and fragments are forbidden".into())
            })?;
        let address: SocketAddr = authority
            .parse()
            .map_err(|_| Error("plain HTTP needs a literal loopback IP and a port".into()))?;
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(Error(
                "plain HTTP reaches only a node on this machine (a literal loopback address); use a remote endpoint's pin".into(),
            ));
        }
        Ok(Endpoint::Loopback(address))
    }

    /// For people: the loopback URL, or the pin's address and key fingerprint.
    pub fn describe(&self) -> String {
        match self {
            Endpoint::Loopback(address) => format!("http://{address}"),
            #[cfg(feature = "comet-rpc")]
            Endpoint::Channel(pin) => format!("{} (key {})", pin.address, pin.fingerprint()),
        }
    }
}

/// POSTs one JSON-RPC body and returns the response body, at most
/// `max_response_bytes`. A non-2xx status is an error.
pub(crate) async fn post(
    endpoint: &Endpoint,
    body: Vec<u8>,
    max_response_bytes: usize,
) -> Result<Vec<u8>> {
    let exchange = async {
        match endpoint {
            Endpoint::Loopback(address) => http_post(*address, body, max_response_bytes).await,
            #[cfg(feature = "comet-rpc")]
            Endpoint::Channel(pin) => channel_post(pin, body, max_response_bytes).await,
        }
    };
    tokio::time::timeout(REQUEST_TIMEOUT, exchange)
        .await
        .map_err(|_| Error("RPC request timed out".into()))?
}

fn bound_exceeded() -> Error {
    Error("RPC response exceeds bound".into())
}

async fn http_post(address: SocketAddr, body: Vec<u8>, max: usize) -> Result<Vec<u8>> {
    use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
    use hyper::body::Bytes;
    let stream = TcpStream::connect(address).await.map_err(error)?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
            .await
            .map_err(error)?;
    let driver = tokio::spawn(connection);
    let request = hyper::Request::post("/")
        .header(hyper::header::HOST, address.to_string())
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .map_err(error)?;
    let result = async {
        let response = sender.send_request(request).await.map_err(error)?;
        if !response.status().is_success() {
            return Err(Error(format!("RPC HTTP status {}", response.status())));
        }
        let collected = Limited::new(response.into_body(), max)
            .collect()
            .await
            .map_err(|e| {
                if e.is::<LengthLimitError>() {
                    bound_exceeded()
                } else {
                    error(e)
                }
            })?;
        Ok(collected.to_bytes().to_vec())
    }
    .await;
    driver.abort();
    result
}

#[cfg(feature = "comet-rpc")]
async fn channel_post(pin: &EndpointPin, body: Vec<u8>, max: usize) -> Result<Vec<u8>> {
    use dytallix_client_channel::{
        client_hello, handshake_payload_len, Error as ChannelError, HandshakeType, MessageReader,
        Method, Request, Response, Session, HANDSHAKE_HEADER_LEN, MAX_HEADER_VALUE,
        MAX_RESPONSE_MESSAGE,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let failed = |_: ChannelError| Error("client channel exchange failed".into());

    let mut stream = TcpStream::connect(pin.address.as_str())
        .await
        .map_err(error)?;
    let (start, hello) = client_hello(&pin.network, &pin.public_key)
        .map_err(|_| Error("invalid endpoint pin".into()))?;
    stream.write_all(&hello).await.map_err(error)?;
    let mut header = [0u8; HANDSHAKE_HEADER_LEN];
    // An endpoint refuses a hello for another key or network by closing.
    stream.read_exact(&mut header).await.map_err(|_| {
        Error("the endpoint closed the channel; its key or network may differ from the pin".into())
    })?;
    let mut offer = vec![0; handshake_payload_len(&header, HandshakeType::Offer).map_err(failed)?];
    stream.read_exact(&mut offer).await.map_err(error)?;
    let (Session { mut sealer, opener }, mut out) = start
        .finish(&offer)
        .map_err(|_| Error("the endpoint did not prove the pinned key".into()))?;
    let request = Request {
        method: Method::Post,
        path: "/".into(),
        query: String::new(),
        body,
    }
    .encode()
    .map_err(|_| Error("RPC request exceeds the channel's bound".into()))?;
    out.extend(sealer.seal_message(&request).map_err(failed)?);
    stream.write_all(&out).await.map_err(error)?;

    let bound = (1 + 2 + 2 * (2 + MAX_HEADER_VALUE) + 4)
        .saturating_add(max)
        .min(MAX_RESPONSE_MESSAGE);
    let mut reader = MessageReader::new(opener, bound);
    let message = loop {
        let mut bytes = vec![0; reader.wanted()];
        stream.read_exact(&mut bytes).await.map_err(error)?;
        match reader.feed(&bytes) {
            Ok(Some(message)) => break message,
            Ok(None) => {}
            Err(ChannelError::Limit) => return Err(bound_exceeded()),
            Err(e) => return Err(failed(e)),
        }
    };
    let response = Response::decode(&message).map_err(failed)?;
    if !(200..300).contains(&response.status) {
        return Err(Error(format!("RPC HTTP status {}", response.status)));
    }
    if response.body.len() > max {
        return Err(bound_exceeded());
    }
    Ok(response.body)
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
