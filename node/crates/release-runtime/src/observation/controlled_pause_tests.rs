use super::*;
use std::{cell::Cell, collections::VecDeque};
struct Fake {
    admission: Admission,
    time: Cell<Duration>,
    cancel: Option<Phase>,
    fail: Option<&'static str>,
    events: VecDeque<Event>,
    calls: Vec<&'static str>,
    observation_cost: Duration,
    idle_cost: Duration,
}
impl Fake {
    fn good() -> Self {
        Self {
            admission: Admission {
                parent_matches: true,
                role_matches: true,
                member_matches: true,
                executable: true,
                digest_matches: true,
                traced: false,
                stopped: false,
            },
            time: Cell::new(Duration::ZERO),
            cancel: None,
            fail: None,
            events: [Event::Stopped, Event::None, Event::Continued].into(),
            calls: vec![],
            observation_cost: Duration::from_millis(2),
            idle_cost: Duration::from_millis(1),
        }
    }
    fn call(&mut self, name: &'static str) -> Result<()> {
        self.calls.push(name);
        ensure!(self.fail != Some(name), "injected {name}");
        Ok(())
    }
}
impl Backend for Fake {
    type Observation = &'static str;
    fn elapsed(&self) -> Duration {
        self.time.get()
    }
    fn cancelled(&mut self, phase: Phase) -> bool {
        self.cancel == Some(phase)
    }
    fn reject_terminal(&mut self) -> Result<()> {
        ensure!(self.events.front() != Some(&Event::Exited),
            "Owned child exited before cancellation classification");
        Ok(())
    }
    fn admission(&mut self) -> Result<()> {
        self.call("admission")?;
        validate_admission(self.admission)
    }
    fn stop(&mut self) -> Result<()> {
        self.call("stop")
    }
    fn event(&mut self) -> Result<Event> {
        self.call("event")?;
        Ok(self.events.pop_front().unwrap_or(Event::None))
    }
    fn idle(&mut self, _: Duration) {
        self.time.set(self.time.get() + self.idle_cost);
    }
    fn observe(&mut self, _: Duration) -> Result<Self::Observation> {
        if self.fail == Some("panic_observe") {
            panic!("injected owner unwind");
        }
        self.call("observe")?;
        self.time.set(self.time.get() + self.observation_cost);
        Ok("exact snapshot")
    }
    fn stopped(&mut self) -> Result<()> {
        self.call("stopped")
    }
    fn resume(&mut self) -> Result<()> {
        self.call("resume")
    }
    fn running(&mut self) -> Result<()> {
        self.call("running")
    }
    fn cleanup(&mut self, total: Duration) -> Result<()> {
        self.call("cleanup")?;
        ensure!(self.time.get() < total, "cleanup deadline");
        Ok(())
    }
}
fn budget() -> Budget {
    Budget {
        total: Duration::from_millis(10),
        cleanup_reserve: Duration::from_millis(2),
    }
}
#[test]
fn completed_stop_observation_and_continuation_succeed_once() {
    let mut b = Fake::good();
    let (s, t) = execute(&mut b, budget()).unwrap();
    assert_eq!(s, "exact snapshot");
    assert_eq!(t.observation_micros, 2000);
    assert_eq!(b.calls.iter().filter(|&&x| x == "observe").count(), 1);
    assert!(!b.calls.contains(&"cleanup"));
}
#[test]
fn admission_pre_stopped_traced_peer_and_role_refusals_never_signal() {
    for predicate in 0..7 {
        let mut b = Fake::good();
        match predicate {
            0 => b.admission.stopped = true,
            1 => b.admission.traced = true,
            2 => b.admission.parent_matches = false,
            3 => b.admission.role_matches = false,
            4 => b.admission.member_matches = false,
            5 => b.admission.executable = false,
            _ => b.admission.digest_matches = false,
        };
        assert!(execute(&mut b, budget()).is_err(), "predicate {predicate}");
        assert_eq!(b.calls, vec!["admission"]);
    }
}
#[test]
fn every_wait_state_rejects_exit_trace_and_unexpected_continuation() {
    for (events, label) in [
        (vec![Event::Exited], "exit before stop"),
        (vec![Event::Traced], "traced stop"),
        (vec![Event::Continued], "continued before stop"),
        (
            vec![Event::Stopped, Event::Continued],
            "continued during observation",
        ),
        (
            vec![Event::Stopped, Event::Exited],
            "exit during observation",
        ),
        (
            vec![Event::Stopped, Event::None, Event::Stopped],
            "stopped during continuation",
        ),
        (
            vec![Event::Stopped, Event::None, Event::Exited],
            "exit at continuation",
        ),
    ] {
        let mut b = Fake::good();
        b.events = events.into();
        assert!(execute(&mut b, budget()).is_err(), "{label}");
        assert_eq!(b.calls.iter().filter(|&&x| x == "cleanup").count(), 1);
    }
}
#[test]
fn cancellation_at_each_phase_rejects_and_cleans_only_after_stop_request() {
    for phase in [
        Phase::Admission,
        Phase::StopRequest,
        Phase::StopWait,
        Phase::Observation,
        Phase::StoppedCheck,
        Phase::ContinueRequest,
        Phase::ContinueWait,
        Phase::RunningCheck,
        Phase::Complete,
    ] {
        let mut b = Fake::good();
        b.cancel = Some(phase);
        let e = execute(&mut b, budget()).unwrap_err().to_string();
        assert!(e.contains("cancelled") || e.contains("refused"));
        assert_eq!(
            b.calls.contains(&"cleanup"),
            !matches!(phase, Phase::Admission | Phase::StopRequest)
        );
    }
}
#[test]
fn mapping_mismatch_has_one_observation_and_never_returns_snapshot() {
    let mut b = Fake::good();
    b.fail = Some("observe");
    let e = execute(&mut b, budget()).unwrap_err();
    assert!(format!("{e:#}").contains("injected observe"));
    assert_eq!(b.calls.iter().filter(|&&x| x == "observe").count(), 1);
    assert!(b.calls.contains(&"cleanup"));
    assert!(!b.calls.contains(&"resume"));
}
#[test]
fn owner_stop_release_and_final_check_failures_have_fatal_cleanup() {
    for name in ["stop", "event", "stopped", "resume", "running"] {
        let mut b = Fake::good();
        b.fail = Some(name);
        assert!(execute(&mut b, budget()).is_err());
        assert_eq!(b.calls.last(), Some(&"cleanup"));
    }
}
#[test]
fn cleanup_failure_has_priority_and_preserves_primary_error() {
    let mut b = Fake::good();
    b.fail = Some("cleanup");
    b.events = [Event::Continued].into();
    let e = format!("{:#}", execute(&mut b, budget()).unwrap_err());
    assert!(e.contains("cleanup failure"));
    assert!(e.contains("Unexpected owned-child event"));
}
#[test]
fn cancellation_classification_requires_successful_cleanup() {
    let mut before_stop = Fake::good();
    before_stop.cancel = Some(Phase::Admission);
    let error = execute(&mut before_stop, budget()).unwrap_err();
    assert!(is_clean_cancellation(&error));
    assert!(!before_stop.calls.contains(&"cleanup"));

    let mut after_stop = Fake::good();
    after_stop.cancel = Some(Phase::Observation);
    let error = execute(&mut after_stop, budget()).unwrap_err();
    assert!(is_clean_cancellation(&error));
    assert!(after_stop.calls.contains(&"cleanup"));

    let mut cleanup_failed = Fake::good();
    cleanup_failed.cancel = Some(Phase::Observation);
    cleanup_failed.fail = Some("cleanup");
    let error = execute(&mut cleanup_failed, budget()).unwrap_err();
    assert!(error.is::<PauseCancelled>());
    assert!(error.is::<PauseCleanupFailure>());
    assert!(!is_clean_cancellation(&error));
}
#[test]
fn observed_child_exit_takes_priority_over_cancellation() {
    let mut before_stop = Fake::good();
    before_stop.cancel = Some(Phase::Admission);
    before_stop.events = [Event::Exited].into();
    let error = execute(&mut before_stop, budget()).unwrap_err();
    assert!(!is_clean_cancellation(&error));
    assert!(error.to_string().contains("exited before cancellation"));
    assert!(!before_stop.calls.contains(&"stop"));

    let mut after_stop = Fake::good();
    after_stop.cancel = Some(Phase::Observation);
    after_stop.events = [Event::Stopped, Event::Exited].into();
    let error = execute(&mut after_stop, budget()).unwrap_err();
    assert!(!is_clean_cancellation(&error));
    assert!(after_stop.calls.contains(&"cleanup"));
    assert!(!after_stop.calls.contains(&"resume"));
}
#[test]
fn one_deadline_bounds_wait_observation_and_cleanup_without_retry() {
    let mut b = Fake::good();
    b.events.clear();
    assert!(execute(&mut b, budget()).is_err());
    assert!(!b.calls.contains(&"observe"));
    assert!(b.calls.contains(&"cleanup"));
    let mut b = Fake::good();
    b.observation_cost = Duration::from_millis(8);
    assert!(execute(&mut b, budget()).is_err());
    assert!(b.calls.contains(&"cleanup"));
    assert!(!b.calls.contains(&"resume"));
    let mut b = Fake::good();
    b.observation_cost = Duration::from_millis(11);
    let e = format!("{:#}", execute(&mut b, budget()).unwrap_err());
    assert!(e.contains("cleanup failure"));
    assert!(e.contains("deadline"));
}
#[test]
fn invalid_total_and_cleanup_reserve_never_admit_child() {
    for reserve in [
        Duration::ZERO,
        Duration::from_millis(10),
        Duration::from_millis(11),
    ] {
        let mut b = Fake::good();
        assert!(execute(
            &mut b,
            Budget {
                total: Duration::from_millis(10),
                cleanup_reserve: reserve
            }
        )
        .is_err());
        assert!(b.calls.is_empty());
    }
}

