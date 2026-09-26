use anyhow::Result;
use dytallix_node::consensus::{
    AIResponsePayload, AIResponseSignature, AIServiceType, OracleCertificate, OracleIdentity,
    SignatureMetadata, SignedAIOracleResponse, TimestampProof, VerificationData,
};
use dytallix_pqc::SignatureAlgorithm;
use std::time::Duration;

#[tokio::test]
async fn test_ai_response_signature_creation() -> Result<()> {
    // Test creating a new AIResponseSignature
    let signature = AIResponseSignature::new(
        SignatureAlgorithm::Dilithium5,
        vec![1, 2, 3, 4, 5],  // Mock signature bytes
        vec![6, 7, 8, 9, 10], // Mock public key
    );

    assert_eq!(signature.algorithm, SignatureAlgorithm::Dilithium5);
    assert_eq!(signature.signature, vec![1, 2, 3, 4, 5]);
    assert_eq!(signature.public_key, vec![6, 7, 8, 9, 10]);
    assert_eq!(signature.signature_version, 1);
    assert!(signature.signature_timestamp > 0);
    assert!(signature.is_recent(300)); // Should be recent within 5 minutes

    println!("✓ AIResponseSignature creation working");
    Ok(())
}

#[tokio::test]
async fn test_signature_with_metadata() -> Result<()> {
    let metadata = SignatureMetadata {
        key_id: Some("oracle_key_123".to_string()),
        cert_chain: Some(vec!["cert1".to_string(), "cert2".to_string()]),
        parameters: Some(serde_json::json!({"version": "1.0", "mode": "fast"})),
    };

    let signature = AIResponseSignature::new(
        SignatureAlgorithm::Falcon1024,
        vec![10, 20, 30],
        vec![40, 50, 60],
    )
    .with_metadata(metadata);

    assert!(signature.metadata.is_some());
    let meta = signature.metadata.unwrap();
    assert_eq!(meta.key_id, Some("oracle_key_123".to_string()));
    assert_eq!(
        meta.cert_chain,
        Some(vec!["cert1".to_string(), "cert2".to_string()])
    );

    println!("✓ Signature metadata functionality working");
    Ok(())
}

#[tokio::test]
async fn test_oracle_identity_creation() -> Result<()> {
    let oracle = OracleIdentity::new(
        "oracle_001".to_string(),
        "Fraud Detection Oracle".to_string(),
        vec![1, 2, 3, 4, 5, 6, 7, 8],
        SignatureAlgorithm::Dilithium5,
    );

    assert_eq!(oracle.oracle_id, "oracle_001");
    assert_eq!(oracle.name, "Fraud Detection Oracle");
    assert_eq!(oracle.reputation_score, 0.5); // Default neutral reputation
    assert!(oracle.is_active);
    assert!(oracle.registered_at > 0);
    assert!(oracle.certificate_chain.is_empty());

    // Test trust evaluation
    assert!(!oracle.is_trusted(0.7)); // Should not be trusted with 0.5 reputation
    assert!(oracle.is_trusted(0.3)); // Should be trusted with lower threshold

    println!("✓ OracleIdentity creation and trust evaluation working");
    Ok(())
}

#[tokio::test]
async fn test_oracle_reputation_update() -> Result<()> {
    let oracle = OracleIdentity::new(
        "oracle_002".to_string(),
        "Risk Scoring Oracle".to_string(),
        vec![9, 8, 7, 6, 5],
        SignatureAlgorithm::Falcon1024,
    )
    .update_reputation(0.85);

    assert_eq!(oracle.reputation_score, 0.85);
    assert!(oracle.is_trusted(0.8));

    // Test clamping
    let high_oracle = oracle.clone().update_reputation(1.5); // Should clamp to 1.0
    assert_eq!(high_oracle.reputation_score, 1.0);

    let low_oracle = oracle.clone().update_reputation(-0.5); // Should clamp to 0.0
    assert_eq!(low_oracle.reputation_score, 0.0);

    println!("✓ Oracle reputation updates and clamping working");
    Ok(())
}

#[tokio::test]
async fn test_oracle_certificate_creation() -> Result<()> {
    let now = chrono::Utc::now().timestamp() as u64;
    let valid_from = now;
    let valid_until = now + 86400 * 30; // 30 days from now

    let cert = OracleCertificate::new(
        "oracle_003".to_string(),
        "root_authority".to_string(),
        valid_from,
        valid_until,
        vec![11, 22, 33, 44],
        SignatureAlgorithm::Dilithium5,
        vec![55, 66, 77, 88],
    );

    assert_eq!(cert.version, 1);
    assert_eq!(cert.subject_oracle_id, "oracle_003");
    assert_eq!(cert.issuer_oracle_id, "root_authority");
    assert!(cert.is_valid());
    assert!(!cert.is_expired());
    assert!(cert.days_until_expiration() >= 29); // At least 29 days

    println!("✓ OracleCertificate creation and validation working");
    Ok(())
}

