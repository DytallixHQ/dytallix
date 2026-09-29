//! The Dytallix client channel, version 1 (E04 gap 19,
//! `docs/architecture/client-channel-v1.md`).
//!
//! A client that pins an endpoint's ML-DSA-65 key opens an encrypted channel
//! to it with no classical public-key cryptography: an ML-KEM-768 key
//! exchange, an ML-DSA-65 signature by the endpoint, HKDF-SHA-256 and
//! AES-256-GCM records. The client stays anonymous; transactions carry their
//! own signatures.
//!
//! The crate performs no I/O. Callers read the byte counts it names and write
//! the frames it returns, so any runtime can drive it.

mod handshake;
mod message;
mod record;

pub use handshake::{
    client_hello, endpoint_offer, handshake_payload_len, ClientStart, EndpointPending,
    HandshakeType, Identity, Session, HANDSHAKE_HEADER_LEN, MAX_NETWORK_LEN, PUBLIC_KEY_LEN,
    SEED_LEN, SUITE,
};
pub use message::{
    Method, Request, Response, MAX_HEADER_VALUE, MAX_PATH, MAX_QUERY, MAX_REQUEST_BODY,
    MAX_REQUEST_MESSAGE, MAX_RESPONSE_BODY, MAX_RESPONSE_MESSAGE,
};
pub use record::{
    MessageReader, Opener, Sealer, MAX_RECORDS, MAX_RECORD_PLAINTEXT, RECORD_HEADER_LEN, TAG_LEN,
};

/// Why the channel refused. Peers learn nothing beyond a closed connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A frame, record or message is not in the version 1 form.
    Malformed,
    /// Authentication failed: a key, signature, confirmation or tag.
    Rejected,
    /// A size or count bound was reached.
    Limit,
    /// The operating system's random source failed.
    Randomness,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::Malformed => "client channel: malformed input",
            Error::Rejected => "client channel: authentication failed",
            Error::Limit => "client channel: limit reached",
            Error::Randomness => "client channel: random source failed",
        })
    }
}

impl std::error::Error for Error {}

/// Length-prefixed concatenation (four-byte big-endian lengths), as the peer
/// transport encodes its transcript.
fn encode(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| 4 + p.len()).sum());
    for part in parts {
        out.extend_from_slice(&(part.len() as u32).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

#[cfg(test)]
mod tests;
