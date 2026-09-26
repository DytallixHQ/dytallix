//! Failure-only summaries. These values never authorize a snapshot.
use super::*;
use serde::Serialize;

#[derive(Default, Serialize)]
struct Classes {
    anonymous: usize,
    file: usize,
    kernel: usize,
}
impl Classes {
    fn add(&mut self, map: &Mapping) {
        match map.kind {
            MappingKind::Anonymous => self.anonymous += 1,
            MappingKind::File(_) => self.file += 1,
            MappingKind::Kernel(_) => self.kernel += 1,
        }
    }
}

#[derive(Default, Serialize)]
struct Changes {
    added: Classes,
    removed: Classes,
    changed_before: Classes,
    changed_after: Classes,
    same_start_range_changes: usize,
    permission_changes: usize,
    backing_identity_changes: usize,
}

// Pair entries only at the same start address. Moved starts count as removal
// and addition. Splits and merges do not imply an identified causal operation.
fn compare(before: &[Mapping], after: &[Mapping]) -> Changes {
    let mut changes = Changes::default();
    let (mut left, mut right) = (0, 0);
    while left < before.len() || right < after.len() {
        match (before.get(left), after.get(right)) {
            (Some(a), Some(b)) if a.start == b.start => {
                if a != b {
                    changes.changed_before.add(a);
                    changes.changed_after.add(b);
                    changes.same_start_range_changes += usize::from(a.end != b.end);
                    changes.permission_changes += usize::from(a.permissions != b.permissions);
                    changes.backing_identity_changes += usize::from(
                        (a.device_major, a.device_minor, a.inode, a.offset, &a.kind)
                            != (b.device_major, b.device_minor, b.inode, b.offset, &b.kind),
                    );
                }
                left += 1;
                right += 1;
            }
            (Some(a), Some(b)) if a.start < b.start => {
                changes.removed.add(a);
                left += 1;
            }
            (Some(_), Some(b)) | (None, Some(b)) => {
                changes.added.add(b);
                right += 1;
            }
            (Some(a), None) => {
                changes.removed.add(a);
                left += 1;
            }
            (None, None) => break,
        }
    }
    changes
}

pub(super) fn summary(
    before: &[u8],
    after: &[u8],
    maps: &[Mapping],
    bounds: &Bounds,
    start: &StartIdentity,
    scope: ObservationScope,
    role: &str,
    elapsed: Duration,
) -> String {
    // Reuse the bounded strict parser. Rejection is reported without its text:
    // parser messages or input paths must not become unbounded log content.
    let parsed = parse_maps(after, bounds);
    let changes = parsed.as_ref().ok().map(|second| compare(maps, second));
    let role_prefix: String = role.chars().take(64).collect();
    serde_json::json!({
        "schema": "mapping-observation-failure-v1",
        "scope": match scope { ObservationScope::OwnedChild => "owned_child", ObservationScope::CurrentProcess => "current_process" },
        "phase": "second_maps_read",
        "pid": start.pid,
        "start_ticks": start.start_ticks,
        "identity_rechecked_after_change": false,
        "role": role_prefix,
        "role_truncated": role.chars().count() > 64,
        "role_sha256": hex::encode(Sha256::digest(role.as_bytes())),
        "before_bytes": before.len(),
        "after_bytes": after.len(),
        "before_sha256": hex::encode(Sha256::digest(before)),
        "after_sha256": hex::encode(Sha256::digest(after)),
        "before_entries": maps.len(),
        "after_entries": parsed.as_ref().ok().map(Vec::len),
        "second_parse_rejected": parsed.is_err(),
        "changes": changes,
        "entries_emitted": 0,
        "raw_maps_emitted": false,
        "counts_truncated": false,
        "elapsed_ms_before_diagnostics": elapsed.as_millis().min(u64::MAX as u128) as u64,
        "deadline_exceeded_before_diagnostics": elapsed >= bounds.max_elapsed,
    }).to_string()
}
