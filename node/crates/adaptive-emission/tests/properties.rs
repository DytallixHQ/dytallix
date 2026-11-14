//! Randomized property tests over the invariants of
//! node/docs/mainnet/adaptive-emission-proof.md section 1 (P01_E06_NO_INDEPENDENT_REVIEW_2026-11-13:
//! "randomized property tests cover the invariants"). A fixed-seed generator keeps every run
//! reproducible and adds no dependency; each failure names its seed and case. Debug test builds
//! check integer overflow, so an overflow anywhere in `step` fails the run.

use dytallix_adaptive_emission::*;

const SEEDS: [u64; 4] = [1, 0x5eed_0f_e06, 0xd17a_111c, u64::MAX / 3];

/// SplitMix64: small, deterministic, enough for test-case generation.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [lo, hi], with the ends and zero drawn often: the proof's bounds live there.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        match self.next() % 8 {
            0 => lo,
            1 => hi,
            _ if hi - lo == u64::MAX => self.next(),
            _ => lo + self.next() % (hi - lo + 1),
        }
    }

    fn signed(&mut self, lo: i64, hi: i64) -> i64 {
        (lo as i128 + self.range(0, (hi as i128 - lo as i128) as u64) as i128) as i64
    }

    fn gains(&mut self) -> Gains {
        Gains {
            proportional: self.range(0, u64::MAX),
            integral: self.range(0, u64::MAX),
            derivative: self.range(0, u64::MAX),
        }
    }
}

/// A configuration `Config::validate` accepts. Small windows mostly, the full one sometimes.
fn config(rng: &mut Rng) -> Config {
    let window_samples = match rng.next() % 10 {
        0 => MAX_WINDOW,
        1 => 1,
        _ => rng.range(1, 64) as usize,
    };
    let bound = window_samples as i64 * SCALE as i64;
    let mut money = [
        rng.range(0, u64::MAX),
        rng.range(0, u64::MAX),
        rng.range(0, u64::MAX),
    ];
    money.sort_unstable();
    let config = Config {
        target_ppm: rng.range(0, SCALE),
        shock_threshold_ppm: rng.range(1, SCALE),
        volatility_threshold_ppm: rng.range(0, u64::MAX),
        window_samples,
        integral_min: rng.signed(-bound, 0),
        integral_max: rng.signed(0, bound),
        soft: rng.gains(),
        hard: rng.gains(),
        base_udrt: money[1],
        min_udrt: money[0],
        max_udrt: money[2],
    };
    config
        .validate()
        .expect("the generator makes only accepted configurations");
    config
}

fn observation(rng: &mut Rng, epoch: u64) -> Observation {
    Observation {
        epoch,
        utilization_ppm: rng.range(0, SCALE),
        volatility_ppm: rng.range(0, u64::MAX),
    }
}

fn steps_for(config: &Config) -> u64 {
    // The full window costs a copy of up to 65,536 errors a step: fewer steps there.
    if config.window_samples == MAX_WINDOW {
        40
    } else {
        3 * config.window_samples as u64 + 20
    }
}

#[test]
fn every_accepted_step_keeps_the_arithmetic_and_state_invariants() {
    for seed in SEEDS {
        let mut rng = Rng(seed);
        for case in 0..120 {
            let config = config(&mut rng);
            let mut controller = Controller::new(config.clone()).unwrap();
            let mut previous = 0i64;
            for epoch in 0..steps_for(&config) {
                let o = observation(&mut rng, epoch);
                let c = controller
                    .step(o)
                    .unwrap_or_else(|e| panic!("seed {seed} case {case} epoch {epoch}: {e}"));
                let at = format!("seed {seed} case {case} epoch {epoch}");
                // Each error is target minus utilization: at most SCALE in magnitude.
                assert_eq!(
                    c.error_ppm,
                    config.target_ppm as i64 - o.utilization_ppm as i64,
                    "{at}"
                );
                assert!(c.error_ppm.unsigned_abs() <= SCALE, "{at}");
                // The derivative is against the previous accepted error (zero at startup): at most 2*SCALE.
                assert_eq!(c.derivative_ppm, c.error_ppm - previous, "{at}");
                assert!(c.derivative_ppm.unsigned_abs() <= 2 * SCALE, "{at}");
                // The integral is clamped into the configured band.
                assert!(
                    config.integral_min <= c.integral_ppm && c.integral_ppm <= config.integral_max,
                    "{at}"
                );
                // Damping never raises a gain.
                let raw = match c.regime {
                    Regime::Soft => config.soft,
                    Regime::Hard => config.hard,
                };
                assert!(c.effective_gains.proportional <= raw.proportional, "{at}");
                assert!(c.effective_gains.integral <= raw.integral, "{at}");
                assert!(c.effective_gains.derivative <= raw.derivative, "{at}");
                // The regime follows the shock threshold.
                assert_eq!(
                    c.regime == Regime::Soft,
                    c.error_ppm.unsigned_abs() < config.shock_threshold_ppm,
                    "{at}"
                );
                // The command is the clamp of the unclipped value: E_min <= E <= E_max.
                assert!(
                    config.min_udrt <= c.emission_udrt && c.emission_udrt <= config.max_udrt,
                    "{at}"
                );
                assert_eq!(
                    c.emission_udrt as i128,
                    c.unclipped_udrt
                        .clamp(config.min_udrt as i128, config.max_udrt as i128),
                    "{at}"
                );
                assert_eq!(c.for_epoch, epoch + 1, "{at}");
                // The window keeps min(accepted steps, W) errors, oldest first, the newest last.
                let snapshot = controller.snapshot();
                assert_eq!(
                    snapshot.errors_ppm.len() as u64,
                    (epoch + 1).min(config.window_samples as u64),
                    "{at}"
                );
                assert_eq!(snapshot.errors_ppm.last(), Some(&c.error_ppm), "{at}");
                assert_eq!(snapshot.last_epoch, Some(epoch), "{at}");
                previous = c.error_ppm;
            }
        }
    }
}

