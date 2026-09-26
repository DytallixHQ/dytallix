// Archived legacy assertions. Current economics use committed reward/issuance settlement.
#[cfg(feature = "legacy-economic-fixtures")]
mod archived_legacy_fixtures {
    use dytallix_fast_node::runtime::emission::EmissionEngine;
    use dytallix_fast_node::state::State;
    use dytallix_fast_node::storage::state::Storage;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn emission_accumulates_and_claims() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));
        let mut engine = EmissionEngine::new(storage.clone(), state.clone());
        assert_eq!(engine.last_accounted_height(), 0);
        engine.apply_until(5); // 5 blocks
                               // Pools should have positive amounts now
        let before = engine.snapshot();
        assert!(before.pools["block_rewards"] > 0);
        assert!(before.pools["staking_rewards"] > 0);
        // claim 10 from staking_rewards to acct X (succeeds given bootstrap amounts)
        engine.claim("staking_rewards", 10, "acctX").unwrap();
        assert_eq!(
            engine.pool_amount("staking_rewards"),
            before.pools["staking_rewards"] - 10
        );
        // account credited in udrt
        let bal = state.lock().unwrap().balance_of("acctX", "udrt");
        assert_eq!(bal, 10);
    }

    #[test]
    fn emission_boundary_epoch_rollover() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
        let state = Arc::new(Mutex::new(State::new(storage.clone())));
        let mut engine = EmissionEngine::new(storage.clone(), state.clone());
        engine.apply_until(1);
        let p1 = engine.pool_amount("block_rewards");
        engine.apply_until(1); // idempotent if height unchanged
        assert_eq!(engine.pool_amount("block_rewards"), p1);
        engine.apply_until(2);
        assert!(engine.pool_amount("block_rewards") >= p1);
    }
}
