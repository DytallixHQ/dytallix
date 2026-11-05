//! Bounded failed-helper diagnostics. These records do not grant authority.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct ObservedHelperDeadlineExpired {
    pub deadline: Instant,
    pub checked_monotonic_ns: u64,
}
impl std::fmt::Display for ObservedHelperDeadlineExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Observed helper total execution deadline exceeded")
    }
}
impl std::error::Error for ObservedHelperDeadlineExpired {}

fn causal_class(error: &anyhow::Error, execution_deadline: Option<Instant>) -> &'static str {
    if error.downcast_ref::<dytallix_release_runtime::ownership::OwnershipCancelled>().is_some() {
        "CANCELLED"
    } else if let Some(expired)=error.downcast_ref::<ObservedHelperDeadlineExpired>() {
        if Some(expired.deadline)==execution_deadline {"EXECUTION_DEADLINE_EXPIRED"} else {"OTHER_DEADLINE_EXPIRED"}
    } else {
        "OTHER_FAILURE"
    }
}

pub(super) fn preserve_error(error: anyhow::Error, cleanup: Result<()>) -> anyhow::Error {
    match cleanup {
        Ok(())=>error,
        Err(cleanup)=>error.context(format!("Observed helper cleanup failed: {cleanup:#}")),
    }
}

/// At most `limit` characters of `raw`, control characters as spaces, so a
/// record line stays one bounded line.
fn bounded_text(raw: &str, limit: usize) -> String {
    raw.chars().map(|c| if c.is_control() { ' ' } else { c }).take(limit).collect()
}

/// Why the helper failed, for the operator (P01, 4 November 2026): the check
/// that failed and what the helper itself wrote to stderr, both bounded.
/// Neither carries a secret: the helper reads only public policy and request
/// bytes.
const ERROR_TEXT_CHARS: usize = 240;
const HELPER_STDERR_CHARS: usize = 480;

