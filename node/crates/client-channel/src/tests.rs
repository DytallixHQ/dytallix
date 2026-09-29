use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::handshake::{client_hello_with, endpoint_offer_with, ClientSeeds, EndpointSeeds};
use crate::*;

const NETWORK: &str = "dytallix-channel-test";

fn identity(byte: u8) -> Identity {
    Identity::from_seed(&[byte; SEED_LEN])
}

fn payload(frame: &[u8], kind: HandshakeType) -> Vec<u8> {
    let header: [u8; HANDSHAKE_HEADER_LEN] = frame[..HANDSHAKE_HEADER_LEN].try_into().unwrap();
    let len = handshake_payload_len(&header, kind).unwrap();
    assert_eq!(frame.len(), HANDSHAKE_HEADER_LEN + len);
    frame[HANDSHAKE_HEADER_LEN..].to_vec()
}

/// Runs a whole handshake and returns (client session, endpoint session).
fn connect(endpoint: &Identity) -> (Session, Session) {
    let (start, hello) = client_hello(NETWORK, endpoint.public_key()).unwrap();
    let (pending, offer) =
        endpoint_offer(endpoint, NETWORK, &payload(&hello, HandshakeType::Hello)).unwrap();
    let (client, finish) = start
        .finish(&payload(&offer, HandshakeType::Offer))
        .unwrap();
    let server = pending
        .finish(&payload(&finish, HandshakeType::Finish))
        .unwrap();
    (client, server)
}

/// Feeds sealed records to a reader the way a caller reading a socket would.
fn read(reader: &mut MessageReader, mut bytes: &[u8]) -> Result<Option<Vec<u8>>, Error> {
    loop {
        let wanted = reader.wanted();
        if bytes.len() < wanted {
            return Ok(None);
        }
        let (chunk, rest) = bytes.split_at(wanted);
        bytes = rest;
        if let Some(message) = reader.feed(chunk)? {
            assert!(bytes.is_empty(), "bytes after the message");
            return Ok(Some(message));
        }
    }
}

fn request() -> Request {
    Request {
        method: Method::Post,
        path: "/".into(),
        query: String::new(),
        body: br#"{"jsonrpc":"2.0","id":1,"method":"status","params":{}}"#.to_vec(),
    }
}

#[test]
fn a_request_and_response_cross_the_channel() {
    let endpoint = identity(7);
    let (mut client, server) = connect(&endpoint);
    let sealed = client
        .sealer
        .seal_message(&request().encode().unwrap())
        .unwrap();
    let mut reader = MessageReader::new(server.opener, MAX_REQUEST_MESSAGE);
    let received = read(&mut reader, &sealed).unwrap().unwrap();
    assert_eq!(Request::decode(&received).unwrap(), request());

    // A response larger than one record is split and reassembled.
    let response = Response {
        status: 200,
        content_type: "application/json".into(),
        cache_control: String::new(),
        body: vec![b'x'; 3 * MAX_RECORD_PLAINTEXT + 5],
    };
    let mut sealer = server.sealer;
    let sealed = sealer.seal_message(&response.encode().unwrap()).unwrap();
    let mut reader = MessageReader::new(client.opener, MAX_RESPONSE_MESSAGE);
    let received = read(&mut reader, &sealed).unwrap().unwrap();
    assert_eq!(Response::decode(&received).unwrap(), response);
}

#[test]
fn only_the_pinned_endpoint_completes_a_handshake() {
    let pinned = identity(1);
    let other = identity(2);

    // The hello names the pinned key, so another endpoint refuses it.
    let (_, hello) = client_hello(NETWORK, pinned.public_key()).unwrap();
    let hello = payload(&hello, HandshakeType::Hello);
    assert_eq!(
        endpoint_offer(&other, NETWORK, &hello).err(),
        Some(Error::Rejected)
    );

    // An interceptor that rewrites the key to its own cannot sign for the
    // pinned one: the client refuses its offer.
    let (start, _) = client_hello(NETWORK, pinned.public_key()).unwrap();
    let mut rewritten = hello.clone();
    let key_at = 1 + NETWORK.len() + 32;
    rewritten[key_at..key_at + PUBLIC_KEY_LEN].copy_from_slice(other.public_key());
    let (_, offer) = endpoint_offer(&other, NETWORK, &rewritten).unwrap();
    assert_eq!(
        start.finish(&payload(&offer, HandshakeType::Offer)).err(),
        Some(Error::Rejected)
    );

    // Another network is refused.
    let (_, hello) = client_hello("another-chain", pinned.public_key()).unwrap();
    assert_eq!(
        endpoint_offer(&pinned, NETWORK, &payload(&hello, HandshakeType::Hello)).err(),
        Some(Error::Rejected)
    );
}

