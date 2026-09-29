//! The endpoint pin: what a client needs to reach an endpoint.
//!
//! `{"version":1,"network":"...","address":"host:port","public_key_base64":"..."}`
//!
//! The operator publishes it. Clients and the node's supervisor read it, and
//! trust it only as far as the channel through which they received it.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, MAX_NETWORK_LEN, PUBLIC_KEY_LEN};

pub const MAX_PIN_BYTES: usize = 8192;
const VERSION: u8 = 1;
const MAX_ADDRESS: usize = 255;

/// An endpoint's network (the chain ID), address and full ML-DSA-65 key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointPin {
    pub network: String,
    /// `host:port`: an IP literal (IPv6 in brackets) or a DNS name. The key,
    /// not the address, authenticates the endpoint.
    pub address: String,
    pub public_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinFile {
    version: u8,
    network: String,
    address: String,
    public_key_base64: String,
}

fn printable(value: &str) -> bool {
    value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn valid_address(address: &str) -> bool {
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    let host = match host.strip_prefix('[') {
        Some(inner) => match inner.strip_suffix(']') {
            Some(v6) if v6.parse::<std::net::Ipv6Addr>().is_ok() => v6,
            _ => return false,
        },
        None if host.contains(':') => return false,
        None => host,
    };
    address.len() <= MAX_ADDRESS
        && !host.is_empty()
        && printable(address)
        && !port.starts_with('0')
        && port.parse::<u16>().is_ok_and(|p| p > 0)
}

impl EndpointPin {
    pub fn new(network: &str, address: &str, public_key: &[u8]) -> Result<Self, Error> {
        let valid = (1..=MAX_NETWORK_LEN).contains(&network.len())
            && printable(network)
            && valid_address(address)
            && public_key.len() == PUBLIC_KEY_LEN;
        if !valid {
            return Err(Error::Malformed);
        }
        Ok(EndpointPin {
            network: network.to_owned(),
            address: address.to_owned(),
            public_key: public_key.to_vec(),
        })
    }

    /// Parses a pin file strictly: the exact fields, version 1 and canonical
    /// base64 of a full key.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_PIN_BYTES {
            return Err(Error::Limit);
        }
        let file: PinFile = serde_json::from_slice(bytes).map_err(|_| Error::Malformed)?;
        let key = STANDARD
            .decode(&file.public_key_base64)
            .map_err(|_| Error::Malformed)?;
        if file.version != VERSION || STANDARD.encode(&key) != file.public_key_base64 {
            return Err(Error::Malformed);
        }
        EndpointPin::new(&file.network, &file.address, &key)
    }

    /// The pin as a compact JSON line.
    pub fn to_json(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec(&PinFile {
            version: VERSION,
            network: self.network.clone(),
            address: self.address.clone(),
            public_key_base64: STANDARD.encode(&self.public_key),
        })
        .expect("a pin serializes");
        out.push(b'\n');
        out
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public_key)
    }
}

/// The SHA-256 of a full public key, in lowercase hex, for people to compare.
pub fn fingerprint(public_key: &[u8]) -> String {
    Sha256::digest(public_key)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