#[tokio::test]
async fn test_certificate_expiration() -> Result<()> {
    let now = chrono::Utc::now().timestamp() as u64;
    let expired_cert = OracleCertificate::new(
        "oracle_004".to_string(),
        "root_authority".to_string(),
        now - 86400 * 2, // 2 days ago
        now - 86400,     // 1 day ago (expired)
        vec![1, 2, 3],
        SignatureAlgorithm::Falcon1024,
        vec![4, 5, 6],
    );

    assert!(!expired_cert.is_valid());
    assert!(expired_cert.is_expired());
    assert_eq!(expired_cert.days_until_expiration(), 0);

    println!("✓ Certificate expiration detection working");
    Ok(())
}

#[tokio::test]
async fn test_signed_response_creation() -> Result<()> {
    // Create a mock AI response
    let mut ai_response = AIResponsePayload::success(
        "request_123".to_string(),
        AIServiceType::FraudDetection,
        serde_json::json!({
            "risk_score": 0.25,
            "confidence": 0.95,
            "factors": ["amount_normal", "location_known"]
        }),
    );
    ai_response.processing_time_ms = 150;

    // Create oracle identity
    let oracle = OracleIdentity::new(
        "fraud_oracle_001".to_string(),
        "Primary Fraud Detection Oracle".to_string(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        SignatureAlgorithm::Dilithium5,
    )
    .update_reputation(0.92);

    // Create signature
    let signature = AIResponseSignature::new(
        SignatureAlgorithm::Dilithium5,
        vec![100, 101, 102, 103, 104, 105],  // Mock signature
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10], // Same as oracle's key
    )
    .with_key_id("fraud_oracle_001_key_v1".to_string());

    // Create signed response
    let nonce = 12345678;
    let expires_at = chrono::Utc::now().timestamp() as u64 + 300; // 5 minutes from now

    let signed_response =
        SignedAIOracleResponse::new(ai_response, signature, nonce, expires_at, oracle);

    // Verify the signed response
    assert_eq!(
        signed_response.response.service_type,
        AIServiceType::FraudDetection
    );
    assert_eq!(signed_response.nonce, nonce);
    assert!(!signed_response.is_expired());
    assert!(signed_response.is_fresh());
    assert!(signed_response.seconds_until_expiration() > 250);
    assert_eq!(
        signed_response.oracle_identity.oracle_id,
        "fraud_oracle_001"
    );

    println!("✓ SignedAIOracleResponse creation working");
    Ok(())
}

#[tokio::test]
async fn test_signable_data_generation() -> Result<()> {
    // Create a simple signed response
    let ai_response = AIResponsePayload::success(
        "test_request".to_string(),
        AIServiceType::RiskScoring,
        serde_json::json!({"score": 0.5}),
    );

    let oracle = OracleIdentity::new(
        "test_oracle".to_string(),
        "Test Oracle".to_string(),
        vec![1, 2, 3],
        SignatureAlgorithm::Dilithium5,
    );

    let signature =
        AIResponseSignature::new(SignatureAlgorithm::Dilithium5, vec![4, 5, 6], vec![1, 2, 3]);

    let signed_response = SignedAIOracleResponse::new(
        ai_response,
        signature,
        999888777,
        chrono::Utc::now().timestamp() as u64 + 600,
        oracle,
    );

    // Test signable data generation
    let signable_data = signed_response.get_signable_data()?;
    assert!(!signable_data.is_empty());

    // Should be deterministic - generate again and compare
    let signable_data2 = signed_response.get_signable_data()?;
    assert_eq!(signable_data, signable_data2);

    println!("✓ Signable data generation working and deterministic");
    Ok(())
}