#[test]
fn a_changed_offer_or_finish_is_refused() {
    let endpoint = identity(3);
    // One byte changed in the ciphertext, the signature or the confirmation.
    for at in [0, 1088 + 100, 1088 + 3309 + 5] {
        let (start, hello) = client_hello(NETWORK, endpoint.public_key()).unwrap();
        let (_, offer) =
            endpoint_offer(&endpoint, NETWORK, &payload(&hello, HandshakeType::Hello)).unwrap();
        let mut offer = payload(&offer, HandshakeType::Offer);
        offer[at] ^= 1;
        assert_eq!(
            start.finish(&offer).err(),
            Some(Error::Rejected),
            "byte {at}"
        );
    }
    let (start, hello) = client_hello(NETWORK, endpoint.public_key()).unwrap();
    let (pending, offer) =
        endpoint_offer(&endpoint, NETWORK, &payload(&hello, HandshakeType::Hello)).unwrap();
    let (_, finish) = start
        .finish(&payload(&offer, HandshakeType::Offer))
        .unwrap();
    let mut finish = payload(&finish, HandshakeType::Finish);
    finish[0] ^= 1;
    assert_eq!(pending.finish(&finish).err(), Some(Error::Rejected));
}

#[test]
fn an_invalid_encapsulation_key_is_refused() {
    let endpoint = identity(4);
    let (_, hello) = client_hello(NETWORK, endpoint.public_key()).unwrap();
    let mut hello = payload(&hello, HandshakeType::Hello);
    // The first 12-bit coefficient becomes 4095, above q = 3329.
    let at = 1 + NETWORK.len() + 32 + PUBLIC_KEY_LEN;
    hello[at] = 0xff;
    hello[at + 1] |= 0x0f;
    assert_eq!(
        endpoint_offer(&endpoint, NETWORK, &hello).err(),
        Some(Error::Malformed)
    );
}

#[test]
fn handshake_headers_are_checked_before_reading() {
    let endpoint = identity(5);
    let (_, hello) = client_hello(NETWORK, endpoint.public_key()).unwrap();
    let header: [u8; HANDSHAKE_HEADER_LEN] = hello[..8].try_into().unwrap();
    assert!(handshake_payload_len(&header, HandshakeType::Hello).is_ok());
    assert!(handshake_payload_len(&header, HandshakeType::Offer).is_err());
    for (at, value) in [(0, b'X'), (4, 2), (5, 9), (6, 0xff)] {
        let mut changed = header;
        changed[at] = value;
        assert_eq!(
            handshake_payload_len(&changed, HandshakeType::Hello),
            Err(Error::Malformed),
            "byte {at}"
        );
    }
    // Hello payloads outside the bounds are refused whatever they hold.
    assert_eq!(
        endpoint_offer(&endpoint, NETWORK, &[]).err(),
        Some(Error::Malformed)
    );
    assert_eq!(
        endpoint_offer(&endpoint, NETWORK, &hello[8..hello.len() - 1]).err(),
        Some(Error::Malformed)
    );
    assert!(client_hello("", endpoint.public_key()).is_err());
    assert!(client_hello(&"n".repeat(65), endpoint.public_key()).is_err());
    assert!(client_hello(NETWORK, &[0u8; 10]).is_err());
}

#[test]
fn records_are_authenticated_in_order_and_to_the_end() {
    let endpoint = identity(6);
    let message = vec![7u8; 2 * MAX_RECORD_PLAINTEXT + 1];
    let record = RECORD_HEADER_LEN + MAX_RECORD_PLAINTEXT + TAG_LEN;

    // A changed byte in a header or a ciphertext.
    for at in [5, 10, RECORD_HEADER_LEN + 3, record + RECORD_HEADER_LEN] {
        let (mut client, server) = connect(&endpoint);
        let mut sealed = client.sealer.seal_message(&message).unwrap();
        sealed[at] ^= 1;
        let mut reader = MessageReader::new(server.opener, MAX_REQUEST_MESSAGE);
        assert!(read(&mut reader, &sealed).is_err(), "byte {at}");
        // A failed reader stays failed.
        assert!(reader.feed(&[0u8; RECORD_HEADER_LEN]).is_err());
    }

    // Records out of order.
    let (mut client, server) = connect(&endpoint);
    let sealed = client.sealer.seal_message(&message).unwrap();
    let mut swapped = sealed[record..2 * record].to_vec();
    swapped.extend_from_slice(&sealed[..record]);
    let mut reader = MessageReader::new(server.opener, MAX_REQUEST_MESSAGE);
    assert_eq!(read(&mut reader, &swapped), Err(Error::Rejected));

    // A message cut before its last record never completes.
    let (mut client, server) = connect(&endpoint);
    let sealed = client.sealer.seal_message(&message).unwrap();
    let mut reader = MessageReader::new(server.opener, MAX_REQUEST_MESSAGE);
    assert_eq!(read(&mut reader, &sealed[..2 * record]), Ok(None));

    // A record reflected back to its sender fails: each direction has its
    // own key.
    let (mut client, _) = connect(&endpoint);
    let sealed = client.sealer.seal_message(b"hello").unwrap();
    let mut reader = MessageReader::new(client.opener, MAX_REQUEST_MESSAGE);
    assert_eq!(read(&mut reader, &sealed), Err(Error::Rejected));

    // A message over the reader's bound is refused from its headers.
    let (mut client, server) = connect(&endpoint);
    let sealed = client.sealer.seal_message(&message).unwrap();
    let mut reader = MessageReader::new(server.opener, MAX_RECORD_PLAINTEXT);
    assert_eq!(read(&mut reader, &sealed), Err(Error::Limit));

    // Empty messages are not sent.
    let (mut client, _) = connect(&endpoint);
    assert_eq!(client.sealer.seal_message(&[]), Err(Error::Malformed));
}

