//! Current core signature integration with generated Dilithium3 keys.
//! Certificate chains, sender ownership, block headers, and consensus need separate qualification.
use anyhow::Result;
use dytallix_node::consensus::{
    ai_integration::{AIIntegrationConfig, AIIntegrationManager, AIVerificationResult},
    signature_verification::{SignatureVerifier, VerificationConfig, VerificationError},
    AIResponsePayload, AIResponseSignature, AIServiceType, OracleIdentity, SignedAIOracleResponse,
};
use dytallix_node::types::{
    Block, BlockHeader, PQCBlockSignature, PQCTransactionSignature, Transaction,
    TransferTransaction,
};
use dytallix_pqc::{PQCManager, Signature, SignatureAlgorithm};

fn config() -> VerificationConfig {
    VerificationConfig {
        enforce_certificate_validation: false,
        ..VerificationConfig::default()
    }
}

fn signed_response() -> Result<SignedAIOracleResponse> {
    let signer = PQCManager::new()?;
    assert_eq!(
        *signer.get_signature_algorithm(),
        SignatureAlgorithm::Dilithium3
    );
    let key = signer.get_signature_public_key().to_vec();
    let mut identity = OracleIdentity::new(
        "fixture-oracle".into(),
        "Fixture Oracle".into(),
        key.clone(),
        SignatureAlgorithm::Dilithium3,
    );
    identity.reputation_score = 0.95;
    let mut payload = AIResponsePayload::success(
        "fixture-request".into(),
        AIServiceType::FraudDetection,
        serde_json::json!({"risk_score": 0.3, "confidence": 0.95}),
    );
    payload.timestamp = chrono::Utc::now().timestamp() as u64;
    payload.nonce = "101".into();
    let signature = AIResponseSignature::new(SignatureAlgorithm::Dilithium3, Vec::new(), key);
    let mut response = SignedAIOracleResponse::new(
        payload,
        signature,
        101,
        chrono::Utc::now().timestamp() as u64 + 300,
        identity,
    );
    response.signature.signature = signer.sign(&response.get_signable_data()?)?.data;
    Ok(response)
}

fn signed_transfer() -> Result<Transaction> {
    let signer = PQCManager::new()?;
    let mut transfer = TransferTransaction::new(
        "fixture-sender".into(),
        "fixture-recipient".into(),
        1000,
        10,
        1,
    );
    transfer.signature = PQCTransactionSignature {
        signature: signer.sign(&transfer.signing_message())?,
        public_key: signer.get_signature_public_key().to_vec(),
    };
    Ok(Transaction::Transfer(transfer))
}

#[test]
fn oracle_registration_preserves_identity_and_stake() -> Result<()> {
    let verifier = SignatureVerifier::new(config())?;
    let response = signed_response()?;
    verifier.register_oracle(response.oracle_identity.clone(), 1000)?;
    let oracle = verifier.get_oracle("fixture-oracle").unwrap();
    assert_eq!(
        oracle.identity.public_key,
        response.oracle_identity.public_key
    );
    assert_eq!(
        oracle.identity.signature_algorithm,
        SignatureAlgorithm::Dilithium3
    );
    assert_eq!(oracle.stake_amount, 1000);
    assert!(oracle.is_active);
    assert_eq!(verifier.list_oracles().len(), 1);
    Ok(())
}

#[tokio::test]
async fn signed_response_passes_ai_manager_and_updates_statistics() -> Result<()> {
    let manager = AIIntegrationManager::new(AIIntegrationConfig {
        verification_config: config(),
        require_ai_verification: true,
        ..AIIntegrationConfig::default()
    })
    .await?;
    let response = signed_response()?;
    manager
        .register_oracle(response.oracle_identity.clone(), 1000)
        .await?;
    let result = manager.verify_ai_response(&response, None).await;
    match result {
        AIVerificationResult::Verified {
            oracle_id,
            response_id,
            ..
        } => {
            assert_eq!(oracle_id, "fixture-oracle");
            assert_eq!(response_id, response.response.id);
        }
        other => panic!("valid fixture was not verified: {other:?}"),
    }
    let stats = manager.get_statistics().await;
    assert_eq!(stats.total_requests, 1);
    assert_eq!(stats.successful_verifications, 1);
    assert_eq!(stats.failed_verifications, 0);
    Ok(())
}

