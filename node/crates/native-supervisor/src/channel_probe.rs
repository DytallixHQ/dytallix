//! The supervisor's channel readiness probe (E04 gap 19, C-b;
//! `docs/architecture/client-channel-v1.md`).
//!
//! A real client exchange with the pinned key: the hello, the endpoint's
//! signed offer, the finish and a sealed `GET /status`. A seed that does not
//! match the published pin, another network, or a broken listener stops
//! startup here, before clients can notice.

use super::{connect_address, read_exact, write_all, Deadline};
use anyhow::{anyhow, ensure, Result};
use dytallix_client_channel::{
    client_hello, handshake_payload_len, HandshakeType, MessageReader, Method, Request, Response,
    Session, HANDSHAKE_HEADER_LEN, MAX_HEADER_VALUE, MAX_RESPONSE_MESSAGE,
};
use std::net::SocketAddr;

/// Fixed evidence: the channel's reason is not formatted.
fn failed(_: dytallix_client_channel::Error) -> anyhow::Error {
    anyhow!("Channel readiness exchange failed")
}

/// Returns the status body, at most `limit` bytes.
pub(super) fn status_body(
    address: SocketAddr,
    network: &str,
    key: &[u8],
    limit: usize,
    deadline: &mut Deadline<'_>,
) -> Result<Vec<u8>> {
    let mut stream = connect_address(address, deadline)?;
    let (start, hello) = client_hello(network, key).map_err(failed)?;
    write_all(&mut stream, &hello, deadline)?;
    let mut header = [0u8; HANDSHAKE_HEADER_LEN];
    read_exact(&mut stream, &mut header, deadline)?;
    let mut offer = vec![0; handshake_payload_len(&header, HandshakeType::Offer).map_err(failed)?];
    read_exact(&mut stream, &mut offer, deadline)?;
    let (Session { mut sealer, opener }, mut out) = start.finish(&offer).map_err(failed)?;
    let request = Request {
        method: Method::Get,
        path: "/status".into(),
        query: String::new(),
        body: vec![],
    };
    out.extend(
        sealer
            .seal_message(&request.encode().map_err(failed)?)
            .map_err(failed)?,
    );
    write_all(&mut stream, &out, deadline)?;
    // The response's framing around a body of at most `limit` bytes.
    let bound = (1 + 2 + 2 * (2 + MAX_HEADER_VALUE) + 4)
        .saturating_add(limit)
        .min(MAX_RESPONSE_MESSAGE);
    let mut reader = MessageReader::new(opener, bound);
    loop {
        let mut bytes = vec![0; reader.wanted()];
        read_exact(&mut stream, &mut bytes, deadline)?;
        if let Some(message) = reader.feed(&bytes).map_err(failed)? {
            let response = Response::decode(&message).map_err(failed)?;
            ensure!(
                response.status == 200,
                "Channel readiness status is not 200"
            );
            ensure!(
                response.body.len() <= limit,
                "Channel readiness response bound"
            );
            return Ok(response.body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessLimits;
    use dytallix_client_channel::{endpoint_offer, Identity, MAX_REQUEST_MESSAGE};
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    fn limits() -> ProcessLimits {
        ProcessLimits {
            startup_millis: 2000,
            stop_millis: 100,
            kill_millis: 100,
            poll_millis: 2,
            max_argument_bytes: 4096,
            max_environment_bytes: 4096,
            max_probe_request_bytes: 4096,
            max_probe_response_bytes: 65536,
        }
    }

    /// One endpoint exchange on a thread, as the adapter makes it; `status`
    /// and `body` are its reply.
    fn endpoint(seed: u8, network: &'static str, status: u16, body: Vec<u8>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let identity = Identity::from_seed(&[seed; 32]);
            let (mut stream, _) = listener.accept().unwrap();
            let mut read = |len: usize| {
                let mut buffer = vec![0; len];
                stream.read_exact(&mut buffer).map(|_| buffer)
            };
            let Ok(header) = read(HANDSHAKE_HEADER_LEN) else {
                return;
            };
            let len =
                handshake_payload_len(header.as_slice().try_into().unwrap(), HandshakeType::Hello)
                    .unwrap();
            let hello = read(len).unwrap();
            let Ok((pending, offer)) = endpoint_offer(&identity, network, &hello) else {
                return;
            };
            drop(read);
            stream.write_all(&offer).unwrap();
            let mut header = [0u8; HANDSHAKE_HEADER_LEN];
            stream.read_exact(&mut header).unwrap();
            let mut finish =
                vec![0; handshake_payload_len(&header, HandshakeType::Finish).unwrap()];
            stream.read_exact(&mut finish).unwrap();
            let Session { mut sealer, opener } = pending.finish(&finish).unwrap();
            let mut reader = MessageReader::new(opener, MAX_REQUEST_MESSAGE);
            let request = loop {
                let mut bytes = vec![0; reader.wanted()];
                stream.read_exact(&mut bytes).unwrap();
                if let Some(message) = reader.feed(&bytes).unwrap() {
                    break Request::decode(&message).unwrap();
                }
            };
            assert_eq!(
                (request.method, request.path.as_str()),
                (Method::Get, "/status")
            );
            let response = Response {
                status,
                content_type: "application/json".into(),
                cache_control: String::new(),
                body,
            };
            let sealed = sealer.seal_message(&response.encode().unwrap()).unwrap();
            stream.write_all(&sealed).unwrap();
        });
        address
    }

    fn probe(address: SocketAddr, seed: u8, network: &str) -> Result<Vec<u8>> {
        let key = Identity::from_seed(&[seed; 32]).public_key().to_vec();
        let mut health = || Ok(());
        let mut deadline = Deadline::new(&limits(), &mut health)?;
        status_body(
            address,
            network,
            &key,
            limits().max_probe_response_bytes,
            &mut deadline,
        )
    }

    #[test]
    fn the_probe_completes_an_exchange_with_the_pinned_key() {
        let address = endpoint(3, "chain-a", 200, b"{\"ok\":true}".to_vec());
        assert_eq!(probe(address, 3, "chain-a").unwrap(), b"{\"ok\":true}");
    }

    #[test]
    fn another_key_network_status_or_size_fails_the_probe() {
        // The pin names another key: the endpoint refuses the hello.
        assert!(probe(endpoint(3, "chain-a", 200, vec![]), 4, "chain-a").is_err());
        // Another network: the endpoint refuses the hello and closes.
        assert!(probe(endpoint(3, "chain-a", 200, vec![]), 3, "chain-b").is_err());
        assert!(probe(endpoint(3, "chain-a", 503, vec![]), 3, "chain-a").is_err());
        let large = vec![b'x'; limits().max_probe_response_bytes + 1];
        assert!(probe(endpoint(3, "chain-a", 200, large), 3, "chain-a").is_err());
    }
}
