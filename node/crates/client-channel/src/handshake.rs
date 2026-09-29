//! The handshake: hello, offer and finish.
//!
//! The client sends a fresh ML-KEM-768 encapsulation key and names the
//! endpoint key it pinned. The endpoint encapsulates to it and signs the
//! transcript with that key. Both sides derive the traffic keys and prove
//! them with key confirmations before any record is accepted.

use fips203::ml_kem_768;
use fips203::traits::{Decaps, Encaps, KeyGen as _, SerDes as _};
use fips204::ml_dsa_65;
use fips204::traits::{KeyGen as _, SerDes as _, Signer as _, Verifier as _};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore as _};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::record::{Opener, Sealer};
use crate::{encode, Error};

/// The fixed suite. There is no negotiation and no fallback.
pub const SUITE: &str = "dytallix-client-channel-v1/mlkem768/mldsa65/hkdfsha256/aes256gcm";
/// A full ML-DSA-65 public key: endpoints are pinned by the whole key.
pub const PUBLIC_KEY_LEN: usize = ml_dsa_65::PK_LEN;
/// An endpoint identity is derived from a 32-byte seed (FIPS 204 key
/// generation).
pub const SEED_LEN: usize = 32;
/// The network string is the chain ID.
pub const MAX_NETWORK_LEN: usize = 64;
pub const HANDSHAKE_HEADER_LEN: usize = 8;

const MAGIC: &[u8; 4] = b"DYCH";
const VERSION: u8 = 1;
const NONCE_LEN: usize = 32;
const CONFIRMATION_LEN: usize = 32;
const SIGNATURE_LEN: usize = ml_dsa_65::SIG_LEN;
const HELLO_FIXED: usize = 1 + NONCE_LEN + PUBLIC_KEY_LEN + ml_kem_768::EK_LEN;
const OFFER_LEN: usize = ml_kem_768::CT_LEN + SIGNATURE_LEN + CONFIRMATION_LEN;

/// The handshake message types, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HandshakeType {
    /// Client to endpoint: network, nonce, pinned key, encapsulation key.
    Hello = 1,
    /// Endpoint to client: ciphertext, signature, endpoint confirmation.
    Offer = 2,
    /// Client to endpoint: client confirmation.
    Finish = 3,
}

/// Checks an eight-byte handshake header (`DYCH`, version, type, two-byte
/// big-endian length) for the expected message and returns the payload
/// length to read. The length is checked before anything is allocated.
pub fn handshake_payload_len(
    header: &[u8; HANDSHAKE_HEADER_LEN],
    expected: HandshakeType,
) -> Result<usize, Error> {
    if &header[..4] != MAGIC || header[4] != VERSION || header[5] != expected as u8 {
        return Err(Error::Malformed);
    }
    let len = u16::from_be_bytes([header[6], header[7]]) as usize;
    let valid = match expected {
        HandshakeType::Hello => (HELLO_FIXED + 1..=HELLO_FIXED + MAX_NETWORK_LEN).contains(&len),
        HandshakeType::Offer => len == OFFER_LEN,
        HandshakeType::Finish => len == CONFIRMATION_LEN,
    };
    if valid {
        Ok(len)
    } else {
        Err(Error::Malformed)
    }
}

fn frame(kind: HandshakeType, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HANDSHAKE_HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(kind as u8);
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// An endpoint's signing identity. It is a role key of its own, distinct
/// from validator and peer keys.
pub struct Identity {
    private: ml_dsa_65::PrivateKey,
    public: [u8; PUBLIC_KEY_LEN],
}

impl Identity {
    pub fn from_seed(seed: &[u8; SEED_LEN]) -> Self {
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(seed);
        Identity {
            private,
            public: public.into_bytes(),
        }
    }

    /// A fresh seed from the operating system's random source.
    pub fn generate_seed() -> Result<Zeroizing<[u8; SEED_LEN]>, Error> {
        random()
    }

    pub fn public_key(&self) -> &[u8; PUBLIC_KEY_LEN] {
        &self.public
    }
}

fn valid_network(network: &[u8]) -> bool {
    (1..=MAX_NETWORK_LEN).contains(&network.len())
}

fn offer_message(
    network: &[u8],
    nonce: &[u8],
    endpoint: &[u8],
    encapsulation_key: &[u8],
    ciphertext: &[u8],
) -> Vec<u8> {
    encode(&[
        SUITE.as_bytes(),
        network,
        nonce,
        endpoint,
        encapsulation_key,
        ciphertext,
    ])
}

fn signature_context() -> Vec<u8> {
    format!("{SUITE}/endpoint-offer").into_bytes()
}

/// Traffic keys and confirmations, from the shared secret with the
/// transcript (the signed offer message and its signature) as the salt.
struct Derived(Zeroizing<[u8; 128]>);

impl Derived {
    fn new(secret: &[u8; 32], offer_message: &[u8], signature: &[u8]) -> Self {
        let transcript = Sha256::digest(encode(&[offer_message, signature]));
        let mut material = Zeroizing::new([0u8; 128]);
        Hkdf::<Sha256>::new(Some(&transcript), secret)
            .expand(
                format!("{SUITE}/traffic-and-confirmation").as_bytes(),
                material.as_mut(),
            )
            .expect("128 bytes is a valid HKDF-SHA-256 length");
        Derived(material)
    }
    fn key(&self, range: std::ops::Range<usize>) -> Zeroizing<[u8; 32]> {
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&self.0[range]);
        out
    }
    fn client_to_endpoint(&self) -> Zeroizing<[u8; 32]> {
        self.key(0..32)
    }
    fn endpoint_to_client(&self) -> Zeroizing<[u8; 32]> {
        self.key(32..64)
    }
    fn endpoint_confirmation(&self) -> Zeroizing<[u8; 32]> {
        self.key(64..96)
    }
    fn client_confirmation(&self) -> Zeroizing<[u8; 32]> {
        self.key(96..128)
    }
}

