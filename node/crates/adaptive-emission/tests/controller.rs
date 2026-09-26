use dytallix_adaptive_emission::*;

fn config() -> Config {
    Config {
        target_ppm: 700_000,
        shock_threshold_ppm: 100_000,
        volatility_threshold_ppm: 500_000,
        window_samples: 3,
        integral_min: -500_000,
        integral_max: 500_000,
        soft: Gains {
            proportional: 200_000,
            integral: 20_000,
            derivative: 50_000,
        },
        hard: Gains {
            proportional: 400_000,
            integral: 40_000,
            derivative: 100_000,
        },
        base_udrt: 1_400_000,
        min_udrt: 500_000,
        max_udrt: 2_500_000,
    }
}

#[test]
fn independent_fraction_vectors_and_restart() {
    let mut controller = Controller::new(config()).unwrap();
    for line in include_str!("vectors.tsv").lines().skip(1) {
        let v: Vec<i128> = line.split('\t').map(|s| s.parse().unwrap()).collect();
        let observation = Observation {
            epoch: v[0] as u64,
            utilization_ppm: v[1] as u64,
            volatility_ppm: v[2] as u64,
        };
        let before = controller.snapshot();
        let command = controller.step(observation).unwrap();
        let mut restarted = Controller::restore(before).unwrap();
        assert_eq!(restarted.step(observation).unwrap(), command);
        assert_eq!(restarted.snapshot(), controller.snapshot());
        assert_eq!(command.for_epoch, observation.epoch + 1);
        assert_eq!(command.error_ppm as i128, v[3]);
        assert_eq!(command.integral_ppm as i128, v[4]);
        assert_eq!(command.derivative_ppm as i128, v[5]);
        assert_eq!(
            command.regime,
            if v[6] == 0 {
                Regime::Soft
            } else {
                Regime::Hard
            }
        );
        assert_eq!(
            command.effective_gains,
            Gains {
                proportional: v[7] as u64,
                integral: v[8] as u64,
                derivative: v[9] as u64
            }
        );
        assert_eq!(command.unclipped_udrt, v[10]);
        assert_eq!(command.emission_udrt as i128, v[11]);
    }
}

#[test]
fn invalid_inputs_and_epoch_order_leave_state_unchanged() {
    let mut c = Controller::new(config()).unwrap();
    for observation in [
        Observation {
            epoch: 0,
            utilization_ppm: SCALE + 1,
            volatility_ppm: 0,
        },
        Observation {
            epoch: 1,
            utilization_ppm: 0,
            volatility_ppm: 0,
        },
    ] {
        let before = c.snapshot();
        assert!(c.step(observation).is_err());
        assert_eq!(c.snapshot(), before);
    }
    let observation = Observation {
        epoch: 0,
        utilization_ppm: 700_000,
        volatility_ppm: 0,
    };
    c.step(observation).unwrap();
    let before = c.snapshot();
    assert_eq!(c.step(observation), Err(Error::UnexpectedEpoch));
    assert_eq!(c.snapshot(), before);
}

#[test]
fn snapshot_validates_version_history_and_epoch() {
    let c = Controller::new(config()).unwrap();
    let mut bad = c.snapshot();
    bad.version += 1;
    assert_eq!(Controller::restore(bad), Err(Error::InvalidSnapshot));
    let mut bad = c.snapshot();
    bad.errors_ppm.push(0);
    assert_eq!(Controller::restore(bad), Err(Error::InvalidSnapshot));
    let mut bad = c.snapshot();
    bad.last_epoch = Some(0);
    bad.errors_ppm = vec![700_001];
    assert_eq!(Controller::restore(bad), Err(Error::InvalidSnapshot));
    let mut bad = c.snapshot();
    bad.last_epoch = Some(u64::MAX);
    assert_eq!(Controller::restore(bad), Err(Error::InvalidSnapshot));
    let mut near_end = c.snapshot();
    near_end.last_epoch = Some(u64::MAX - 1);
    near_end.errors_ppm = vec![0; 3];
    let mut c = Controller::restore(near_end).unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.step(Observation {
            epoch: u64::MAX,
            utilization_ppm: 0,
            volatility_ppm: 0
        }),
        Err(Error::EpochOverflow)
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn invalid_configuration_has_no_default_fallback() {
    let good = config();
    let mut bad = good.clone();
    bad.window_samples = 0;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.window_samples = MAX_WINDOW + 1;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.target_ppm = SCALE + 1;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.shock_threshold_ppm = 0;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.integral_min = 1;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.integral_max = -1;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.integral_min = -3_000_001;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.integral_max = 3_000_001;
    assert!(Controller::new(bad).is_err());
    let mut bad = good.clone();
    bad.base_udrt = bad.max_udrt + 1;
    assert!(Controller::new(bad).is_err());
    let mut bad = good;
    bad.base_udrt = bad.min_udrt - 1;
    assert!(Controller::new(bad).is_err());
}

#[test]
fn full_numeric_domain_preserves_bounds_without_overflow() {
    for target in [0, SCALE] {
        let mut cfg = config();
        cfg.target_ppm = target;
        cfg.window_samples = MAX_WINDOW;
        cfg.integral_min = -(MAX_WINDOW as i64 * SCALE as i64);
        cfg.integral_max = -cfg.integral_min;
        cfg.soft = Gains {
            proportional: u64::MAX,
            integral: u64::MAX,
            derivative: u64::MAX,
        };
        cfg.hard = cfg.soft;
        cfg.base_udrt = u64::MAX / 2;
        cfg.min_udrt = 1;
        cfg.max_udrt = u64::MAX;
        let error = target as i64 - (SCALE - target) as i64;
        let snapshot = Snapshot {
            version: VERSION,
            config: cfg,
            last_epoch: Some(MAX_WINDOW as u64 - 1),
            errors_ppm: vec![error; MAX_WINDOW],
        };
        let mut c = Controller::restore(snapshot).unwrap();
        let cmd = c
            .step(Observation {
                epoch: MAX_WINDOW as u64,
                utilization_ppm: SCALE - target,
                volatility_ppm: 0,
            })
            .unwrap();
        assert_eq!(cmd.emission_udrt, if target == 0 { 1 } else { u64::MAX });
        let cmd = c
            .step(Observation {
                epoch: MAX_WINDOW as u64 + 1,
                utilization_ppm: target,
                volatility_ppm: u64::MAX,
            })
            .unwrap();
        assert!((1..=u64::MAX).contains(&cmd.emission_udrt));
        assert_eq!(c.snapshot().errors_ppm.len(), MAX_WINDOW);
    }
}

#[test]
fn one_sample_window_keeps_previous_error_for_derivative() {
    let mut cfg = config();
    cfg.window_samples = 1;
    let mut c = Controller::new(cfg).unwrap();
    c.step(Observation {
        epoch: 0,
        utilization_ppm: 600_000,
        volatility_ppm: 0,
    })
    .unwrap();
    let cmd = c
        .step(Observation {
            epoch: 1,
            utilization_ppm: 800_000,
            volatility_ppm: 0,
        })
        .unwrap();
    assert_eq!(cmd.integral_ppm, -100_000);
    assert_eq!(cmd.derivative_ppm, -200_000);
    assert_eq!(c.snapshot().errors_ppm, vec![-100_000]);
}
