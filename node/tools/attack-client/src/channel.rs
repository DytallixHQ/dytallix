//! The endpoint and RPC family's channel attacks (launch/approvals/
//! P01_T04_ATTACK_DECISIONS_2026-11-04.json): a client that completes the
//! handshake with the pinned endpoint and then sends what the release's own
//! client never would, and an endpoint that signs with another key so a
//! client's pin check can be attacked.
//!
//! `dytallix-attack channel-request --pin PIN --variant V [--wait S]`:
//!
//! - `oversized_body`: a `POST /` whose body is one byte over the channel's
//!   bound (`MAX_REQUEST_BODY + 1`), encoded by hand since the codec refuses
//!   it;
//! - `method_outside_allowlist`: a request whose method byte is neither GET
//!   (1) nor POST (2).
//!
//! It prints what it saw as one JSON line: whether the handshake completed,
//! whether the endpoint stopped reading, and the sealed response's status
//! (null when the connection closed with no reply).
//!
//! `dytallix-attack wrong-key-endpoint --listen HOST:PORT --network NET
//! [--wait S]`: an endpoint with a fresh key. It prints `{"ready":true}`,
//! answers one client's hello with its own signed offer and reports whether
//! the client sent anything after it (a finish means the client accepted a
//! key it had not pinned).
use anyhow::{anyhow, bail, Context, Result};
use dytallix_client_channel::{
    client_hello, endpoint_offer, handshake_payload_len, EndpointPin, HandshakeType, Identity,
    MessageReader, Response, Session, HANDSHAKE_HEADER_LEN, MAX_PIN_BYTES, MAX_REQUEST_BODY,
    MAX_RESPONSE_MESSAGE, PUBLIC_KEY_LEN,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(test)]
const REQUEST_VARIANTS: [&str; 2] = ["oversized_body", "method_outside_allowlist"];
const REQUEST: u8 = 1;
const POST: u8 = 2;
/// Neither GET (1) nor POST (2).
const OUTSIDE_METHOD: u8 = 3;

