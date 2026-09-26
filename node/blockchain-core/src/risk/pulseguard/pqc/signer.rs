use crate::crypto::PQCManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone)]
pub struct PqcSigner {
    pub(crate) inner: Arc<PQCManager>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignedPayload {
    pub algo: String,
    pub signature_hex: String,
    pub sha256_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegrityManifest {
    pub items: Vec<ManifestItem>,
    pub generated_at: u64,
    pub algo: String,
    pub aggregate_sha256: String,
    pub signature_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestItem {
    pub path: String,
    pub sha256_hex: String,
}

impl PqcSigner {
    pub fn new(inner: Arc<PQCManager>) -> Self {
        Self { inner }
    }

    pub fn sign(&self, data: &[u8]) -> Result<SignedPayload, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let sig = self.inner.sign_message(data)?;
        Ok(SignedPayload {
            algo: sig.algorithm,
            signature_hex: hex::encode(sig.signature),
            sha256_hex: hex::encode(digest),
        })
    }

    pub fn verify_payload(
        &self,
        data: &[u8],
        signed: &SignedPayload,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Re-hash and compare digest; signature verification uses PQCManager verify
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest_hex = hex::encode(hasher.finalize());
        if digest_hex != signed.sha256_hex {
            return Ok(false);
        }
        // Convert signature hex back to bytes
        let sig_bytes = hex::decode(&signed.signature_hex)?;
        // Rebuild PQCSignature wrapper for verification
        let pqc_sig = crate::crypto::PQCSignature {
            signature: sig_bytes,
            algorithm: signed.algo.clone(),
            nonce: 0,
            timestamp: 0,
        };
        let ok =
            self.inner
                .verify_signature(data, &pqc_sig, self.inner.get_dilithium_public_key())?;
        Ok(ok)
    }

    pub fn build_manifest(
        &self,
        paths: &[(&str, Vec<u8>)],
    ) -> Result<IntegrityManifest, Box<dyn std::error::Error>> {
        let mut items = Vec::new();
        let mut agg = Sha256::new();
        for (p, bytes) in paths {
            let mut h = Sha256::new();
            h.update(bytes);
            let digest = h.finalize();
            agg.update(digest);
            items.push(ManifestItem {
                path: p.to_string(),
                sha256_hex: hex::encode(digest),
            });
        }
        let aggregate_sha256 = hex::encode(agg.finalize());
        let payload = serde_json::to_vec(&items)?; // signing only item list for deterministic payload
        let sig = self.sign(&payload)?;
        Ok(IntegrityManifest {
            items,
            generated_at: crate::risk::pulseguard::now_ms() as u64,
            algo: sig.algo,
            aggregate_sha256,
            signature_hex: sig.signature_hex,
        })
    }

    pub fn verify_manifest(
        &self,
        manifest: &IntegrityManifest,
        file_loader: impl Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Recompute aggregate
        let mut agg = Sha256::new();
        for item in &manifest.items {
            let content = match file_loader(&item.path) {
                Some(b) => b,
                None => return Ok(false),
            };
            let mut h = Sha256::new();
            h.update(&content);
            let digest = h.finalize();
            if hex::encode(digest) != item.sha256_hex {
                return Ok(false);
            }
            agg.update(hex::decode(&item.sha256_hex)?);
        }
        let recomputed = hex::encode(agg.finalize());
        if recomputed != manifest.aggregate_sha256 {
            return Ok(false);
        }
        // Verify signature over serialized items list
        let payload = serde_json::to_vec(&manifest.items)?;
        let signed = SignedPayload {
            algo: manifest.algo.clone(),
            signature_hex: manifest.signature_hex.clone(),
            sha256_hex: String::new(),
        }; // sha unused for manifest verify
        self.verify_payload(&payload, &signed)
    }

    pub fn signed_headers_for(
        &self,
        data: &[u8],
    ) -> Result<[(String, String); 3], Box<dyn std::error::Error>> {
        let s = self.sign(data)?;
        Ok([
            ("x-pqc-algo".into(), s.algo),
            ("x-pqc-sig".into(), s.signature_hex),
            ("x-evidence-sha256".into(), s.sha256_hex),
        ])
    }
}
