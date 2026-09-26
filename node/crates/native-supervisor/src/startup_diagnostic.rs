//! Fixed-field failure evidence. Never format the source error or configuration.
use crate::processes::{OperationCancelled, Role};
use anyhow::Result;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Start, Spawn, GuardPrepare, GuardReady, CaptureIdentity, SecurityBefore,
    ObserveInitial, SecurityAfter, GuardRelease, EndpointAbsent, EndpointReady,
    ControlledObservation, EngineReadiness, AdapterReadiness, IpcConnect, IpcPeer, IpcFrame,
    StatusRpc, StatusValidation, ApplicationRpc, ApplicationValidation, ApplicationComparison,
    ReadinessDeadline, AdapterListener, AdapterHttp, AdapterIdentity,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Start => "start", Self::Spawn => "spawn", Self::GuardPrepare => "guard_prepare",
            Self::GuardReady => "guard_ready", Self::CaptureIdentity => "capture_identity",
            Self::SecurityBefore => "security_before", Self::ObserveInitial => "observe_initial",
            Self::SecurityAfter => "security_after", Self::GuardRelease => "guard_release",
            Self::EndpointAbsent => "endpoint_absent", Self::EndpointReady => "endpoint_ready",
            Self::ControlledObservation => "controlled_observation",
            Self::EngineReadiness=>"engine_readiness", Self::AdapterReadiness=>"adapter_readiness",
            Self::IpcConnect=>"ipc_connect", Self::IpcPeer=>"ipc_peer", Self::IpcFrame=>"ipc_frame",
            Self::StatusRpc=>"status_rpc",Self::StatusValidation=>"status_validation",
            Self::ApplicationRpc=>"application_rpc",Self::ApplicationValidation=>"application_validation",
            Self::ApplicationComparison=>"application_comparison",Self::ReadinessDeadline=>"readiness_deadline",
            Self::AdapterListener=>"adapter_listener",Self::AdapterHttp=>"adapter_http",Self::AdapterIdentity=>"adapter_identity",
        }
    }
}
#[derive(Debug)]
pub struct StartupPhase { role: Role, stage: Stage }
impl std::fmt::Display for StartupPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Startup {} {}", self.role.as_str(), self.stage.name())
    }
}
/// Preserve the earliest specific stage and the original typed error.
pub fn phase<T>(result: Result<T>, role: Role, stage: Stage) -> Result<T> {
    result.map_err(|error| {
        if error.is::<StartupPhase>() { error } else { let stage=if error.is::<ReadinessExpired>() { Stage::ReadinessDeadline } else {stage}; error.context(StartupPhase { role, stage }) }
    })
}
#[derive(Clone, Copy, Debug)]
pub struct ExitObservation {
    pub observed_pid: Option<i32>, pub si_code: Option<i32>, pub si_status: Option<i32>, pub observation_errno: Option<i32>,
}
#[derive(Debug)]
pub struct OwnedChildExit { pub role: Role, pub pid: u32, pub observation: ExitObservation }
impl std::fmt::Display for OwnedChildExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Owned child exit observed") }
}
impl std::error::Error for OwnedChildExit {}
#[derive(Clone, Copy, Debug)]
pub enum EndpointViolation { InodeChanged, Disappeared, TypeOrOwner, NotAbsolute, ParentMissing, ParentAlias, ParentOwnershipOrMode }
impl EndpointViolation {
    pub fn name(self) -> &'static str { match self {
        Self::InodeChanged=>"INODE_CHANGED", Self::Disappeared=>"DISAPPEARED", Self::TypeOrOwner=>"TYPE_OR_OWNER",
        Self::NotAbsolute=>"NOT_ABSOLUTE", Self::ParentMissing=>"PARENT_MISSING", Self::ParentAlias=>"PARENT_ALIAS",
        Self::ParentOwnershipOrMode=>"PARENT_OWNERSHIP_OR_MODE",
    } }
}
impl std::fmt::Display for EndpointViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.name()) }
}
impl std::error::Error for EndpointViolation {}
pub(crate) fn child_exit_record(exit: &OwnedChildExit) -> Value {
    // Linux waitid returns CLD_EXITED=1, CLD_KILLED=2 or CLD_DUMPED=3 for WEXITED.
    // Preserve raw provenance. ECHILD has no wait status and is never called a signal.
    let classification=match exit.observation.si_code {
        Some(1)=>"EXITED", Some(2)=>"KILLED", Some(3)=>"DUMPED", None=>"NO_CHILD_STATUS", _=>"OTHER_WAITID_CODE",
    };
    json!({"role":exit.role.as_str(),"owned_pid":exit.pid,"observed_pid":exit.observation.observed_pid,
        "si_code":exit.observation.si_code,"si_status":exit.observation.si_status,"classification":classification,
        "observation_errno":exit.observation.observation_errno,
        "source":"WAITID_WEXITED_WNOHANG_WNOWAIT","reaped_by_observation":false})
}
#[derive(Debug)]
pub struct ReadinessExpired;
impl std::fmt::Display for ReadinessExpired { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.write_str("Readiness deadline exceeded") } }
impl std::error::Error for ReadinessExpired {}
pub fn record(error: &anyhow::Error) -> Value {
    let phase = error.downcast_ref::<StartupPhase>();
    let io = error.downcast_ref::<std::io::Error>();
    let class = if crate::processes::has_cleanup_failure(error) { "CLEANUP_FAILURE" }
        else if error.is::<OperationCancelled>() || error.is::<dytallix_release_runtime::ownership::OwnershipCancelled>() { "CANCELLED" }
        else if error.is::<ReadinessExpired>() { "DEADLINE_EXPIRED" }
        else if error.is::<OwnedChildExit>() { "OWNED_CHILD_EXIT" }
        else if error.is::<EndpointViolation>() { "ENDPOINT_VIOLATION" }
        else if io.is_some() { "IO_ERROR" } else { "OTHER_FAILURE" };
    let kind = io.map(|value| match value.kind() {
        std::io::ErrorKind::PermissionDenied => "PERMISSION_DENIED",
        std::io::ErrorKind::NotFound => "NOT_FOUND",
        std::io::ErrorKind::TimedOut => "TIMED_OUT",
        std::io::ErrorKind::UnexpectedEof => "UNEXPECTED_EOF",
        std::io::ErrorKind::BrokenPipe => "BROKEN_PIPE",
        std::io::ErrorKind::WouldBlock => "WOULD_BLOCK",
        std::io::ErrorKind::Interrupted => "INTERRUPTED",
        _ => "OTHER_IO",
    });
    json!({"scope":"BOUNDED_STARTUP_FAILURE_V1",
        "role":phase.map(|p|p.role.as_str()), "stage":phase.map(|p|p.stage.name()),
        "error_class":class, "io_kind":kind, "os_errno":io.and_then(|e|e.raw_os_error()),
        "cleanup_failure":crate::processes::has_cleanup_failure(error),
        "child_exit":error.downcast_ref::<OwnedChildExit>().map(child_exit_record),
        "endpoint_violation":error.downcast_ref::<EndpointViolation>().map(|v|v.name()), "production_qualified":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_os_error_and_specific_phase_survive_outer_context() {
        let error = phase::<()>(Err(std::io::Error::from_raw_os_error(13).into()),Role::Engine,Stage::GuardReady).unwrap_err();
        let error = phase::<()>(Err(error),Role::Engine,Stage::Start).unwrap_err().context("private configuration omitted");
        let row=record(&error);
        assert_eq!(row["stage"],"guard_ready");assert_eq!(row["role"],"consensus_engine");
        assert_eq!(row["os_errno"],13);assert_eq!(row["error_class"],"IO_ERROR");
        assert!(error.downcast_ref::<std::io::Error>().is_some());
    }
    #[test]
    fn source_display_and_private_context_are_never_serialized() {
        #[derive(Debug)]struct Secret;
        impl std::fmt::Display for Secret {fn fmt(&self,_:&mut std::fmt::Formatter<'_>)->std::fmt::Result {panic!("source Display must not run")}}
        impl std::error::Error for Secret {}
        let error=phase::<()>(Err(anyhow::Error::new(Secret).context("SECRET_KEY_AND_ARGV")),Role::Bridge,Stage::ObserveInitial).unwrap_err();
        let raw=serde_json::to_vec(&record(&error)).unwrap();
        assert!(raw.len()<1024);assert!(!String::from_utf8(raw).unwrap().contains("SECRET"));
    }
    #[test]
    fn cleanup_failure_retains_priority_and_causal_phase() {
        let original=phase::<()>(Err(OperationCancelled.into()),Role::Application,Stage::ControlledObservation);
        let error=crate::processes::with_owned_cleanup(original,Err(anyhow::anyhow!("private cleanup context")),"Startup").unwrap_err();
        let row=record(&error);assert_eq!(row["error_class"],"CLEANUP_FAILURE");assert_eq!(row["cleanup_failure"],true);
        assert_eq!(row["stage"],"controlled_observation");assert!(error.is::<OperationCancelled>());
    }
    #[test]
    fn unknown_failure_does_not_invent_role_stage_or_errno() {
        let row=record(&anyhow::anyhow!("secret source"));
        assert!(row["role"].is_null());assert!(row["stage"].is_null());assert!(row["os_errno"].is_null());
        assert_eq!(row["error_class"],"OTHER_FAILURE");
    }
}

#[cfg(test)]
mod exit_record_tests {
    use super::*;
    #[test]
    fn raw_status_and_owned_role_are_preserved_through_cleanup_failure() {
        for (code,expected) in [(1,"EXITED"),(2,"KILLED"),(3,"DUMPED")] {
            let error=OwnedChildExit {role:Role::Bridge,pid:123,observation:ExitObservation{observed_pid:Some(123),si_code:Some(code),si_status:Some(9),observation_errno:None}};
            let failure=phase::<()>(Err(error.into()),Role::Bridge,Stage::EndpointReady);
            let failure=crate::processes::with_owned_cleanup(failure,Err(anyhow::anyhow!("secret cleanup")),"Startup").unwrap_err();
            let row=record(&failure);assert_eq!(row["error_class"],"CLEANUP_FAILURE");assert_eq!(row["child_exit"]["classification"],expected);
            assert_eq!(row["child_exit"]["owned_pid"],123);assert_eq!(row["child_exit"]["observed_pid"],123);assert_eq!(row["child_exit"]["si_status"],9);
            assert_eq!(row["child_exit"]["reaped_by_observation"],false);assert_eq!(row["stage"],"endpoint_ready");
        }
    }
    #[test]
    fn missing_wait_status_does_not_invent_signal_or_code() {
        let error=OwnedChildExit{role:Role::Application,pid:123,observation:ExitObservation{observed_pid:None,si_code:None,si_status:None,observation_errno:Some(libc::ECHILD)}};
        let row=record(&error.into());assert_eq!(row["child_exit"]["classification"],"NO_CHILD_STATUS");
        assert!(row["child_exit"]["si_code"].is_null());assert!(row["child_exit"]["si_status"].is_null());assert_eq!(row["child_exit"]["observation_errno"],libc::ECHILD);
    }
    #[test]
    fn every_endpoint_branch_has_a_fixed_distinct_code() {
        for (violation,expected) in [(EndpointViolation::InodeChanged,"INODE_CHANGED"),(EndpointViolation::Disappeared,"DISAPPEARED"),(EndpointViolation::TypeOrOwner,"TYPE_OR_OWNER"),(EndpointViolation::NotAbsolute,"NOT_ABSOLUTE"),(EndpointViolation::ParentMissing,"PARENT_MISSING"),(EndpointViolation::ParentAlias,"PARENT_ALIAS"),(EndpointViolation::ParentOwnershipOrMode,"PARENT_OWNERSHIP_OR_MODE")] {
            let error=phase::<()>(Err(violation.into()),Role::Bridge,Stage::EndpointReady).unwrap_err();let row=record(&error);
            assert_eq!(row["endpoint_violation"],expected);assert_eq!(row["error_class"],"ENDPOINT_VIOLATION");assert!(row["child_exit"].is_null());assert!(row["os_errno"].is_null());
            assert!(serde_json::to_vec(&row).unwrap().len()<1024);
        }
    }
}

#[cfg(test)]
mod readiness_phase_tests {
    use super::*;
    #[test]
    fn typed_deadline_retains_specific_stage_and_cleanup_priority() {
        let error=phase::<()>(Err(ReadinessExpired.into()),Role::Engine,Stage::StatusRpc).unwrap_err();
        let error=phase::<()>(Err(error),Role::Engine,Stage::EngineReadiness).unwrap_err();
        let row=record(&error);assert_eq!(row["stage"],"readiness_deadline");assert_eq!(row["error_class"],"DEADLINE_EXPIRED");
        let error=crate::processes::with_owned_cleanup::<()>(Err(error),Err(anyhow::anyhow!("private cleanup")),"startup").unwrap_err();
        assert_eq!(record(&error)["error_class"],"CLEANUP_FAILURE");
    }
    #[test]
    fn readiness_stages_are_fixed_and_inner_stage_survives() {
        for stage in [Stage::IpcConnect,Stage::IpcPeer,Stage::IpcFrame,Stage::StatusRpc,Stage::StatusValidation,Stage::ApplicationRpc,Stage::ApplicationValidation,Stage::ApplicationComparison,Stage::AdapterListener,Stage::AdapterHttp,Stage::AdapterIdentity] {
            let error=phase::<()>(Err(anyhow::anyhow!("private detail")),Role::Engine,stage).unwrap_err();
            let error=phase::<()>(Err(error),Role::Engine,Stage::EngineReadiness).unwrap_err();
            assert_eq!(record(&error)["stage"],stage.name());assert!(!record(&error).to_string().contains("private"));
        }
    }
}
