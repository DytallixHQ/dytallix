//! Epoch observation contract v1: observations are derived from committed
//! blocks, so no transaction submitter or proposer can choose issuance inputs.
use super::*;

const EPOCH_BLOCKS: u64 = 2;

fn epoch_zero(inputs: &Inputs, path: &std::path::Path, txs: Vec<Vec<u8>>) -> ConsensusApplication {
    let mut app = inputs.initialized(path);
    app.finalize_block(block(1, txs)).unwrap();
    app.commit().unwrap();
    commit_empty(&mut app, 2);
    app
}

fn derived(app: &ConsensusApplication) -> EpochObservation {
    derived_observation(&app.storage, &app.config, EPOCH_BLOCKS, 3, &block(2, vec![]).hash)
        .unwrap()
        .unwrap()
}

#[test]
fn utilization_is_committed_block_space_over_epoch_capacity() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let send = filler();
    let app = epoch_zero(&inputs, &dir.path().join("db"), vec![send.clone()]);
    let observation = derived(&app);
    let capacity = u128::from(EPOCH_BLOCKS) * app.config.max_block_bytes as u128;
    assert_eq!(
        u128::from(observation.utilization_ppm),
        send.len() as u128 * 1_000_000 / capacity
    );
    assert!(observation.utilization_ppm > 0);
    assert_eq!(observation.volatility_ppm, 0);
    assert_eq!((observation.first_height, observation.last_height), (1, 2));
}

#[test]
fn forged_observation_values_are_rejected_before_any_write() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = epoch_zero(&inputs, &dir.path().join("db"), vec![]);
    let before = data(&app);
    for forge in [
        |o: &mut EpochObservation| o.utilization_ppm = 999_999,
        |o: &mut EpochObservation| o.volatility_ppm = 1,
    ] {
        let mut observation = derived(&app);
        forge(&mut observation);
        let raw = serde_json::to_vec(&WireTransaction::EpochObservation { observation }).unwrap();
        assert_ne!(app.check_tx(&raw).code, 0);
        assert!(app.finalize_block(block(3, vec![raw])).is_err());
        assert_eq!(data(&app), before);
    }
}

#[test]
fn submitted_observations_are_refused_even_when_correct() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = epoch_zero(&inputs, &dir.path().join("db"), vec![]);
    let raw = serde_json::to_vec(&WireTransaction::EpochObservation {
        observation: derived(&app),
    })
    .unwrap();
    assert_ne!(app.check_tx(&raw).code, 0);
}

#[test]
fn boundary_proposal_derives_its_observation_without_mempool_input() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = epoch_zero(&inputs, &dir.path().join("db"), vec![]);
    let expected = serde_json::to_vec(&WireTransaction::EpochObservation {
        observation: derived(&app),
    })
    .unwrap();
    let selected = app.prepare_proposal(3, 30, 17, vec![], 1_000_000).unwrap();
    assert_eq!(selected, vec![expected]);
    let result = app.finalize_block(block(3, selected)).unwrap();
    assert_eq!(result.tx_results[0], TxResult::observation());
    app.commit().unwrap();
    assert_eq!(timing(&app).active_epoch, 1);
}
