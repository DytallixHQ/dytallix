use axum::response::IntoResponse;

use dytallix_fast_node::rpc::errors::ApiError;
use dytallix_fast_node::storage::receipts::{TxReceipt, TxStatus, RECEIPT_FORMAT_VERSION};

#[test]
fn receipt_string_u128_fields() {
    let r = TxReceipt {
        receipt_version: RECEIPT_FORMAT_VERSION,
        tx_hash: "0xabc".into(),
        status: TxStatus::Success,
        block_height: Some(1),
        index: Some(0),
        from: "a".into(),
        to: "b".into(),
        amount: 1000000000000000000u128,
        fee: 2000000000000000000u128,
        nonce: 1,
        error: None,
        gas_used: 0,
        gas_limit: 0,
        gas_price: 0,
        gas_refund: 0,
        success: true,
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"amount\":\"1000000000000000000\""));
    assert!(s.contains("\"fee\":\"2000000000000000000\""));
}

#[test]
fn api_error_shape() {
    let err = ApiError::InvalidNonce {
        expected: 1,
        got: 2,
    };
    let resp = err.into_response();
    let body = futures::executor::block_on(axum::body::to_bytes(resp.into_body(), 1024)).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], "InvalidNonce");
    assert!(v["message"].as_str().unwrap().contains("expected 1"));
}