/// An established channel: seal what you send, open what you receive.
pub struct Session {
    pub sealer: Sealer,
    pub opener: Opener,
}

/// A client between its hello and the endpoint's offer.
pub struct ClientStart {
    network: Vec<u8>,
    nonce: [u8; NONCE_LEN],
    endpoint: Vec<u8>,
    encapsulation_key: [u8; ml_kem_768::EK_LEN],
    decapsulation_key: ml_kem_768::DecapsKey,
}

/// Starts a handshake to the endpoint whose full public key the client
/// pinned. Returns the state and the hello frame to send.
pub fn client_hello(network: &str, endpoint_key: &[u8]) -> Result<(ClientStart, Vec<u8>), Error> {
    let seeds = ClientSeeds {
        nonce: random()?,
        d: random()?,
        z: random()?,
    };
    client_hello_with(network, endpoint_key, &seeds)
}

/// The client's randomness: its nonce and the ML-KEM key generation seeds
/// (FIPS 203 `d` and `z`). Fixed seeds give reproducible test vectors.
pub(crate) struct ClientSeeds {
    pub nonce: Zeroizing<[u8; 32]>,
    pub d: Zeroizing<[u8; 32]>,
    pub z: Zeroizing<[u8; 32]>,
}

/// The endpoint's randomness: the ML-KEM encapsulation seed (FIPS 203 `m`)
/// and the ML-DSA signing randomness (FIPS 204 `rnd`).
pub(crate) struct EndpointSeeds {
    pub m: Zeroizing<[u8; 32]>,
    pub rnd: Zeroizing<[u8; 32]>,
}

fn random() -> Result<Zeroizing<[u8; 32]>, Error> {
    let mut out = Zeroizing::new([0u8; 32]);
    OsRng
        .try_fill_bytes(out.as_mut())
        .map_err(|_| Error::Randomness)?;
    Ok(out)
}

pub(crate) fn client_hello_with(
    network: &str,
    endpoint_key: &[u8],
    seeds: &ClientSeeds,
) -> Result<(ClientStart, Vec<u8>), Error> {
    let network = network.as_bytes();
    if !valid_network(network) || endpoint_key.len() != PUBLIC_KEY_LEN {
        return Err(Error::Malformed);
    }
    let nonce = *seeds.nonce;
    let (encapsulation, decapsulation_key) = ml_kem_768::KG::keygen_from_seed(*seeds.d, *seeds.z);
    let encapsulation_key = encapsulation.into_bytes();
    let mut payload = Vec::with_capacity(HELLO_FIXED + network.len());
    payload.push(network.len() as u8);
    payload.extend_from_slice(network);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(endpoint_key);
    payload.extend_from_slice(&encapsulation_key);
    let start = ClientStart {
        network: network.to_vec(),
        nonce,
        endpoint: endpoint_key.to_vec(),
        encapsulation_key,
        decapsulation_key,
    };
    Ok((start, frame(HandshakeType::Hello, &payload)))
}

