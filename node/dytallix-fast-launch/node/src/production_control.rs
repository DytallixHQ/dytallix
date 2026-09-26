//! Process-local producer lifecycle. A failed producer requires recovery and restart.
use std::sync::atomic::{AtomicU8, Ordering};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionState {
    Running,
    Paused,
    Failed,
}
impl ProductionState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Failed => "failed",
        }
    }
    pub fn health(self) -> &'static str {
        if self == Self::Failed {
            "failed"
        } else {
            "healthy"
        }
    }
}
pub struct ProductionControl(AtomicU8);
impl ProductionControl {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }
    pub fn state(&self) -> ProductionState {
        match self.0.load(Ordering::Acquire) {
            0 => ProductionState::Running,
            1 => ProductionState::Paused,
            _ => ProductionState::Failed,
        }
    }
    pub fn fail(&self) {
        self.0.store(2, Ordering::Release);
    }
    /// Failure is terminal for this process. Pause/resume cannot overwrite it.
    pub fn set_paused(&self, paused: bool) -> Result<(), ProductionState> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                if value == 2 {
                    None
                } else {
                    Some(u8::from(paused))
                }
            })
            .map(|_| ())
            .map_err(|_| ProductionState::Failed)
    }
}
impl Default for ProductionControl {
    fn default() -> Self {
        Self::new()
    }
}
pub static PRODUCTION: ProductionControl = ProductionControl::new();
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_is_terminal_and_visible_after_pause_and_resume() {
        let control = ProductionControl::new();
        assert_eq!(control.state(), ProductionState::Running);
        control.set_paused(true).unwrap();
        assert_eq!(control.state(), ProductionState::Paused);
        control.set_paused(false).unwrap();
        assert_eq!(control.state(), ProductionState::Running);
        control.fail();
        assert!(control.set_paused(false).is_err());
        assert!(control.set_paused(true).is_err());
        assert_eq!(control.state().health(), "failed");
        assert_eq!(control.state().name(), "failed");
    }
    #[test]
    fn concurrent_controls_cannot_clear_failure() {
        let control = std::sync::Arc::new(ProductionControl::new());
        let other = control.clone();
        let worker = std::thread::spawn(move || {
            for i in 0..1000 {
                let _ = other.set_paused(i % 2 == 0);
            }
        });
        control.fail();
        worker.join().unwrap();
        assert_eq!(control.state(), ProductionState::Failed);
    }
}
