//! Version 1 account addresses. Address validity does not prove account ownership.
use crate::sha3_256;
use anyhow::{ensure, Context, Result};
use bech32::{primitives::decode::CheckedHrpstring, Bech32m, Hrp};

pub const ADDRESS_VERSION: u8 = 1;
pub const ACCOUNT_KIND: u8 = 1;
const PAYLOAD_LEN: usize = 34;
const ORIGIN_DOMAIN: &[u8] = b"DYTALLIX/ACCOUNT-ORIGIN/V1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressNetwork {
    Mainnet,
    Testnet,
    Development,
}
impl AddressNetwork {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Mainnet => "dytallix",
            Self::Testnet => "tdytallix",
            Self::Development => "ddytallix",
        }
    }
    pub const fn code(self) -> u8 {
        match self {
            Self::Mainnet => 1,
            Self::Testnet => 2,
            Self::Development => 3,
        }
    }
}

/// Exact algorithm used by the initial account authorization key.
/// These identifiers do not select algorithms permitted by mainnet policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginKeyAlgorithm {
    MlDsa65,
    MlDsa87,
    LegacyDilithium5,
}
impl OriginKeyAlgorithm {
    pub const fn code(self) -> u16 {
        match self {
            Self::MlDsa65 => 1,
            Self::MlDsa87 => 2,
            Self::LegacyDilithium5 => 0x8001,
        }
    }
    pub const fn public_key_len(self) -> usize {
        match self {
            Self::MlDsa65 => 1952,
            Self::MlDsa87 | Self::LegacyDilithium5 => 2592,
        }
    }
}

/// Stable account identifier. Current signing keys belong in account authorization state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountAddress {
    network: AddressNetwork,
    account_id: [u8; 32],
}
impl AccountAddress {
    pub const fn from_account_id(network: AddressNetwork, account_id: [u8; 32]) -> Self {
        Self {
            network,
            account_id,
        }
    }
    /// Derive an identifier once, at account creation. Do not repeat this on key rotation.
    /// Key length is a format check. The caller must validate the key and prove possession.
    pub fn from_origin_key(
        network: AddressNetwork,
        chain_id: &str,
        algorithm: OriginKeyAlgorithm,
        public_key: &[u8],
    ) -> Result<Self> {
        ensure!(
            !chain_id.is_empty() && chain_id.len() <= 255,
            "Address origin chain ID must contain 1 to 255 UTF-8 bytes"
        );
        ensure!(
            public_key.len() == algorithm.public_key_len(),
            "Address origin public key length differs from algorithm"
        );
        let mut origin = ORIGIN_DOMAIN.to_vec();
        origin.push(network.code());
        origin.extend_from_slice(&(chain_id.len() as u16).to_be_bytes());
        origin.extend_from_slice(chain_id.as_bytes());
        origin.extend_from_slice(&algorithm.code().to_be_bytes());
        origin.extend_from_slice(&(public_key.len() as u32).to_be_bytes());
        origin.extend_from_slice(public_key);
        Ok(Self::from_account_id(network, sha3_256(&origin)))
    }
    pub const fn network(&self) -> AddressNetwork {
        self.network
    }
    pub const fn account_id(&self) -> &[u8; 32] {
        &self.account_id
    }
    pub fn encode(&self) -> String {
        let mut payload = [0u8; PAYLOAD_LEN];
        payload[0] = ADDRESS_VERSION;
        payload[1] = ACCOUNT_KIND;
        payload[2..].copy_from_slice(&self.account_id);
        let hrp = Hrp::parse(self.network.prefix()).expect("constant address prefix");
        bech32::encode::<Bech32m>(hrp, &payload).expect("fixed address payload fits Bech32m")
    }
    /// Require the expected network and canonical lowercase encoding.
    /// Re-encoding also rejects unused nonzero bits and additional padding symbols.
    pub fn decode(network: AddressNetwork, value: &str) -> Result<Self> {
        let encoded_len = network.prefix().len() + 1 + (PAYLOAD_LEN * 8).div_ceil(5) + 6;
        ensure!(
            value.len() == encoded_len,
            "Address length differs from version 1"
        );
        ensure!(
            value.is_ascii() && !value.bytes().any(|b| b.is_ascii_uppercase()),
            "Address must use canonical lowercase ASCII"
        );
        let parsed = CheckedHrpstring::new::<Bech32m>(value).context("Invalid Bech32m address")?;
        ensure!(
            parsed.hrp().as_str() == network.prefix(),
            "Address network differs"
        );
        let payload: Vec<u8> = parsed.byte_iter().collect();
        ensure!(
            payload.len() == PAYLOAD_LEN,
            "Address payload length differs"
        );
        ensure!(payload[0] == ADDRESS_VERSION, "Unsupported address version");
        ensure!(
            payload[1] == ACCOUNT_KIND,
            "Unsupported address account type"
        );
        let mut account_id = [0u8; 32];
        account_id.copy_from_slice(&payload[2..]);
        let address = Self::from_account_id(network, account_id);
        ensure!(address.encode() == value, "Noncanonical address padding");
        Ok(address)
    }
}