/// The request message, by hand: `[1][method][u16 path][u16 query][u32 body]`.
pub fn message(variant: &str) -> Result<Vec<u8>> {
    let (method, path, body): (u8, &[u8], Vec<u8>) = match variant {
        "oversized_body" => (POST, b"/", vec![b' '; MAX_REQUEST_BODY + 1]),
        "method_outside_allowlist" => (OUTSIDE_METHOD, b"/status", Vec::new()),
        other => bail!("unknown channel-request variant {other:?}"),
    };
    let mut out = vec![REQUEST, method];
    out.extend_from_slice(&(path.len() as u16).to_be_bytes());
    out.extend_from_slice(path);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn wait(args: &[String]) -> Result<Duration> {
    let seconds: f64 = flag(args, "--wait")
        .unwrap_or_else(|| "15".into())
        .parse()
        .context("--wait takes seconds")?;
    if !(seconds > 0.0 && seconds <= 600.0) {
        bail!("--wait is 0 to 600 seconds");
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

pub fn channel_request(args: &[String]) -> Result<Value> {
    let pin = PathBuf::from(flag(args, "--pin").ok_or_else(|| anyhow!("--pin is required"))?);
    let variant = flag(args, "--variant").ok_or_else(|| anyhow!("--variant is required"))?;
    let message = message(&variant)?;
    let wait = wait(args)?;
    let raw = std::fs::read(&pin).with_context(|| format!("reading {}", pin.display()))?;
    if raw.len() > MAX_PIN_BYTES {
        bail!("the pin is over {MAX_PIN_BYTES} bytes");
    }
    let pin = EndpointPin::parse(&raw).map_err(|e| anyhow!("the pin: {e}"))?;
    let address = pin
        .address
        .to_socket_addrs()
        .context("resolving the pinned address")?
        .next()
        .ok_or_else(|| anyhow!("the pinned address resolves to nothing"))?;
    let failed = |stage: &str, error: String| json!({"variant": variant, "handshake": false, "stage": stage, "error": error});
    let mut stream = match TcpStream::connect_timeout(&address, wait) {
        Ok(stream) => stream,
        Err(e) => return Ok(failed("connect", e.kind().to_string())),
    };
    stream.set_read_timeout(Some(wait))?;
    stream.set_write_timeout(Some(wait))?;
    let (start, hello) = client_hello(&pin.network, &pin.public_key).map_err(|e| anyhow!("{e}"))?;
    if let Err(e) = stream.write_all(&hello) {
        return Ok(failed("hello", e.kind().to_string()));
    }
    let mut header = [0u8; HANDSHAKE_HEADER_LEN];
    if let Err(e) = stream.read_exact(&mut header) {
        return Ok(failed("offer", e.kind().to_string()));
    }
    let len = match handshake_payload_len(&header, HandshakeType::Offer) {
        Ok(len) => len,
        Err(e) => return Ok(failed("offer", e.to_string())),
    };
    let mut offer = vec![0; len];
    if let Err(e) = stream.read_exact(&mut offer) {
        return Ok(failed("offer", e.kind().to_string()));
    }
    let (Session { mut sealer, opener }, finish) = match start.finish(&offer) {
        Ok(done) => done,
        Err(e) => return Ok(failed("finish", e.to_string())),
    };
    let sealed = sealer
        .seal_message(&message)
        .map_err(|e| anyhow!("sealing: {e}"))?;
    let sent_at = Instant::now();
    let mut out = finish;
    out.extend_from_slice(&sealed);
    // The endpoint may stop reading once the message is over its bound.
    let write_error = stream.write_all(&out).err().map(|e| e.kind().to_string());
    let mut reader = MessageReader::new(opener, MAX_RESPONSE_MESSAGE);
    let mut read = 0usize;
    let (status, closed) = loop {
        let mut bytes = vec![0; reader.wanted()];
        match stream.read(&mut bytes[..1]) {
            Ok(0) => break (None, "eof".to_string()),
            Ok(_) => {}
            Err(e) => break (None, e.kind().to_string()),
        }
        if let Err(e) = stream.read_exact(&mut bytes[1..]) {
            read += 1;
            break (None, e.kind().to_string());
        }
        read += bytes.len();
        match reader.feed(&bytes) {
            Ok(Some(message)) => match Response::decode(&message) {
                Ok(response) => break (Some(response.status), "response".to_string()),
                Err(e) => break (None, format!("undecodable response: {e}")),
            },
            Ok(None) => {}
            Err(e) => break (None, format!("unopenable response: {e}")),
        }
    };
    Ok(json!({
        "variant": variant,
        "handshake": true,
        "message_bytes": message.len(),
        "write_error": write_error,
        "response_status": status,
        "response_bytes": read,
        "ended": closed,
        "ended_after_ms": elapsed_ms(sent_at),
    }))
}

pub fn wrong_key_endpoint(args: &[String]) -> Result<Value> {
    let listen = flag(args, "--listen").ok_or_else(|| anyhow!("--listen is required"))?;
    let network = flag(args, "--network").ok_or_else(|| anyhow!("--network is required"))?;
    let wait = wait(args)?;
    let seed = Identity::generate_seed().map_err(|e| anyhow!("{e}"))?;
    let identity = Identity::from_seed(&seed);
    let listener = TcpListener::bind(&listen).with_context(|| format!("binding {listen}"))?;
    listener.set_nonblocking(true)?;
    println!("{}", json!({"ready": true}));
    std::io::stdout().flush()?;
    let deadline = Instant::now() + wait;
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Ok(json!({"hello": false, "error": "no client connected"}));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Ok(json!({"hello": false, "error": e.kind().to_string()})),
        }
    };
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(wait))?;
    stream.set_write_timeout(Some(wait))?;
    let mut header = [0u8; HANDSHAKE_HEADER_LEN];
    let hello = stream.read_exact(&mut header).ok().and_then(|_| {
        let len = handshake_payload_len(&header, HandshakeType::Hello).ok()?;
        let mut payload = vec![0; len];
        stream.read_exact(&mut payload).ok()?;
        Some(payload)
    });
    let Some(hello) = hello else {
        return Ok(json!({"hello": false, "error": "no valid hello"}));
    };
    // The client named the pinned key, not this endpoint's: answer as the
    // pinned endpoint would, signed with this other key.
    let offer = endpoint_offer_as_any(&identity, &network, &hello);
    let Some((pending, offer)) = offer else {
        return Ok(
            json!({"hello": true, "offer_sent": false, "error": "the hello did not name this network"}),
        );
    };
    if stream.write_all(&offer).is_err() {
        return Ok(
            json!({"hello": true, "offer_sent": false, "error": "the offer could not be sent"}),
        );
    }
    let mut after = Vec::new();
    let mut buffer = [0u8; 4096];
    let started = Instant::now();
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => after.extend_from_slice(&buffer[..n]),
            Err(_) => break,
        }
        if started.elapsed() > wait {
            break;
        }
    }
    let finished = after.len() >= HANDSHAKE_HEADER_LEN
        && <[u8; HANDSHAKE_HEADER_LEN]>::try_from(&after[..HANDSHAKE_HEADER_LEN])
            .ok()
            .and_then(|h| handshake_payload_len(&h, HandshakeType::Finish).ok())
            .is_some_and(|len| {
                after.len() >= HANDSHAKE_HEADER_LEN + len
                    && pending
                        .finish(&after[HANDSHAKE_HEADER_LEN..HANDSHAKE_HEADER_LEN + len])
                        .is_ok()
            });
    Ok(json!({
        "hello": true,
        "offer_sent": true,
        "after_offer_bytes": after.len(),
        "finished": finished,
        "ended_after_ms": elapsed_ms(started),
    }))
}

