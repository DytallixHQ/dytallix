//! Inactive reference controller for the corrected adaptive-emission specification.
//! This module proposes issuance. It does not mint tokens or prove plant stability.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

mod encoding;
pub use encoding::{MAX_ENCODED_LEN, SNAPSHOT_HEADER_LEN};

pub const VERSION: u16 = 1;
/// Dimensionless observations use parts per million.
pub const SCALE: u64 = 1_000_000;
pub const MAX_WINDOW: usize = 65_536;

/// Each coefficient has units of uDRT per unit of dimensionless error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gains {
    pub proportional: u64,
    pub integral: u64,
    pub derivative: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub target_ppm: u64,
    pub shock_threshold_ppm: u64,
    pub volatility_threshold_ppm: u64,
    /// Number of samples, including the current error. No duration is implied.
    pub window_samples: usize,
    /// Signed sums of errors in parts per million.
    pub integral_min: i64,
    pub integral_max: i64,
    pub soft: Gains,
    pub hard: Gains,
    pub base_udrt: u64,
    pub min_udrt: u64,
    pub max_udrt: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfig,
    InvalidInput,
    InvalidSnapshot,
    UnexpectedEpoch,
    EpochOverflow,
    InvalidEncoding,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "adaptive emission: {self:?}")
    }
}

impl core::error::Error for Error {}

impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        if self.target_ppm > SCALE
            || self.shock_threshold_ppm == 0
            || self.shock_threshold_ppm > SCALE
            || self.window_samples == 0
            || self.window_samples > MAX_WINDOW
            || self.min_udrt > self.base_udrt
            || self.base_udrt > self.max_udrt
        {
            return Err(Error::InvalidConfig);
        }
        let bound = self.window_samples as i64 * SCALE as i64;
        if self.integral_min < -bound
            || self.integral_min > 0
            || self.integral_max < 0
            || self.integral_max > bound
        {
            return Err(Error::InvalidConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    /// First epoch is zero. Later epochs must be consecutive.
    pub epoch: u64,
    pub utilization_ppm: u64,
    /// Must come from the future consensus input contract, not a local clock/feed.
    pub volatility_ppm: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Regime {
    Soft,
    Hard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub for_epoch: u64,
    pub emission_udrt: u64,
    pub unclipped_udrt: i128,
    pub error_ppm: i64,
    pub integral_ppm: i64,
    pub derivative_ppm: i64,
    pub regime: Regime,
    pub effective_gains: Gains,
}

/// Logical checkpoint. This is not a consensus wire encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub version: u16,
    pub config: Config,
    pub last_epoch: Option<u64>,
    /// Oldest first. Startup has no synthetic history samples.
    pub errors_ppm: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controller {
    config: Config,
    last_epoch: Option<u64>,
    errors: VecDeque<i64>,
}

impl Controller {
    pub fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        Ok(Self {
            config,
            last_epoch: None,
            errors: VecDeque::new(),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: VERSION,
            config: self.config.clone(),
            last_epoch: self.last_epoch,
            errors_ppm: self.errors.iter().copied().collect(),
        }
    }

    pub fn restore(snapshot: Snapshot) -> Result<Self, Error> {
        snapshot.config.validate()?;
        let count = match snapshot.last_epoch {
            None => 0,
            Some(epoch) => epoch.checked_add(1).ok_or(Error::InvalidSnapshot)?,
        };
        let expected = count.min(snapshot.config.window_samples as u64) as usize;
        let target = snapshot.config.target_ppm as i64;
        if snapshot.version != VERSION
            || snapshot.errors_ppm.len() != expected
            || snapshot
                .errors_ppm
                .iter()
                .any(|e| *e < target - SCALE as i64 || *e > target)
        {
            return Err(Error::InvalidSnapshot);
        }
        Ok(Self {
            config: snapshot.config,
            last_epoch: snapshot.last_epoch,
            errors: snapshot.errors_ppm.into(),
        })
    }

    /// Rejection leaves state unchanged. Callers must persist accepted state and
    /// eventual accounting in one transaction before accepting the next epoch.
    pub fn step(&mut self, observation: Observation) -> Result<Command, Error> {
        if observation.utilization_ppm > SCALE {
            return Err(Error::InvalidInput);
        }
        let expected = match self.last_epoch {
            None => 0,
            Some(epoch) => epoch.checked_add(1).ok_or(Error::EpochOverflow)?,
        };
        if observation.epoch != expected {
            return Err(Error::UnexpectedEpoch);
        }
        let for_epoch = observation
            .epoch
            .checked_add(1)
            .ok_or(Error::EpochOverflow)?;
        let error = self.config.target_ppm as i64 - observation.utilization_ppm as i64;
        let previous = self.errors.back().copied().unwrap_or(0);
        let mut next_errors = self.errors.clone();
        if next_errors.len() == self.config.window_samples {
            next_errors.pop_front();
        }
        next_errors.push_back(error);
        let integral = next_errors
            .iter()
            .sum::<i64>()
            .clamp(self.config.integral_min, self.config.integral_max);
        let derivative = error - previous;
        let regime = if error.unsigned_abs() < self.config.shock_threshold_ppm {
            Regime::Soft
        } else {
            Regime::Hard
        };
        let gains = match regime {
            Regime::Soft => self.config.soft,
            Regime::Hard => self.config.hard,
        };
        let damp = |gain: u64| -> u64 {
            if observation.volatility_ppm > self.config.volatility_threshold_ppm {
                (gain as u128 * SCALE as u128
                    / (SCALE as u128 + observation.volatility_ppm as u128)) as u64
            } else {
                gain
            }
        };
        let effective_gains = Gains {
            proportional: damp(gains.proportional),
            integral: damp(gains.integral),
            derivative: damp(gains.derivative),
        };
        // Signed division truncates each contribution toward zero.
        let term = |gain: u64, value: i64| gain as i128 * value as i128 / SCALE as i128;
        let unclipped_udrt = self.config.base_udrt as i128
            + term(effective_gains.proportional, error)
            + term(effective_gains.integral, integral)
            + term(effective_gains.derivative, derivative);
        let emission_udrt =
            unclipped_udrt.clamp(self.config.min_udrt as i128, self.config.max_udrt as i128) as u64;
        self.errors = next_errors;
        self.last_epoch = Some(observation.epoch);
        Ok(Command {
            for_epoch,
            emission_udrt,
            unclipped_udrt,
            error_ppm: error,
            integral_ppm: integral,
            derivative_ppm: derivative,
            regime,
            effective_gains,
        })
    }
}