#[tokio::test]
async fn test_signed_response_summary() -> Result<()> {
    let ai_response = AIResponsePayload::success(
        "summary_test".to_string(),
        AIServiceType::ContractAnalysis,
        serde_json::json!({"analysis": "safe"}),
    );

    let oracle = OracleIdentity::new(
        "summary_oracle".to_string(),
        "Summary Test Oracle".to_string(),
        vec![1, 2, 3],
        SignatureAlgorithm::Falcon1024,
    )
    .update_reputation(0.88);

    let signature =
        AIResponseSignature::new(SignatureAlgorithm::Falcon1024, vec![7, 8, 9], vec![1, 2, 3]);

    // Use a far future time to ensure the response is always fresh
    let future_time = chrono::Utc::now().timestamp() as u64 + 3600; // 1 hour from now
    let signed_response =
        SignedAIOracleResponse::new(ai_response, signature, 555444333, future_time, oracle);

    // Test summary generation
    let summary = signed_response.get_summary();

    assert_eq!(summary["oracle_id"], "summary_oracle");
    assert_eq!(summary["service_type"], "ContractAnalysis");
    assert_eq!(summary["status"], "Success");
    assert_eq!(summary["signature_algorithm"], "Falcon1024");
    assert_eq!(summary["nonce"], 555444333);
    assert_eq!(summary["is_fresh"], true);
    assert_eq!(summary["oracle_reputation"], 0.88);

    println!("✓ Signed response summary generation working");
    println!("Summary: {}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

#[test]
fn test_summary_nonce_matches_signed_bytes_without_changing_response() -> Result<()> {
    let mut response = AIResponsePayload::success(
        "nonce_summary_test".to_string(),
        AIServiceType::RiskScoring,
        serde_json::json!({"score": 0.5}),
    );
    // The summary must expose the envelope nonce, not the payload nonce.
    response.nonce = "123".to_string();
    let oracle = OracleIdentity::new(
        "nonce_summary_oracle".to_string(),
        "Nonce Summary Oracle".to_string(),
        vec![1, 2, 3],
        SignatureAlgorithm::Dilithium5,
    );
    let signature =
        AIResponseSignature::new(SignatureAlgorithm::Dilithium5, vec![4, 5, 6], vec![1, 2, 3]);

    for nonce in [0, (1_u64 << 53) + 1, u64::MAX] {
        let signed_response = SignedAIOracleResponse::new(
            response.clone(),
            signature.clone(),
            nonce,
            u64::MAX,
            oracle.clone(),
        );
        let encoded_before = serde_json::to_vec(&signed_response)?;
        let signable_before = signed_response.get_signable_data()?;
        let nonce_offset = serde_json::to_vec(&signed_response.response)?.len();
        let signed_nonce =
            u64::from_be_bytes(signable_before[nonce_offset..nonce_offset + 8].try_into()?);

        let summary = signed_response.get_summary();
        let summary_round_trip: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&summary)?)?;
        assert_eq!(summary_round_trip["nonce"].as_u64(), Some(nonce));
        assert_eq!(summary_round_trip["nonce"].as_u64(), Some(signed_nonce));
        assert_eq!(signed_response.get_signable_data()?, signable_before);
        assert_eq!(serde_json::to_vec(&signed_response)?, encoded_before);
    }
    Ok(())
}

