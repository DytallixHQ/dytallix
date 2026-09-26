use crate::{Config, Controller, Error, Gains, Snapshot, MAX_WINDOW, VERSION};
use alloc::vec::Vec;

const MAGIC: &[u8; 8] = b"DYTAEC01";
pub const SNAPSHOT_HEADER_LEN: usize = 139;
pub const MAX_ENCODED_LEN: usize = SNAPSHOT_HEADER_LEN + 8 * MAX_WINDOW;

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let bytes = self.0.get(..N).ok_or(Error::InvalidEncoding)?;
        let mut value = [0; N];
        value.copy_from_slice(bytes);
        self.0 = &self.0[N..];
        Ok(value)
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_be_bytes(self.take()?))
    }
    fn gains(&mut self) -> Result<Gains, Error> {
        Ok(Gains {
            proportional: self.u64()?,
            integral: self.u64()?,
            derivative: self.u64()?,
        })
    }
}

impl Controller {
    /// Canonical version-1 encoding. All integers use fixed-width big endian.
    pub fn encode(&self) -> Vec<u8> {
        let snapshot = self.snapshot();
        let c = &snapshot.config;
        let mut out = Vec::with_capacity(SNAPSHOT_HEADER_LEN + 8 * snapshot.errors_ppm.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_be_bytes());
        for value in [
            c.target_ppm,
            c.shock_threshold_ppm,
            c.volatility_threshold_ppm,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        // Config validation guarantees that the sample count fits u32.
        out.extend_from_slice(&(c.window_samples as u32).to_be_bytes());
        out.extend_from_slice(&c.integral_min.to_be_bytes());
        out.extend_from_slice(&c.integral_max.to_be_bytes());
        for gain in [c.soft, c.hard] {
            for value in [gain.proportional, gain.integral, gain.derivative] {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        for value in [c.base_udrt, c.min_udrt, c.max_udrt] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.push(u8::from(snapshot.last_epoch.is_some()));
        out.extend_from_slice(&snapshot.last_epoch.unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&(snapshot.errors_ppm.len() as u32).to_be_bytes());
        for value in snapshot.errors_ppm {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    /// Rejects unsupported versions, noncanonical tags, trailing bytes, invalid
    /// configuration, and invalid state. No unchecked allocation uses input counts.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if !(SNAPSHOT_HEADER_LEN..=MAX_ENCODED_LEN).contains(&bytes.len()) {
            return Err(Error::InvalidEncoding);
        }
        let mut r = Reader(bytes);
        if &r.take::<8>()? != MAGIC || u16::from_be_bytes(r.take()?) != VERSION {
            return Err(Error::InvalidEncoding);
        }
        let config = Config {
            target_ppm: r.u64()?,
            shock_threshold_ppm: r.u64()?,
            volatility_threshold_ppm: r.u64()?,
            window_samples: u32::from_be_bytes(r.take()?) as usize,
            integral_min: r.i64()?,
            integral_max: r.i64()?,
            soft: r.gains()?,
            hard: r.gains()?,
            base_udrt: r.u64()?,
            min_udrt: r.u64()?,
            max_udrt: r.u64()?,
        };
        config.validate()?;
        let tag = r.take::<1>()?[0];
        let epoch = r.u64()?;
        let last_epoch = match (tag, epoch) {
            (0, 0) => None,
            (1, epoch) => Some(epoch),
            _ => return Err(Error::InvalidEncoding),
        };
        let count = u32::from_be_bytes(r.take()?) as usize;
        if count > config.window_samples || r.0.len() != count * 8 {
            return Err(Error::InvalidEncoding);
        }
        let mut errors_ppm = Vec::with_capacity(count);
        for _ in 0..count {
            errors_ppm.push(r.i64()?);
        }
        Self::restore(Snapshot {
            version: VERSION,
            config,
            last_epoch,
            errors_ppm,
        })
    }
}
