use dytallix_adaptive_emission::*;

fn fixture() -> Controller {
    Controller::new(Config {
        target_ppm: 700000,
        shock_threshold_ppm: 100000,
        volatility_threshold_ppm: 500000,
        window_samples: 3,
        integral_min: -500000,
        integral_max: 500000,
        soft: Gains {
            proportional: 200000,
            integral: 20000,
            derivative: 50000,
        },
        hard: Gains {
            proportional: 400000,
            integral: 40000,
            derivative: 100000,
        },
        base_udrt: 1400000,
        min_udrt: 500000,
        max_udrt: 2500000,
    })
    .unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn independent_python_binary_vectors_match_after_every_epoch() {
    let mut c = fixture();
    let vectors: Vec<&str> = include_str!("encoding-vectors.txt").lines().collect();
    assert_eq!(hex(&c.encode()), vectors[0]);
    for (epoch, utilization) in [700000, 600000, 1000000, 0].into_iter().enumerate() {
        c.step(Observation {
            epoch: epoch as u64,
            utilization_ppm: utilization,
            volatility_ppm: 1000000,
        })
        .unwrap();
        let bytes = c.encode();
        assert_eq!(hex(&bytes), vectors[epoch + 1]);
        assert_eq!(Controller::decode(&bytes).unwrap(), c);
        assert_eq!(Controller::decode(&bytes).unwrap().encode(), bytes);
    }
}

#[test]
fn rejects_truncation_trailing_bytes_tags_versions_and_counts() {
    let valid = fixture().encode();
    assert_eq!(valid.len(), SNAPSHOT_HEADER_LEN);
    for length in 0..valid.len() {
        assert!(Controller::decode(&valid[..length]).is_err());
    }
    let mut bad = valid.clone();
    bad.push(0);
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid.clone();
    bad[0] ^= 1;
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid.clone();
    bad[9] = 2;
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid.clone();
    bad[126] = 2;
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid.clone();
    bad[134] = 1;
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid.clone();
    bad[135..139].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Controller::decode(&bad).is_err());
    let mut bad = valid;
    bad[34..38].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Controller::decode(&bad).is_err());
    assert!(Controller::decode(&vec![0; MAX_ENCODED_LEN + 1]).is_err());
}

#[test]
fn restart_preserves_next_command_and_rejects_bad_history() {
    let mut c = fixture();
    for epoch in 0..8 {
        c.step(Observation {
            epoch,
            utilization_ppm: if epoch % 2 == 0 { 0 } else { SCALE },
            volatility_ppm: 0,
        })
        .unwrap();
    }
    let mut restored = Controller::decode(&c.encode()).unwrap();
    let observation = Observation {
        epoch: 8,
        utilization_ppm: 800000,
        volatility_ppm: u64::MAX,
    };
    assert_eq!(c.step(observation), restored.step(observation));
    let mut bad = c.encode();
    bad[SNAPSHOT_HEADER_LEN..SNAPSHOT_HEADER_LEN + 8].copy_from_slice(&i64::MAX.to_be_bytes());
    assert!(Controller::decode(&bad).is_err());
}