#[test]
fn transfer_signature_verifies_with_generated_key() -> Result<()> {
    let mut transaction = signed_transfer()?;
    assert!(transaction.verify_signature());
    if let Transaction::Transfer(transfer) = &mut transaction {
        transfer.signature.signature.data.clear();
    }
    assert!(!transaction.verify_signature());
    Ok(())
}

#[test]
fn block_transaction_check_verifies_actual_signatures() -> Result<()> {
    let transactions = vec![signed_transfer()?];
    // This API checks transaction signatures only. It does not verify this header.
    let header = BlockHeader {
        number: 1,
        parent_hash: "0".repeat(64),
        transactions_root: BlockHeader::calculate_transactions_root(&transactions),
        state_root: "0".repeat(64),
        timestamp: chrono::Utc::now().timestamp() as u64,
        validator: "fixture-validator".into(),
        signature: PQCBlockSignature {
            signature: Signature {
                data: Vec::new(),
                algorithm: SignatureAlgorithm::Dilithium3,
            },
            public_key: Vec::new(),
        },
        nonce: 0,
    };
    let mut block = Block {
        header,
        transactions,
    };
    assert!(block.verify_transactions());
    block.transactions.clear();
    assert!(!block.verify_transactions());
    Ok(())
}

#[test]
fn oracle_registry_reputation_and_deactivation_are_applied() -> Result<()> {
    let verifier = SignatureVerifier::new(config())?;
    let response = signed_response()?;
    verifier.register_oracle(response.oracle_identity, 1000)?;
    verifier.update_oracle_reputation("fixture-oracle", 0.9)?;
    assert_eq!(
        verifier
            .get_oracle("fixture-oracle")
            .unwrap()
            .identity
            .reputation_score,
        0.9
    );
    verifier.deactivate_oracle("fixture-oracle")?;
    assert!(!verifier.get_oracle("fixture-oracle").unwrap().is_active);
    Ok(())
}

#[test]
fn successful_verification_rejects_a_second_use_of_the_same_nonce() -> Result<()> {
    let verifier = SignatureVerifier::new(config())?;
    let response = signed_response()?;
    verifier.register_oracle(response.oracle_identity.clone(), 1000)?;
    verifier.verify_signed_response(&response, None)?;
    assert!(matches!(
        verifier.verify_signed_response(&response, None),
        Err(VerificationError::ReplayAttack(_))
    ));
    assert_eq!(
        verifier
            .get_oracle("fixture-oracle")
            .unwrap()
            .performance_metrics
            .successful_verifications,
        1
    );
    Ok(())
}

#[tokio::test]
async fn ai_manager_statistics_start_empty() -> Result<()> {
    let manager = AIIntegrationManager::new(AIIntegrationConfig::default()).await?;
    let stats = manager.get_statistics().await;
    assert_eq!(stats.total_requests, 0);
    assert_eq!(stats.successful_verifications, 0);
    assert_eq!(stats.failed_verifications, 0);
    assert_eq!(stats.cache_hits, 0);
    assert_eq!(stats.cache_misses, 0);
    assert_eq!(stats.avg_verification_time_ms, 0.0);
    Ok(())
}

#[test]
fn unregistered_oracle_is_rejected_explicitly() -> Result<()> {
    let verifier = SignatureVerifier::new(config())?;
    assert!(matches!(
        verifier.verify_signed_response(&signed_response()?, None),
        Err(VerificationError::OracleNotFound(_))
    ));
    Ok(())
}