#[tokio::test]
async fn test_verification_data_structures() -> Result<()> {
    // Test TimestampProof
    let timestamp_proof = TimestampProof {
        authority_id: "trusted_timestamp_authority".to_string(),
        proof: vec![10, 20, 30, 40, 50],
        algorithm: "RFC3161".to_string(),
        created_at: chrono::Utc::now().timestamp() as u64,
    };

    // Test VerificationData
    let verification_data = VerificationData {
        request_hash: vec![1, 2, 3, 4, 5, 6, 7, 8], // SHA256 would be 32 bytes
        merkle_proof: Some(vec![
            vec![9, 10, 11, 12],
            vec![13, 14, 15, 16],
            vec![17, 18, 19, 20],
        ]),
        timestamp_proof: Some(timestamp_proof),
        metadata: Some(serde_json::json!({
            "batch_id": "batch_20250706_001",
            "position": 42,
            "total_responses": 100
        })),
    };

    // Add verification data to a signed response
    let ai_response = AIResponsePayload::success(
        "verification_test".to_string(),
        AIServiceType::ThreatDetection,
        serde_json::json!({"threat_level": "low"}),
    );

    let oracle = OracleIdentity::new(
        "verification_oracle".to_string(),
        "Verification Test Oracle".to_string(),
        vec![11, 22, 33],
        SignatureAlgorithm::Dilithium5,
    );

    let signature = AIResponseSignature::new(
        SignatureAlgorithm::Dilithium5,
        vec![44, 55, 66],
        vec![11, 22, 33],
    );

    let signed_response = SignedAIOracleResponse::new(
        ai_response,
        signature,
        123456789,
        chrono::Utc::now().timestamp() as u64 + 900,
        oracle,
    )
    .with_verification_data(verification_data);

    // Verify verification data is present
    assert!(signed_response.verification_data.is_some());
    let ver_data = signed_response.verification_data.unwrap();
    assert_eq!(ver_data.request_hash, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(ver_data.merkle_proof.is_some());
    assert!(ver_data.timestamp_proof.is_some());
    assert!(ver_data.metadata.is_some());

    println!("✓ Verification data structures working");
    Ok(())
}

#[tokio::test]
async fn test_signature_age_and_freshness() -> Result<()> {
    // Test signature age calculation
    let signature =
        AIResponseSignature::new(SignatureAlgorithm::Dilithium5, vec![1, 2, 3], vec![4, 5, 6]);

    // Should be very recent
    assert!(signature.age_seconds() <= 1);
    assert!(signature.is_recent(300)); // Within 5 minutes
    assert!(signature.is_recent(60)); // Within 1 minute
    assert!(signature.is_recent(10)); // Within 10 seconds

    // Test response expiration
    let ai_response = AIResponsePayload::success(
        "freshness_test".to_string(),
        AIServiceType::KYC,
        serde_json::json!({"kyc_status": "verified"}),
    );

    let oracle = OracleIdentity::new(
        "freshness_oracle".to_string(),
        "Freshness Test Oracle".to_string(),
        vec![1, 2, 3],
        SignatureAlgorithm::Dilithium5,
    );

    // Create response that expires in 1 second
    let expires_soon = chrono::Utc::now().timestamp() as u64 + 1;
    let signed_response =
        SignedAIOracleResponse::new(ai_response, signature, 987654321, expires_soon, oracle);

    // Should be fresh initially
    assert!(signed_response.is_fresh());
    assert!(!signed_response.is_expired());
    assert!(signed_response.seconds_until_expiration() <= 1);

    // Wait for expiration
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Should now be expired
    assert!(!signed_response.is_fresh());
    assert!(signed_response.is_expired());
    assert_eq!(signed_response.seconds_until_expiration(), 0);

    println!("✓ Signature age and response freshness checks working");
    Ok(())
}

#[tokio::test]
async fn test_certificate_chain_management() -> Result<()> {
    // Create a certificate chain
    let now = chrono::Utc::now().timestamp() as u64;

    // Root certificate (self-signed)
    let root_cert = OracleCertificate::new(
        "root_authority".to_string(),
        "root_authority".to_string(), // Self-signed
        now,
        now + 86400 * 365,   // 1 year validity
        vec![100, 101, 102], // Root public key
        SignatureAlgorithm::Dilithium5,
        vec![200, 201, 202], // Self-signature
    );

    // Intermediate certificate
    let intermediate_cert = OracleCertificate::new(
        "intermediate_authority".to_string(),
        "root_authority".to_string(),
        now,
        now + 86400 * 180,   // 6 months validity
        vec![110, 111, 112], // Intermediate public key
        SignatureAlgorithm::Dilithium5,
        vec![210, 211, 212], // Signed by root
    );

    // Oracle certificate
    let oracle_cert = OracleCertificate::new(
        "chain_test_oracle".to_string(),
        "intermediate_authority".to_string(),
        now,
        now + 86400 * 90,    // 3 months validity
        vec![120, 121, 122], // Oracle public key
        SignatureAlgorithm::Dilithium5,
        vec![220, 221, 222], // Signed by intermediate
    );

    // Create oracle with certificate chain
    let oracle = OracleIdentity::new(
        "chain_test_oracle".to_string(),
        "Certificate Chain Test Oracle".to_string(),
        vec![120, 121, 122], // Same as in certificate
        SignatureAlgorithm::Dilithium5,
    )
    .add_certificate(oracle_cert)
    .add_certificate(intermediate_cert)
    .add_certificate(root_cert);

    // Verify certificate chain
    assert_eq!(oracle.certificate_chain.len(), 3);
    assert!(oracle.certificate_chain.iter().all(|cert| cert.is_valid()));

    // Check certificate subjects and issuers
    let oracle_cert = &oracle.certificate_chain[0];
    let intermediate_cert = &oracle.certificate_chain[1];
    let root_cert = &oracle.certificate_chain[2];

    assert_eq!(oracle_cert.subject_oracle_id, "chain_test_oracle");
    assert_eq!(oracle_cert.issuer_oracle_id, "intermediate_authority");

    assert_eq!(
        intermediate_cert.subject_oracle_id,
        "intermediate_authority"
    );
    assert_eq!(intermediate_cert.issuer_oracle_id, "root_authority");

    assert_eq!(root_cert.subject_oracle_id, "root_authority");
    assert_eq!(root_cert.issuer_oracle_id, "root_authority"); // Self-signed

    println!("✓ Certificate chain management working");
    Ok(())
}
