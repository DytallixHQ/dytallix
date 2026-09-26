use super::PQC;
use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _, SignedMessage as _};

pub struct Dilithium;

impl PQC for Dilithium {
    const ALG: &'static str = "dilithium5";
    fn keypair() -> (Vec<u8>, Vec<u8>) {
        let (pk, sk) = dilithium5::keypair();
        let sk_bytes = sk.as_bytes().to_vec();
        let pk_bytes = pk.as_bytes().to_vec();
        (sk_bytes, pk_bytes)
    }
    fn sign(sk: &[u8], msg: &[u8]) -> Vec<u8> {
        let sk_obj = match dilithium5::SecretKey::from_bytes(sk) {
            Ok(sk_obj) => sk_obj,
            Err(_) => {
                tracing::error!(
                    "Invalid Dilithium5 secret key bytes provided to sign(); returning empty signature"
                );
                return Vec::new();
            }
        };

        let sm = dilithium5::sign(msg, &sk_obj);
        sm.as_bytes().to_vec()
    }
    fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        let pk_obj = match dilithium5::PublicKey::from_bytes(pk) {
            Ok(pk_obj) => pk_obj,
            Err(_) => return false,
        };

        if let Ok(sm) = dilithium5::SignedMessage::from_bytes(sig) {
            return dilithium5::open(&sm, &pk_obj)
                .map(|opened| opened == msg)
                .unwrap_or(false);
        }

        false
    }
}