#[test]
fn overflowing_total_budget_never_admits_or_signals() {
    let mut b = Fake::good();
    assert!(execute(
        &mut b,
        Budget {
            total: Duration::MAX,
            cleanup_reserve: Duration::from_secs(1)
        }
    )
    .is_err());
    assert!(b.calls.is_empty());
}

#[test]
fn owner_unwind_runs_one_terminal_cleanup_and_never_continues() {
    let mut b = Fake::good();
    b.fail = Some("panic_observe");
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(&mut b, budget())));
    assert!(result.is_err());
    assert_eq!(b.calls.iter().filter(|&&x| x == "cleanup").count(), 1);
    assert!(!b.calls.contains(&"resume"));
}
#[test]
fn direct_armed_guard_drop_has_one_terminal_cleanup() {
    let mut b = Fake::good();
    {
        let _guard = CleanupGuard {
            backend: &mut b,
            total: budget().total,
            armed: true,
        };
    }
    assert_eq!(b.calls, vec!["cleanup"]);
}
#[test]
fn owner_unwind_and_cleanup_failure_never_continue_or_succeed() {
    let mut b = Fake::good();
    b.fail = Some("cleanup");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = CleanupGuard {
            backend: &mut b,
            total: budget().total,
            armed: true,
        };
        panic!("injected owner failure");
    }));
    assert!(result.is_err());
    assert_eq!(b.calls, vec!["cleanup"]);
}