#[test]
fn a_rejected_input_leaves_the_state_unchanged() {
    for seed in SEEDS {
        let mut rng = Rng(seed ^ 0xbad);
        for case in 0..200 {
            let config = config(&mut rng);
            let mut controller = Controller::new(config.clone()).unwrap();
            let accepted = rng.range(0, 8);
            for epoch in 0..accepted {
                controller.step(observation(&mut rng, epoch)).unwrap();
            }
            let before = controller.snapshot();
            let mut bad = observation(&mut rng, accepted);
            let expected = match rng.next() % 3 {
                0 => {
                    bad.utilization_ppm = rng.range(SCALE + 1, u64::MAX);
                    Error::InvalidInput
                }
                1 => {
                    bad.epoch = accepted + rng.range(1, u64::MAX - accepted);
                    Error::UnexpectedEpoch
                }
                _ => {
                    bad.epoch = accepted.checked_sub(1).unwrap_or(u64::MAX);
                    Error::UnexpectedEpoch
                }
            };
            assert_eq!(
                controller.step(bad),
                Err(expected),
                "seed {seed} case {case}"
            );
            assert_eq!(
                controller.snapshot(),
                before,
                "seed {seed} case {case}: a rejection changed the state"
            );
        }
    }
}

#[test]
fn identical_state_and_input_give_identical_output_and_a_restored_snapshot_continues_identically() {
    for seed in SEEDS {
        let mut rng = Rng(seed ^ 0x5a5a);
        for case in 0..80 {
            let config = config(&mut rng);
            let steps = steps_for(&config);
            let inputs: Vec<Observation> = (0..steps).map(|e| observation(&mut rng, e)).collect();
            let cut = rng.range(0, steps) as usize;
            let mut a = Controller::new(config.clone()).unwrap();
            let mut b = Controller::new(config.clone()).unwrap();
            let mut outputs = Vec::new();
            for o in &inputs {
                let x = a.step(*o).unwrap();
                assert_eq!(b.step(*o).unwrap(), x, "seed {seed} case {case}");
                outputs.push(x);
            }
            let mut c = Controller::new(config.clone()).unwrap();
            for o in &inputs[..cut] {
                c.step(*o).unwrap();
            }
            let mut c = Controller::restore(c.snapshot()).unwrap();
            for (o, x) in inputs[cut..].iter().zip(&outputs[cut..]) {
                assert_eq!(
                    &c.step(*o).unwrap(),
                    x,
                    "seed {seed} case {case}: restore at {cut} diverged"
                );
            }
            assert_eq!(c.snapshot(), a.snapshot(), "seed {seed} case {case}");
        }
    }
}

#[test]
fn the_next_epoch_must_be_representable() {
    let mut rng = Rng(7);
    for case in 0..50 {
        let mut config = config(&mut rng);
        config.window_samples = (config.window_samples % 64).max(1);
        let bound = config.window_samples as i64 * SCALE as i64;
        config.integral_min = config.integral_min.max(-bound);
        config.integral_max = config.integral_max.min(bound);
        let target = config.target_ppm as i64;
        let errors: Vec<i64> = (0..config.window_samples)
            .map(|_| rng.signed(target - SCALE as i64, target))
            .collect();
        let snapshot = Snapshot {
            version: VERSION,
            config: config.clone(),
            last_epoch: Some(u64::MAX - 1),
            errors_ppm: errors,
        };
        let mut controller = Controller::restore(snapshot.clone()).unwrap();
        let o = Observation {
            epoch: u64::MAX,
            utilization_ppm: rng.range(0, SCALE),
            volatility_ppm: 0,
        };
        assert_eq!(controller.step(o), Err(Error::EpochOverflow), "case {case}");
        assert_eq!(
            controller.snapshot(),
            snapshot,
            "case {case}: the overflow changed the state"
        );
    }
}