fn encode(binding: Value, error: &anyhow::Error, attempt: u64, cleanup_ok: bool,
          reaped: bool, exit_status: Option<i32>, elapsed_ms: u64, execution_deadline: Option<Instant>,
          helper_stderr: &[u8]) -> Result<Vec<u8>> {
    let why = serde_json::json!({
        "error_text":bounded_text(&format!("{error:#}"),ERROR_TEXT_CHARS),
        "helper_stderr":bounded_text(&String::from_utf8_lossy(helper_stderr),HELPER_STDERR_CHARS),
    });
    // The reasons go only when the record stays within its bound.
    match encode_row(binding.clone(),error,attempt,cleanup_ok,reaped,exit_status,elapsed_ms,execution_deadline,Some(why)) {
        Ok(bytes) => Ok(bytes),
        Err(_) => encode_row(binding,error,attempt,cleanup_ok,reaped,exit_status,elapsed_ms,execution_deadline,None),
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_row(mut binding: Value, error: &anyhow::Error, attempt: u64, cleanup_ok: bool,
              reaped: bool, exit_status: Option<i32>, elapsed_ms: u64, execution_deadline: Option<Instant>,
              why: Option<Value>) -> Result<Vec<u8>> {
    let row = binding.as_object_mut().context("Helper failure binding is not an object")?;
    row.extend(serde_json::json!({
        "schema":1, "scope":"LOCAL_FAILED_HELPER_LIFECYCLE_V1",
        "production_qualified":false, "release_authority":false,
        "attempt":attempt, "failure_class":causal_class(error,execution_deadline),
        "deadline_checked_monotonic_ns":error.downcast_ref::<ObservedHelperDeadlineExpired>().map(|e|e.checked_monotonic_ns),
        "cleanup_status":if cleanup_ok {"COMPLETED"} else {"FAILED"},
        "reaped":reaped, "exit_status_raw":exit_status,
        "total_elapsed_ms":elapsed_ms
    }).as_object().unwrap().clone());
    if let Some(why) = why {
        row.extend(why.as_object().unwrap().clone());
    }
    let mut bytes=serde_json::to_vec(&binding)?;
    bytes.push(b'\n');
    ensure!(bytes.len()<=4096,"Helper failure diagnostic exceeded bound");
    Ok(bytes)
}

// No fresh timeout on retry. A blocked sink cannot hold the creator indefinitely.
fn deliver(bytes: &[u8], deadline: Instant,
           mut send: impl FnMut(&[u8])->std::io::Result<usize>) -> Result<()> {
    let mut offset=0;
    while offset<bytes.len() {
        ensure!(Instant::now()<deadline,"Helper failure diagnostic deadline expired");
        match send(&bytes[offset..]) {
            Ok(0)=>anyhow::bail!("Helper failure diagnostic socket closed"),
            Ok(count)=>{ensure!(count<=bytes.len()-offset,"Invalid diagnostic write count");offset+=count;},
            Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>{
                let remaining=deadline.saturating_duration_since(Instant::now());
                if !remaining.is_zero(){std::thread::sleep(remaining.min(Duration::from_millis(1)));}
            },
            Err(error) if error.kind()==std::io::ErrorKind::Interrupted=>{},
            Err(error)=>return Err(error.into()),
        }
    }
    ensure!(Instant::now()<deadline,"Helper failure diagnostic completed after deadline");
    Ok(())
}

#[cfg(target_os="linux")]
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(binding: Value, error: &anyhow::Error, attempt: u64,
                   cleanup_ok: bool, reaped: bool, exit_status: Option<i32>,
                   started: Instant, total_deadline: Instant, execution_deadline: Instant,
                   helper_stderr: &[u8]) -> Result<()> {
    let now=Instant::now();
    ensure!(now<total_deadline,"No remaining helper diagnostic budget");
    let deadline=total_deadline.min(now.checked_add(Duration::from_millis(200)).context("Diagnostic deadline overflow")?);
    let bytes=encode(binding,error,attempt,cleanup_ok,reaped,exit_status,u64::try_from(started.elapsed().as_millis())?,Some(execution_deadline),helper_stderr)?;
    match hold(bytes) {Some(bytes)=>send_socket(2,&bytes,deadline), None=>Ok(())}
}

/// A transaction CheckTx or the mempool's recheck refused because the helper
/// did not run (P01, 5 November 2026): one bounded line, the refusals since
/// the process started and those left unwritten since the last line. The
/// caller's gate allows one a minute.
fn encode_admission(error: &anyhow::Error, refused: u64, unwritten: u64) -> Result<Vec<u8>> {
    let mut bytes=serde_json::to_vec(&serde_json::json!({
        "schema":1, "scope":"LOCAL_ADMISSION_HELPER_FAILURE_V1",
        "production_qualified":false, "release_authority":false,
        "refused_total":refused, "unwritten_since_last_line":unwritten,
        "error_text":bounded_text(&format!("{error:#}"),ERROR_TEXT_CHARS)
    }))?;
    bytes.push(b'\n');
    ensure!(bytes.len()<=4096,"Admission helper failure line exceeded bound");
    Ok(bytes)
}

// CheckTx's scope (P01, 5 November 2026): the record of a helper that fails
// inside it is held for the caller's once-a-minute gate instead of sent, since
// anyone can send a control to a public node. Startup, proposals and blocks
// send every record.
thread_local!{static HELD: std::cell::RefCell<Option<Vec<Vec<u8>>>> = const{std::cell::RefCell::new(None)};}

/// Runs `check` holding the records of the helpers that fail inside it, and
/// returns them with its result. An enclosing scope keeps them instead.
pub(crate) fn holding_records<T>(check: impl FnOnce()->T) -> (T, Vec<Vec<u8>>) {
    if HELD.with(|held|held.borrow().is_some()) {return (check(),Vec::new());}
    struct Scope;
    impl Drop for Scope {fn drop(&mut self){HELD.with(|held|held.borrow_mut().take());}}
    HELD.with(|held|*held.borrow_mut()=Some(Vec::new()));
    let scope=Scope;
    let result=check();
    let records=HELD.with(|held|held.borrow_mut().take()).unwrap_or_default();
    drop(scope);
    (result,records)
}

/// The record back to send now, or None when a scope holds it.
fn hold(record: Vec<u8>) -> Option<Vec<u8>> {
    HELD.with(|held|match held.borrow_mut().as_mut() {Some(records)=>{records.push(record);None}, None=>Some(record)})
}

/// Sent as the failure record is; a short budget, since CheckTx waits on it.
#[cfg(target_os="linux")]
fn admission_deadline() -> Result<Instant> {
    Instant::now().checked_add(Duration::from_millis(50)).context("Diagnostic deadline overflow")
}

#[cfg(target_os="linux")]
pub(crate) fn emit_admission(error: &anyhow::Error, refused: u64, unwritten: u64) -> Result<()> {
    send_socket(2,&encode_admission(error,refused,unwritten)?,admission_deadline()?)
}

/// A record held in CheckTx's scope, sent when the gate allows it.
#[cfg(target_os="linux")]
pub(crate) fn send_held(record: &[u8]) -> Result<()> {
    send_socket(2,record,admission_deadline()?)
}

/// A node warning (P01, 5 November 2026): a snapshot restore or snapshot
/// write that failed, or metrics not recorded at a commit. One bounded line
/// with the warnings of its kind since the process started and those of its
/// kind not written since its last line. The caller's gate allows one a
/// minute for each kind.
fn encode_warning(kind: &str, text: &str, total: u64, unwritten: u64) -> Result<Vec<u8>> {
    let mut bytes=serde_json::to_vec(&serde_json::json!({
        "schema":1, "scope":"LOCAL_NODE_WARNING_V1",
        "production_qualified":false, "release_authority":false,
        "kind":kind, "warnings_total":total, "unwritten_since_last_line":unwritten,
        "error_text":bounded_text(text,ERROR_TEXT_CHARS)
    }))?;
    bytes.push(b'\n');
    ensure!(bytes.len()<=4096,"Node warning line exceeded bound");
    Ok(bytes)
}

/// Commit writes some of those lines while it holds the execution lock, so
/// none waits for the journal (P01, 5 November 2026): a line the journal
/// takes nothing of is dropped at once, and one it takes part of is finished
/// within 5 ms or dropped.
fn deliver_now(bytes: &[u8], mut send: impl FnMut(&[u8])->std::io::Result<usize>) -> Result<()> {
    let deadline=Instant::now().checked_add(Duration::from_millis(5)).context("Diagnostic deadline overflow")?;
    deliver(bytes,deadline,|part|match send(part) {
        Err(error) if error.kind()==std::io::ErrorKind::WouldBlock && part.len()==bytes.len()=>
            Err(std::io::Error::other("Journal did not take the node warning")),
        sent=>sent,
    })
}

#[cfg(target_os="linux")]
pub(crate) fn emit_warning(kind: &str, text: &str, total: u64, unwritten: u64) -> Result<()> {
    deliver_now(&encode_warning(kind,text,total,unwritten)?,socket(2)?)
}

#[cfg(target_os="linux")]
fn send_socket(fd: libc::c_int, bytes: &[u8], deadline: Instant) -> Result<()> {
    ensure!(Instant::now()<deadline,"Helper failure diagnostic deadline expired");
    deliver(bytes,deadline,socket(fd)?)
}

/// Sends on `fd`, which must be the inherited journal socket, never waiting.
#[cfg(target_os="linux")]
fn socket(fd: libc::c_int) -> Result<impl FnMut(&[u8])->std::io::Result<usize>> {
    let mut stat:libc::stat=unsafe{std::mem::zeroed()};
    ensure!(unsafe{libc::fstat(fd,&mut stat)}==0 && stat.st_mode&libc::S_IFMT==libc::S_IFSOCK,
        "Helper diagnostic requires inherited socket FD2");
    // MSG_DONTWAIT does not change shared open-file flags. No pipe/file fallback.
    Ok(move |part:&[u8]|{
        let count=unsafe{libc::send(fd,part.as_ptr() as *const _,part.len(),libc::MSG_DONTWAIT|libc::MSG_NOSIGNAL)};
        if count<0 {Err(std::io::Error::last_os_error())} else {Ok(count as usize)}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_causal_type_marks_cancellation_and_cleanup_failure_stays_failed(){
        let typed:anyhow::Error=dytallix_release_runtime::ownership::OwnershipCancelled.into();
        let typed=typed.context("outer failure");
        assert_eq!(causal_class(&typed,None),"CANCELLED");
        // Identical text, or a later global signal flag, does not change this error's type.
        let text=anyhow::anyhow!("Ownership operation cancelled");
        let flag=dytallix_release_runtime::ownership::cancellation_handle();
        let before=flag.swap(true,std::sync::atomic::Ordering::SeqCst);
        let unrelated=causal_class(&text,None);
        flag.store(before,std::sync::atomic::Ordering::SeqCst);
        assert_eq!(unrelated,"OTHER_FAILURE");
        let raw=encode(serde_json::json!({}),&typed,1,false,false,None,3,None,b"").unwrap();
        let row:Value=serde_json::from_slice(&raw).unwrap();
        assert_eq!(row["failure_class"],"CANCELLED");assert_eq!(row["cleanup_status"],"FAILED");assert_eq!(row["reaped"],false);
    }
    #[test]
    fn the_record_says_why_within_its_bound(){
        // P01, 4 November 2026: the failed check and the helper's own stderr,
        // bounded, one line; dropped rather than overflowing the record.
        let error=anyhow::anyhow!("Observed helper READY protocol differs").context("helper run");
        let raw=encode(serde_json::json!({}),&error,4,true,true,Some(256),56,None,b"policy\nrefused\x07 here").unwrap();
        let row:Value=serde_json::from_slice(&raw).unwrap();
        assert_eq!(row["error_text"],"helper run: Observed helper READY protocol differs");
        assert_eq!(row["helper_stderr"],"policy refused  here");
        assert_eq!(raw.iter().filter(|&&b|b==b'\n').count(),1);
        let noisy=encode(serde_json::json!({}),&error,4,true,true,None,1,None,&[b'x';10_000]).unwrap();
        let row:Value=serde_json::from_slice(&noisy).unwrap();
        assert_eq!(row["helper_stderr"].as_str().unwrap().chars().count(),HELPER_STDERR_CHARS);
        let crowded=encode(serde_json::json!({"large":"a".repeat(3500)}),&error,4,true,true,None,1,None,&[b'x';480]).unwrap();
        let row:Value=serde_json::from_slice(&crowded).unwrap();
        assert!(row.get("helper_stderr").is_none() && crowded.len()<=4096);
    }
    #[test]
    fn the_admission_line_is_one_bounded_record(){
        // P01, 5 November 2026: the error, bounded, with the counts.
        let error=anyhow::anyhow!("{}\n{}","x".repeat(300),"tail").context("Emergency helper execution failed");
        let raw=encode_admission(&error,7,3).unwrap();
        assert_eq!(raw.iter().filter(|&&b|b==b'\n').count(),1);assert!(raw.ends_with(b"\n"));
        let row:Value=serde_json::from_slice(&raw).unwrap();
        assert_eq!((row["scope"].as_str(),row["refused_total"].as_u64(),row["unwritten_since_last_line"].as_u64()),
                   (Some("LOCAL_ADMISSION_HELPER_FAILURE_V1"),Some(7),Some(3)));
        assert_eq!((row["production_qualified"].as_bool(),row["release_authority"].as_bool()),(Some(false),Some(false)));
        let text=row["error_text"].as_str().unwrap();
        assert!(text.starts_with("Emergency helper execution failed: xxx"));
        assert_eq!(text.chars().count(),ERROR_TEXT_CHARS);
    }
    #[test]
    fn a_check_holds_its_helpers_records_for_the_gate(){
        let (value,records)=holding_records(||{
            assert!(hold(b"one\n".to_vec()).is_none());
            let (_,inner)=holding_records(||assert!(hold(b"two\n".to_vec()).is_none()));
            assert!(inner.is_empty());
            7
        });
        assert_eq!((value,records),(7,vec![b"one\n".to_vec(),b"two\n".to_vec()]));
        assert_eq!(hold(b"three\n".to_vec()),Some(b"three\n".to_vec()));
        assert!(std::panic::catch_unwind(||holding_records(||panic!("check failed"))).is_err());
        assert_eq!(hold(b"four\n".to_vec()),Some(b"four\n".to_vec()));
    }
    #[test]
    fn a_node_warning_is_one_bounded_record(){
        // P01, 5 November 2026: the kind and the warning, bounded, with the counts.
        let text=format!("Snapshot chunk 3 refused: {}\n{}","x".repeat(300),"tail");
        let raw=encode_warning("STATE_SYNC_RESTORE",&text,9,4).unwrap();
        assert_eq!(raw.iter().filter(|&&b|b==b'\n').count(),1);assert!(raw.ends_with(b"\n"));
        let row:Value=serde_json::from_slice(&raw).unwrap();
        assert_eq!((row["scope"].as_str(),row["kind"].as_str(),row["warnings_total"].as_u64(),row["unwritten_since_last_line"].as_u64()),
                   (Some("LOCAL_NODE_WARNING_V1"),Some("STATE_SYNC_RESTORE"),Some(9),Some(4)));
        assert_eq!((row["production_qualified"].as_bool(),row["release_authority"].as_bool()),(Some(false),Some(false)));
        let text=row["error_text"].as_str().unwrap();
        assert!(text.starts_with("Snapshot chunk 3 refused: xxx"));
        assert_eq!(text.chars().count(),ERROR_TEXT_CHARS);
    }
    #[test]
    fn a_node_warning_never_waits_for_a_full_journal(){
        // P01, 5 November 2026: taken in nothing, dropped at once, with no retry.
        let mut calls=0;
        assert!(deliver_now(b"warning\n",|_|{calls+=1;Err(std::io::ErrorKind::WouldBlock.into())}).is_err());
        assert_eq!(calls,1);
        // Taken in part, finished, so no half line joins the next one.
        let (mut output,mut full)=(Vec::new(),true);
        deliver_now(b"warning\n",|part|{
            if output.is_empty() {output.extend_from_slice(&part[..3]);return Ok(3);}
            if std::mem::take(&mut full) {return Err(std::io::ErrorKind::WouldBlock.into());}
            output.extend_from_slice(part);Ok(part.len())
        }).unwrap();
        assert_eq!(output,b"warning\n");
        // Or dropped at its 5 ms deadline.
        let (start,mut taken)=(Instant::now(),false);
        assert!(deliver_now(b"warning\n",|_|if std::mem::replace(&mut taken,true) {Err(std::io::ErrorKind::WouldBlock.into())} else {Ok(1)}).is_err());
        assert!(start.elapsed()<Duration::from_millis(200));
    }
    #[test]
    fn expired_and_blocked_sinks_do_not_extend_deadline(){
        let mut calls=0;
        assert!(deliver(b"record\n",Instant::now(), |_| {calls+=1;Ok(1)}).is_err());assert_eq!(calls,0);
        let start=Instant::now();
        assert!(deliver(b"record\n",start+Duration::from_millis(2), |_|Err(std::io::ErrorKind::WouldBlock.into())).is_err());
        assert!(start.elapsed()<Duration::from_millis(200));
    }
    #[test]
    fn partial_writes_are_not_duplicated_and_records_are_bounded(){
        let mut output=Vec::new();
        deliver(b"record\n",Instant::now()+Duration::from_secs(1),|part|{output.push(part[0]);Ok(1)}).unwrap();
        assert_eq!(output,b"record\n");
        assert!(encode(serde_json::json!({"large":"a".repeat(4096)}),&anyhow::anyhow!("failure"),1,true,true,Some(9),4,None,b"").is_err());
    }
    #[cfg(target_os="linux")]
    #[test]
    fn actual_socket_backpressure_closed_peer_and_non_socket_refusal(){
        use std::os::fd::{AsRawFd,FromRawFd};
        use std::os::unix::net::UnixStream;
        let (sender,receiver)=UnixStream::pair().unwrap();
        let fd=sender.as_raw_fd();
        let flags=unsafe{libc::fcntl(fd,libc::F_GETFL)};
        let descriptor_flags=unsafe{libc::fcntl(fd,libc::F_GETFD)};
        let fill=[7u8;4096];
        let fill_deadline=Instant::now()+Duration::from_millis(200);
        loop {
            assert!(Instant::now()<fill_deadline,"Socket fill deadline exceeded");
            let n=unsafe{libc::send(fd,fill.as_ptr() as *const _,fill.len(),libc::MSG_DONTWAIT|libc::MSG_NOSIGNAL)};
            if n<0 {assert_eq!(std::io::Error::last_os_error().kind(),std::io::ErrorKind::WouldBlock);break;}
        }
        assert!(send_socket(fd,b"record\n",Instant::now()+Duration::from_millis(2)).is_err());
        // A node warning is dropped at once on the full socket (P01, 5 November 2026).
        assert!(deliver_now(b"record\n",socket(fd).unwrap()).is_err());
        assert_eq!(unsafe{libc::fcntl(fd,libc::F_GETFL)},flags);
        assert_eq!(unsafe{libc::fcntl(fd,libc::F_GETFD)},descriptor_flags);
        drop(receiver);
        assert!(send_socket(fd,b"record\n",Instant::now()+Duration::from_millis(20)).is_err());
        let file=tempfile::tempfile().unwrap();
        assert!(send_socket(file.as_raw_fd(),b"record\n",Instant::now()+Duration::from_millis(20)).is_err());
        assert_eq!(file.metadata().unwrap().len(),0);
        let mut pipe=[-1;2];assert_eq!(unsafe{libc::pipe(pipe.as_mut_ptr())},0);
        let _read=unsafe{std::fs::File::from_raw_fd(pipe[0])};
        let write=unsafe{std::fs::File::from_raw_fd(pipe[1])};
        assert!(send_socket(write.as_raw_fd(),b"record\n",Instant::now()+Duration::from_millis(20)).is_err());
    }

    #[test]
    fn cleanup_failure_keeps_original_cause_and_priority(){
        let error:anyhow::Error=dytallix_release_runtime::ownership::OwnershipCancelled.into();
        let combined=preserve_error(error,Err(anyhow::anyhow!("reap deadline exceeded")));
        assert_eq!(causal_class(&combined,None),"CANCELLED");
        assert!(combined.to_string().starts_with("Observed helper cleanup failed: reap deadline exceeded"));
        let original=preserve_error(anyhow::anyhow!("unrelated failure"),Ok(()));
        assert_eq!(original.to_string(),"unrelated failure");
        assert_eq!(causal_class(&original,None),"OTHER_FAILURE");
    }

    #[test]
    fn only_original_execution_deadline_cause_is_classified_as_execution_expiry(){
        let execution=Instant::now();
        let total=execution+Duration::from_millis(20);
        let expired:anyhow::Error=ObservedHelperDeadlineExpired{deadline:execution,checked_monotonic_ns:77}.into();
        let expired=expired.context("channel read");
        assert_eq!(causal_class(&expired,Some(execution)),"EXECUTION_DEADLINE_EXPIRED");
        assert_eq!(causal_class(&expired,Some(total)),"OTHER_DEADLINE_EXPIRED");
        let unrelated=anyhow::anyhow!("Observed helper output ended before complete protocol frame");
        assert_eq!(causal_class(&unrelated,Some(execution)),"OTHER_FAILURE");
        let combined=preserve_error(unrelated,Err(ObservedHelperDeadlineExpired{deadline:total,checked_monotonic_ns:99}.into()));
        assert_eq!(causal_class(&combined,Some(execution)),"OTHER_FAILURE");
        let raw=encode(serde_json::json!({}),&expired,1,true,true,Some(9),3,Some(execution),b"").unwrap();
        let row:Value=serde_json::from_slice(&raw).unwrap();
        assert_eq!(row["failure_class"],"EXECUTION_DEADLINE_EXPIRED");assert_eq!(row["deadline_checked_monotonic_ns"],77);
    }

}
