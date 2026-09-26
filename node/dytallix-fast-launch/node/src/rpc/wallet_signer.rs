// Backend Transaction Signing Service for Frontend
// This service handles PQC signing server-side to avoid heavy computation in browser

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    rpc::{RpcContext, SubmitTx, errors::ApiError},
    types::tx::{Msg, SignedTx, Tx},
};

/// Request to sign and submit a transaction
#[derive(Debug, Deserialize)]
pub struct SignTransactionRequest {
    /// Wallet data (encrypted keystore)
    pub wallet: WalletData,
    /// Passphrase to decrypt the wallet
    pub passphrase: String,
    /// Transaction to sign
    pub transaction: TransactionData,
}

#[derive(Debug, Deserialize)]
pub struct WalletData {
    pub address: String,
    pub encrypted_private_key: String,
    pub public_key_b64: String,
    pub salt: String,
    pub iv: String,
}

#[derive(Debug, Deserialize)]
pub struct TransactionData {
    pub to: String,
    pub amount: String, // Amount in micro units (udgt/udrt)
    pub denom: String,  // "udgt" or "udrt"
    pub memo: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SignTransactionResponse {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub error: Option<String>,
}

/// Sign and submit a transaction on behalf of the frontend
pub async fn sign_and_submit_transaction(
    Extension(ctx): Extension<RpcContext>,
    Json(request): Json<SignTransactionRequest>,
) -> Result<Json<SignTransactionResponse>, StatusCode> {
    // 1. Decrypt the private key using the passphrase
    let secret_key = match decrypt_wallet_key(&request.wallet, &request.passphrase) {
        Ok(sk) => sk,
        Err(e) => {
            return Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: None,
                error: Some(format!("Failed to decrypt wallet: {}", e)),
            }));
        }
    };

    // 2. Decode public key
    use base64::Engine;
    let public_key = match base64::engine::general_purpose::STANDARD.decode(&request.wallet.public_key_b64) {
        Ok(pk) => pk,
        Err(e) => {
            return Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: None,
                error: Some(format!("Invalid public key: {}", e)),
            }));
        }
    };

    // 3. Get current nonce from state (non-mutating read)
    let nonce = {
        let state = ctx.state.lock().unwrap();
        state.snapshot_nonce(&request.wallet.address)
    };

    // 4. Parse amount
    let amount: u128 = match request.transaction.amount.parse() {
        Ok(a) => a,
        Err(e) => {
            return Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: None,
                error: Some(format!("Invalid amount: {}", e)),
            }));
        }
    };

    // 5. Build transaction
    let chain_id = ctx.storage.get_chain_id().unwrap_or_else(|| "dyt-local-1".to_string());
    
    let tx = Tx {
        chain_id,
        nonce,
        msgs: vec![Msg::Send {
            from: request.wallet.address.clone(),
            to: request.transaction.to,
            denom: request.transaction.denom,
            amount,
        }],
        fee: 1000, // Standard fee
        memo: request.transaction.memo.unwrap_or_default(),
    };

    // 6. Sign the transaction
    let signed_tx = match SignedTx::sign(tx, &secret_key, &public_key) {
        Ok(stx) => stx,
        Err(e) => {
            return Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: None,
                error: Some(format!("Failed to sign transaction: {}", e)),
            }));
        }
    };

    // 7. Compute tx hash
    let tx_hash = match signed_tx.tx_hash() {
        Ok(hash) => hash,
        Err(e) => {
            return Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: None,
                error: Some(format!("Failed to compute tx hash: {}", e)),
            }));
        }
    };

    // 8. Reuse existing submit logic
    let submit_body = SubmitTx { signed_tx };
    match crate::rpc::submit(Extension(ctx), Json(submit_body)).await {
        Ok(_) => Ok(Json(SignTransactionResponse {
            success: true,
            tx_hash: Some(tx_hash),
            error: None,
        })),
        Err(e) => {
            let error_msg = match e {
                ApiError::InvalidNonce { expected, got } => {
                    format!("Invalid nonce: expected {}, got {}", expected, got)
                }
                ApiError::Validation(v) => format!("Validation error: {:?}", v),
                ApiError::NotFound => "Transaction not found".to_string(),
                _ => format!("Transaction failed: {:?}", e),
            };
            Ok(Json(SignTransactionResponse {
                success: false,
                tx_hash: Some(tx_hash),
                error: Some(error_msg),
            }))
        }
    }
}

/// Decrypt wallet private key using passphrase
fn decrypt_wallet_key(wallet: &WalletData, passphrase: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use argon2::Argon2;
    use base64::Engine;

    // Decode components
    let encrypted_key = base64::engine::general_purpose::STANDARD
        .decode(&wallet.encrypted_private_key)
        .map_err(|e| format!("Invalid encrypted key: {}", e))?;
    let salt_bytes = base64::engine::general_purpose::STANDARD
        .decode(&wallet.salt)
        .map_err(|e| format!("Invalid salt: {}", e))?;
    let iv = base64::engine::general_purpose::STANDARD
        .decode(&wallet.iv)
        .map_err(|e| format!("Invalid IV: {}", e))?;

    // Derive key from passphrase using Argon2
    let argon2 = Argon2::default();
    let mut key = [0u8; 32];
    
    // Use the raw salt bytes directly with hash_password_into
    argon2
        .hash_password_into(passphrase.as_bytes(), &salt_bytes, &mut key)
        .map_err(|e| format!("Key derivation failed: {}", e))?;

    // Decrypt using AES-256-GCM
    let cipher = Aes256Gcm::new(key.as_ref().into());
    let nonce = Nonce::from_slice(&iv);
    
    cipher
        .decrypt(nonce, encrypted_key.as_ref())
        .map_err(|e| format!("Decryption failed: {}", e))
}
