//! Fixed compatibility examples for the modular extraction.
use dytallix_fast_node::{
    crypto::canonical_json,
    gas::GasMeter,
    storage::{blocks::Block, receipts::TxReceipt, state::Storage, tx::Transaction},
    types::tx::{Msg, Tx},
};

#[test]
fn canonical_transaction_bytes_remain_stable() {
    let tx = Tx::new(
        "dyt-test",
        7,
        vec![Msg::Send {
            from: "alice".into(),
            to: "bob".into(),
            denom: "udgt".into(),
            amount: u128::MAX,
        }],
        1,
        "fixture",
    )
    .unwrap();
    let expected = concat!(
        r#"{"chain_id":"dyt-test","fee":"1","memo":"fixture","msgs":[{"amount":"340282366920938463463374607431768211455","denom":"udgt","from":"alice","to":"bob","type":"send"}],"nonce":7}"#
    );
    assert_eq!(canonical_json(&tx).unwrap(), expected.as_bytes());
    assert_eq!(serde_json::from_str::<Tx>(expected).unwrap(), tx);
    assert_eq!(
        tx.tx_hash().unwrap(),
        "0xbccd530e7038d1c2e93e62d46d9fb5c76b452157ab827a04a0ac24ea01f426a8"
    );
}

#[test]
fn legacy_storage_records_keep_defaults_and_decimal_amounts() {
    let json = r#"{"hash":"0x0123456789abcdef","from":"alice","to":"bob","amount":"42","fee":"1","nonce":7,"signature":null}"#;
    let tx: Transaction = serde_json::from_str(json).unwrap();
    assert_eq!(tx.amount, 42);
    assert_eq!(tx.denom, "udgt");
    assert_eq!(tx.gas_limit, 0);
    assert!(tx.messages.is_none());
    let receipt = TxReceipt::pending(&tx);
    let encoded = serde_json::to_value(receipt).unwrap();
    assert_eq!(encoded["amount"], "42");
    assert_eq!(encoded["receipt_version"], 1);
}

#[test]
fn stored_block_survives_database_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let tx = Transaction::base(
        "0x0123456789abcdef0123456789abcdef",
        "dyt1alice123456789",
        "dyt1bob123456789",
        42,
        1,
        7,
    );
    let block = Block::new(1, "0xparent".into(), 12345, vec![tx.clone()]);
    let receipt = TxReceipt::pending(&tx);
    {
        let storage = Storage::open(directory.path().to_path_buf()).unwrap();
        storage.put_block(&block, &[receipt]).unwrap();
    }
    let storage = Storage::open(directory.path().to_path_buf()).unwrap();
    assert_eq!(storage.height(), 1);
    assert_eq!(storage.best_hash(), block.hash);
    let bytes = storage
        .db
        .get(format!("blk_hash:{}", block.hash))
        .unwrap()
        .unwrap();
    let restored: Block = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(block).unwrap()
    );
    assert!(storage
        .db
        .get(format!("rcpt:{}", tx.hash))
        .unwrap()
        .is_some());
}

#[test]
fn gas_accounting_preserves_failed_operation_state() {
    let mut meter = GasMeter::new(10);
    meter.consume(7, "first").unwrap();
    assert!(meter.consume(4, "over-limit").is_err());
    assert_eq!(meter.gas_used(), 7);
    assert_eq!(meter.remaining_gas(), 3);
    assert!(!meter.operations().contains_key("over-limit"));
}