impl ClientStart {
    /// Checks the endpoint's offer payload: the signature by the pinned key,
    /// then the endpoint's key confirmation. Returns the session and the
    /// finish frame to send before any record.
    pub fn finish(self, offer: &[u8]) -> Result<(Session, Vec<u8>), Error> {
        if offer.len() != OFFER_LEN {
            return Err(Error::Malformed);
        }
        let (ciphertext, rest) = offer.split_at(ml_kem_768::CT_LEN);
        let (signature, confirmation) = rest.split_at(SIGNATURE_LEN);
        let message = offer_message(
            &self.network,
            &self.nonce,
            &self.endpoint,
            &self.encapsulation_key,
            ciphertext,
        );
        let key = ml_dsa_65::PublicKey::try_from_bytes(
            self.endpoint
                .as_slice()
                .try_into()
                .map_err(|_| Error::Malformed)?,
        )
        .map_err(|_| Error::Rejected)?;
        let signature_array: &[u8; SIGNATURE_LEN] =
            signature.try_into().map_err(|_| Error::Malformed)?;
        if !key.verify(&message, signature_array, &signature_context()) {
            return Err(Error::Rejected);
        }
        let ciphertext = ml_kem_768::CipherText::try_from_bytes(
            ciphertext.try_into().map_err(|_| Error::Malformed)?,
        )
        .map_err(|_| Error::Malformed)?;
        let secret = self
            .decapsulation_key
            .try_decaps(&ciphertext)
            .map_err(|_| Error::Rejected)?;
        let secret = Zeroizing::new(secret.into_bytes());
        let derived = Derived::new(&secret, &message, signature);
        if !bool::from(
            derived
                .endpoint_confirmation()
                .as_slice()
                .ct_eq(confirmation),
        ) {
            return Err(Error::Rejected);
        }
        let session = Session {
            sealer: Sealer::new(&derived.client_to_endpoint()),
            opener: Opener::new(&derived.endpoint_to_client()),
        };
        Ok((
            session,
            frame(
                HandshakeType::Finish,
                derived.client_confirmation().as_ref(),
            ),
        ))
    }
}

/// An endpoint between its offer and the client's finish.
pub struct EndpointPending {
    derived: Derived,
}

/// Answers a client's hello payload. The hello must name this network and
/// this endpoint's own key. Returns the state and the offer frame to send.
pub fn endpoint_offer(
    identity: &Identity,
    network: &str,
    hello: &[u8],
) -> Result<(EndpointPending, Vec<u8>), Error> {
    let seeds = EndpointSeeds {
        m: random()?,
        rnd: random()?,
    };
    endpoint_offer_with(identity, network, hello, &seeds)
}

pub(crate) fn endpoint_offer_with(
    identity: &Identity,
    network: &str,
    hello: &[u8],
    seeds: &EndpointSeeds,
) -> Result<(EndpointPending, Vec<u8>), Error> {
    let expected = network.as_bytes();
    if !valid_network(expected) {
        return Err(Error::Malformed);
    }
    let (&length, rest) = hello.split_first().ok_or(Error::Malformed)?;
    let length = length as usize;
    if !(1..=MAX_NETWORK_LEN).contains(&length) || hello.len() != HELLO_FIXED + length {
        return Err(Error::Malformed);
    }
    let (claimed, rest) = rest.split_at(length);
    let (nonce, rest) = rest.split_at(NONCE_LEN);
    let (endpoint, encapsulation_key) = rest.split_at(PUBLIC_KEY_LEN);
    if claimed != expected || endpoint != identity.public.as_slice() {
        return Err(Error::Rejected);
    }
    // FIPS 203 input check: every coefficient of the key is below q.
    let key = ml_kem_768::EncapsKey::try_from_bytes(
        encapsulation_key.try_into().map_err(|_| Error::Malformed)?,
    )
    .map_err(|_| Error::Malformed)?;
    let (secret, ciphertext) = key.encaps_from_seed(&seeds.m);
    let secret = Zeroizing::new(secret.into_bytes());
    let ciphertext = ciphertext.into_bytes();
    let message = offer_message(expected, nonce, endpoint, encapsulation_key, &ciphertext);
    let signature = identity
        .private
        .try_sign_with_seed(&seeds.rnd, &message, &signature_context())
        .map_err(|_| Error::Rejected)?;
    let derived = Derived::new(&secret, &message, &signature);
    let mut payload = Vec::with_capacity(OFFER_LEN);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&signature);
    payload.extend_from_slice(derived.endpoint_confirmation().as_ref());
    Ok((
        EndpointPending { derived },
        frame(HandshakeType::Offer, &payload),
    ))
}

impl EndpointPending {
    /// Checks the client's finish payload. Only then may records be opened.
    pub fn finish(self, finish: &[u8]) -> Result<Session, Error> {
        if finish.len() != CONFIRMATION_LEN {
            return Err(Error::Malformed);
        }
        if !bool::from(self.derived.client_confirmation().as_slice().ct_eq(finish)) {
            return Err(Error::Rejected);
        }
        Ok(Session {
            sealer: Sealer::new(&self.derived.endpoint_to_client()),
            opener: Opener::new(&self.derived.client_to_endpoint()),
        })
    }
}
