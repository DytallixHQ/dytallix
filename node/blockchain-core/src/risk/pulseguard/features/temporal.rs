use crate::risk::pulseguard::{FeatureVector, RiskEvent};
use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

pub struct TemporalWindowEngine {
    one_min: VecDeque<RiskEvent>,
    five_min: VecDeque<RiskEvent>,
    sixty_min: VecDeque<RiskEvent>,
}

impl Default for TemporalWindowEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemporalWindowEngine {
    pub fn new() -> Self {
        Self {
            one_min: VecDeque::new(),
            five_min: VecDeque::new(),
            sixty_min: VecDeque::new(),
        }
    }
    pub fn ingest(&mut self, ev: RiskEvent) -> FeatureVector {
        self.one_min.push_back(ev.clone());
        self.five_min.push_back(ev.clone());
        self.sixty_min.push_back(ev.clone());
        let now = SystemTime::now();
        let trim = |dq: &mut VecDeque<RiskEvent>, window: Duration| {
            while let Some(front) = dq.front() {
                let age =
                    now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - front.timestamp;
                if age > window.as_secs() {
                    dq.pop_front();
                } else {
                    break;
                }
            }
        };
        trim(&mut self.one_min, Duration::from_secs(60));
        trim(&mut self.five_min, Duration::from_secs(300));
        trim(&mut self.sixty_min, Duration::from_secs(3600));
        let v1 = self.one_min.len() as f32;
        let v5 = self.five_min.len() as f32;
        let v60 = self.sixty_min.len() as f32;
        FeatureVector::new(
            ev.tx_hash,
            vec![
                "velocity_1m".into(),
                "velocity_5m".into(),
                "velocity_60m".into(),
            ],
            vec![v1, v5, v60],
        )
    }
}
