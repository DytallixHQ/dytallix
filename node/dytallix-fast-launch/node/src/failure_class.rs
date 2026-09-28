//! Fixed classes for a stopped application (E04 gap 15, P01 28 September
//! 2026). Only the class leaves the process: as the application's exit
//! status and in the supervisor's failure record. The error text stays with
//! the operator's read-only check (`dytallix-state-check`).
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// The configuration or genesis is invalid or differs from storage.
    Configuration,
    /// A stored block record, its commitments or the application hash differ.
    History,
    /// The supply accounting differs.
    Supply,
    /// The emergency, upgrade or handover history does not replay.
    Replay,
    /// The running executable is not the committed release.
    Release,
    /// Executing a block failed after its inputs passed.
    Execution,
    /// The database could not be opened, read or written.
    Storage,
    /// The system refused space, memory, descriptors or tasks.
    Resource,
}

const RESOURCE_ERRNOS: [i32; 6] = [
    libc::ENOSPC,
    libc::EDQUOT,
    libc::ENOMEM,
    libc::EMFILE,
    libc::ENFILE,
    libc::EAGAIN,
];

impl FailureClass {
    pub const ALL: [Self; 8] = [
        Self::Configuration,
        Self::History,
        Self::Supply,
        Self::Replay,
        Self::Release,
        Self::Execution,
        Self::Storage,
        Self::Resource,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::History => "history",
            Self::Supply => "supply",
            Self::Replay => "replay",
            Self::Release => "release",
            Self::Execution => "execution",
            Self::Storage => "storage",
            Self::Resource => "resource",
        }
    }
    /// The application's exit status. 1 stays unclassified; 101 is a panic.
    pub fn exit_status(self) -> u8 {
        match self {
            Self::Configuration => 10,
            Self::History => 11,
            Self::Supply => 12,
            Self::Replay => 13,
            Self::Release => 14,
            Self::Execution => 15,
            Self::Storage => 16,
            Self::Resource => 17,
        }
    }
    pub fn from_exit_status(status: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|class| i32::from(class.exit_status()) == status)
    }
}
/// The class, attached as context. It displays the message it covers, so an
/// error's text and every downcast are unchanged; only the alternate form
/// (`{:#}`) repeats the first line, which `describe` drops.
#[derive(Debug)]
struct Classified {
    class: FailureClass,
    message: String,
}
impl std::fmt::Display for Classified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Mark the error with `class` unless it already has one: the first, most
/// specific class wins.
pub fn classify<T>(result: Result<T>, class: FailureClass) -> Result<T> {
    result.map_err(|error| {
        if error.is::<Classified>() {
            error
        } else {
            let message = error.to_string();
            error.context(Classified { class, message })
        }
    })
}

/// The error's class. A cancellation has none. A refused resource anywhere
/// in the chain wins over the mark: the cause is the host, not the state.
pub fn class_of(error: &anyhow::Error) -> Option<FailureClass> {
    if error.is::<dytallix_release_runtime::ownership::OwnershipCancelled>() {
        return None;
    }
    let refused = error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::raw_os_error)
            .is_some_and(|errno| RESOURCE_ERRNOS.contains(&errno))
            // RocksDB reports errno only as text; the service runs with a
            // cleared environment, so the C locale's message applies.
            || cause.downcast_ref::<rocksdb::Error>().is_some_and(|e| {
                e.kind() == rocksdb::ErrorKind::IOError
                    && e.as_ref().contains("No space left on device")
            })
    });
    if refused {
        return Some(FailureClass::Resource);
    }
    error.downcast_ref::<Classified>().map(|marked| marked.class)
}

/// The full error text without the class's repeated line.
pub fn describe(error: &anyhow::Error) -> String {
    let mut lines: Vec<String> = error.chain().map(ToString::to_string).collect();
    lines.dedup();
    lines.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Context};

    #[test]
    fn exit_statuses_are_fixed_and_distinct() {
        let statuses: Vec<u8> = FailureClass::ALL.iter().map(|c| c.exit_status()).collect();
        assert_eq!(statuses, [10, 11, 12, 13, 14, 15, 16, 17]);
        for class in FailureClass::ALL {
            assert_eq!(FailureClass::from_exit_status(class.exit_status().into()), Some(class));
        }
        for other in [0, 1, 2, 9, 18, 101, 137] {
            assert_eq!(FailureClass::from_exit_status(other), None);
        }
    }
    #[test]
    fn the_first_class_survives_outer_context() {
        let inner = classify::<()>(Err(anyhow!("totals differ")), FailureClass::Supply);
        let outer = classify(inner.context("history check"), FailureClass::History).unwrap_err();
        assert_eq!(class_of(&outer), Some(FailureClass::Supply));
        assert_eq!(class_of(&anyhow!("unmarked")), None);
        assert_eq!(describe(&outer), "history check: totals differ");
    }
    #[test]
    fn text_and_downcasts_are_unchanged() {
        #[derive(Debug)]
        struct Rejected;
        impl std::fmt::Display for Rejected {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("rejected")
            }
        }
        impl std::error::Error for Rejected {}
        let error = classify::<()>(
            Err(anyhow::Error::new(Rejected).context("control refused")),
            FailureClass::Replay,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "control refused");
        assert!(error.downcast_ref::<Rejected>().is_some());
        assert!(error.chain().any(|cause| cause.is::<Rejected>()));
        assert_eq!(describe(&error), "control refused: rejected");
    }
    #[test]
    fn a_cancellation_has_no_class() {
        let error = classify::<()>(
            Err(dytallix_release_runtime::ownership::OwnershipCancelled.into()),
            FailureClass::Replay,
        )
        .unwrap_err();
        assert_eq!(class_of(&error), None);
    }
    #[test]
    fn a_refused_resource_wins_over_the_mark() {
        for errno in RESOURCE_ERRNOS {
            let error = classify::<()>(
                Err(std::io::Error::from_raw_os_error(errno).into()),
                FailureClass::Storage,
            )
            .unwrap_err();
            assert_eq!(class_of(&error), Some(FailureClass::Resource), "{errno}");
        }
        let denied = classify::<()>(
            Err(std::io::Error::from_raw_os_error(libc::EACCES).into()),
            FailureClass::Storage,
        )
        .unwrap_err();
        assert_eq!(class_of(&denied), Some(FailureClass::Storage));
    }
}
