use dytallix_fast_node::{state::State, storage::state::Storage};
use std::sync::Arc;

#[test]
fn nonce_snapshot_preserves_cache_precedence_and_does_not_populate_cache() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    assert_eq!(state.snapshot_nonce("absent"), 0);
    assert!(!state.accounts.contains_key("absent"));

    state.storage.set_nonce_db("alice", 7).unwrap();
    assert_eq!(state.snapshot_nonce("alice"), 7);
    assert!(!state.accounts.contains_key("alice"));

    let mut account = state.get_account("alice");
    account.nonce = 9;
    account.set_balance("udrt", 123);
    state.accounts.insert("alice".into(), account);
    state.storage.set_nonce_db("alice", 8).unwrap();
    assert_eq!(state.snapshot_nonce("alice"), 9);
    assert_eq!(state.snapshot_account("alice").nonce, 9);
    assert_eq!(state.snapshot_account("alice").balance_of("udrt"), 123);
    assert_eq!(state.storage.get_nonce_db("alice"), 8);

    state.accounts.remove("alice");
    assert_eq!(state.snapshot_nonce("alice"), 8);
    assert!(!state.accounts.contains_key("alice"));
}