/// The client's hello names the key it pinned; an endpoint answers only a
/// hello naming its own key. The attack endpoint puts its own key in the
/// hello's place and signs the offer with it, which is what a client's pin
/// check must catch.
fn endpoint_offer_as_any(
    identity: &Identity,
    network: &str,
    hello: &[u8],
) -> Option<(dytallix_client_channel::EndpointPending, Vec<u8>)> {
    let key = identity.public_key();
    let at = key_offset(hello)?;
    let mut ours = hello.to_vec();
    ours.get_mut(at..at + key.len())?.copy_from_slice(key);
    endpoint_offer(identity, network, &ours).ok()
}

/// Where the hello payload carries the endpoint key it names:
/// `[network length][network][32-byte nonce][key][encapsulation key]`.
pub fn key_offset(hello: &[u8]) -> Option<usize> {
    let length = *hello.first()? as usize;
    let at = 1 + length + 32;
    (hello.len() >= at + PUBLIC_KEY_LEN).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dytallix_client_channel::{Method, Request, Response, MAX_REQUEST_MESSAGE};

    fn identity(seed: u8) -> Identity {
        Identity::from_seed(&[seed; 32])
    }

    fn payload(frame: &[u8]) -> &[u8] {
        &frame[HANDSHAKE_HEADER_LEN..]
    }

    #[test]
    fn the_codec_refuses_both_requests_and_the_body_is_one_over_the_bound() {
        for variant in REQUEST_VARIANTS {
            assert!(
                Request::decode(&message(variant).unwrap()).is_err(),
                "{variant}"
            );
        }
        let oversized = message("oversized_body").unwrap();
        assert_eq!(oversized.len(), 2 + 2 + 1 + 2 + 4 + MAX_REQUEST_BODY + 1);
        assert!(
            oversized.len() <= MAX_REQUEST_MESSAGE,
            "within the record reader's absolute bound"
        );
        assert_eq!(
            message("method_outside_allowlist").unwrap()[1],
            OUTSIDE_METHOD
        );
        // The same request with an allowed method decodes, so only the method is wrong.
        let mut get = message("method_outside_allowlist").unwrap();
        get[1] = Method::Get as u8;
        assert_eq!(Request::decode(&get).unwrap().path, "/status");
        assert!(message("wrong_chain").is_err());
    }

    #[test]
    fn a_client_refuses_an_offer_signed_by_a_key_it_did_not_pin() {
        let pinned = identity(1);
        let (start, hello) = client_hello("dytallix-staging-1", pinned.public_key()).unwrap();
        let (_, offer) =
            endpoint_offer_as_any(&identity(2), "dytallix-staging-1", payload(&hello)).unwrap();
        assert!(start.finish(payload(&offer)).is_err());
        // The pinned key's own offer completes, so the refusal is the pin check.
        let (start, hello) = client_hello("dytallix-staging-1", pinned.public_key()).unwrap();
        let (pending, offer) =
            endpoint_offer_as_any(&pinned, "dytallix-staging-1", payload(&hello)).unwrap();
        let (_, finish) = start.finish(payload(&offer)).unwrap();
        assert!(pending.finish(payload(&finish)).is_ok());
    }

    #[test]
    fn the_key_offset_follows_the_network_length() {
        let (_, hello) = client_hello("dytallix-staging-1", identity(1).public_key()).unwrap();
        let at = key_offset(payload(&hello)).unwrap();
        assert_eq!(
            &payload(&hello)[at..at + PUBLIC_KEY_LEN],
            identity(1).public_key()
        );
        assert_eq!(key_offset(&[5, 1, 2]), None);
    }

    /// The adapter's exchange in miniature: a malformed request gets a
    /// sealed 400; one over the bound closes the connection with no reply.
    #[test]
    fn the_request_attacks_against_a_local_endpoint() {
        for (variant, expected) in [
            ("method_outside_allowlist", Some(400)),
            ("oversized_body", None),
        ] {
            let endpoint = identity(3);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let pin = std::env::temp_dir()
                .join(format!("attack-pin-{variant}-{}.json", std::process::id()));
            std::fs::write(
                &pin,
                EndpointPin::new("dytallix-staging-1", &address, endpoint.public_key())
                    .unwrap()
                    .to_json(),
            )
            .unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut header = [0u8; HANDSHAKE_HEADER_LEN];
                stream.read_exact(&mut header).unwrap();
                let mut hello =
                    vec![0; handshake_payload_len(&header, HandshakeType::Hello).unwrap()];
                stream.read_exact(&mut hello).unwrap();
                let (pending, offer) =
                    endpoint_offer(&endpoint, "dytallix-staging-1", &hello).unwrap();
                stream.write_all(&offer).unwrap();
                stream.read_exact(&mut header).unwrap();
                let mut finish =
                    vec![0; handshake_payload_len(&header, HandshakeType::Finish).unwrap()];
                stream.read_exact(&mut finish).unwrap();
                let Session { mut sealer, opener } = pending.finish(&finish).unwrap();
                let mut reader = MessageReader::new(opener, 1024);
                let message = loop {
                    let mut bytes = vec![0; reader.wanted()];
                    stream.read_exact(&mut bytes).unwrap();
                    match reader.feed(&bytes) {
                        Ok(Some(message)) => break message,
                        Ok(None) => {}
                        Err(_) => return,
                    }
                };
                assert!(Request::decode(&message).is_err());
                let response = Response {
                    status: 400,
                    content_type: "application/json".into(),
                    cache_control: String::new(),
                    body: b"{}".to_vec(),
                };
                let sealed = sealer.seal_message(&response.encode().unwrap()).unwrap();
                stream.write_all(&sealed).unwrap();
            });
            let args: Vec<String> = [
                "--pin",
                pin.to_str().unwrap(),
                "--variant",
                variant,
                "--wait",
                "10",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let seen = channel_request(&args).unwrap();
            server.join().unwrap();
            let _ = std::fs::remove_file(&pin);
            assert_eq!(seen["handshake"], true, "{seen}");
            assert_eq!(
                seen["response_status"].as_u64().map(|s| s as u16),
                expected,
                "{seen}"
            );
        }
    }
}