#[test]
fn messages_have_one_strict_form() {
    let get = Request {
        method: Method::Get,
        path: "/abci_query".into(),
        query: "path=%22%2Fstatus%22".into(),
        body: vec![],
    };
    assert_eq!(Request::decode(&get.encode().unwrap()).unwrap(), get);

    let refused = [
        Request {
            body: vec![1],
            ..get.clone()
        },
        Request {
            path: "abci_query".into(),
            ..get.clone()
        },
        Request {
            path: "/a b".into(),
            ..get.clone()
        },
        Request {
            path: format!("/{}", "a".repeat(MAX_PATH)),
            ..get.clone()
        },
        Request {
            query: "a".repeat(MAX_QUERY + 1),
            ..get.clone()
        },
        Request {
            path: "/status".into(),
            ..request()
        },
        Request {
            query: "x=1".into(),
            ..request()
        },
        Request {
            body: vec![],
            ..request()
        },
        Request {
            body: vec![1; MAX_REQUEST_BODY + 1],
            ..request()
        },
    ];
    for request in refused {
        assert_eq!(request.encode(), Err(Error::Malformed), "{request:?}");
    }

    let mut bytes = get.encode().unwrap();
    bytes.push(0);
    assert_eq!(Request::decode(&bytes), Err(Error::Malformed));
    bytes.truncate(bytes.len() - 2);
    assert_eq!(Request::decode(&bytes), Err(Error::Malformed));
    let mut bytes = get.encode().unwrap();
    bytes[1] = 3;
    assert_eq!(Request::decode(&bytes), Err(Error::Malformed));

    let response = Response {
        status: 200,
        content_type: "application/json".into(),
        cache_control: "no-store".into(),
        body: b"{}".to_vec(),
    };
    assert_eq!(
        Response::decode(&response.encode().unwrap()).unwrap(),
        response
    );
    for bad in [
        Response {
            status: 99,
            ..response.clone()
        },
        Response {
            content_type: "a\nb".into(),
            ..response.clone()
        },
        Response {
            cache_control: "a".repeat(MAX_HEADER_VALUE + 1),
            ..response.clone()
        },
        Response {
            body: vec![0; MAX_RESPONSE_BODY + 1],
            ..response.clone()
        },
    ] {
        assert_eq!(bad.encode(), Err(Error::Malformed));
    }
    // A request is not a response.
    assert_eq!(
        Response::decode(&get.encode().unwrap()),
        Err(Error::Malformed)
    );
}

/// Fixed seeds give fixed bytes: the FIPS 203 `d`, `z` and `m` inputs, the
/// FIPS 204 `rnd` input and the endpoint's key seed. A second
/// implementation reproduces this digest from the same seeds.
#[test]
fn fixed_seeds_give_the_recorded_transcript() {
    let seed = |byte: u8| Zeroizing::new([byte; 32]);
    let endpoint = identity(0x11);
    let client_seeds = ClientSeeds {
        nonce: seed(0x21),
        d: seed(0x22),
        z: seed(0x23),
    };
    let endpoint_seeds = EndpointSeeds {
        m: seed(0x31),
        // Zero signing randomness is FIPS 204's deterministic variant, which
        // other libraries expose.
        rnd: seed(0x00),
    };
    let (start, hello) = client_hello_with(NETWORK, endpoint.public_key(), &client_seeds).unwrap();
    let (pending, offer) = endpoint_offer_with(
        &endpoint,
        NETWORK,
        &payload(&hello, HandshakeType::Hello),
        &endpoint_seeds,
    )
    .unwrap();
    let (mut client, finish) = start
        .finish(&payload(&offer, HandshakeType::Offer))
        .unwrap();
    let mut server = pending
        .finish(&payload(&finish, HandshakeType::Finish))
        .unwrap();
    let request = client
        .sealer
        .seal_message(&request().encode().unwrap())
        .unwrap();
    let response = Response {
        status: 200,
        content_type: "application/json".into(),
        cache_control: String::new(),
        body: b"{}".to_vec(),
    };
    let response = server
        .sealer
        .seal_message(&response.encode().unwrap())
        .unwrap();

    let mut digest = Sha256::new();
    for part in [&hello, &offer, &finish, &request, &response] {
        digest.update((part.len() as u32).to_be_bytes());
        digest.update(part);
    }
    let digest: [u8; 32] = digest.finalize().into();
    assert_eq!(
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "74a0f0cae5e6ae2effe560bb09a8191384019f4e99d27d2b9c42c37616c759f8"
    );
}
