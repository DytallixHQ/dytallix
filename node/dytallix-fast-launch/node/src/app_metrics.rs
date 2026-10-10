//! Application metrics (metrics v1): the core set, recorded at the points
//! the application measures and written as a Prometheus text-format file,
//! `dytallix-app.prom`, for an operator agent to read. No listener.
use crate::supply::NativeSupply;
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

pub const FILE_NAME: &str = "dytallix-app.prom";
const WRITTEN: &str = "dytallix_metrics_written_timestamp_seconds";
/// The metrics file format (interfaces v1); both processes write it.
const FORMAT: &str = "dytallix_metrics_format_version";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Summary {
    sum: f64,
    count: u64,
}
impl Summary {
    fn observe(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Values {
    height: u64,
    block_execution: Summary,
    commit: Summary,
    startup_check_seconds: Option<f64>,
    /// (denomination, bucket, amount), in a fixed order.
    supply: Vec<(&'static str, String, u128)>,
    retained_from: u64,
    records_pruned: u64,
    admission_entries: u64,
    /// Committed transactions by (kind, result), since the process started
    /// (P01, 10 October 2026), and the activity they carried.
    transactions: BTreeMap<(&'static str, &'static str), u64>,
    gas_used: u64,
    evidence: u64,
    root_controls: BTreeMap<&'static str, u64>,
}

/// What one committed block carried, for the counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockActivity {
    /// Each transaction's kind and whether it succeeded.
    pub transactions: Vec<(&'static str, bool)>,
    pub gas_used: u64,
    /// Validator evidence applied.
    pub evidence: u64,
    /// Root controls committed, by kind.
    pub root_controls: Vec<&'static str>,
}

/// SLH-DSA verifications by the root helper since the process started: valid,
/// invalid and the time they took. A helper failure is not a verification.
static HELPER_VALID: AtomicU64 = AtomicU64::new(0);
static HELPER_INVALID: AtomicU64 = AtomicU64::new(0);
static HELPER_NANOS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn helper_verified(valid: bool, elapsed: Duration) {
    let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
    HELPER_NANOS.fetch_add(nanos, Relaxed);
    if valid {
        &HELPER_VALID
    } else {
        &HELPER_INVALID
    }
    .fetch_add(1, Relaxed);
}

/// The latest published snapshot: height and write time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotState {
    pub height: u64,
    pub written_unix_seconds: u64,
}

#[derive(Debug, Default)]
pub struct AppMetrics {
    values: Mutex<Values>,
}
impl AppMetrics {
    pub fn new() -> Self {
        Self::default()
    }
    fn with<T>(&self, f: impl FnOnce(&mut Values) -> T) -> T {
        let mut values = self.values.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut values)
    }
    pub(crate) fn startup_checked(&self, seconds: f64) {
        self.with(|v| v.startup_check_seconds = Some(seconds));
    }
    pub(crate) fn block_executed(&self, seconds: f64) {
        self.with(|v| v.block_execution.observe(seconds));
    }
    /// One committed block's transactions, gas, evidence and root controls.
    pub(crate) fn block_activity(&self, activity: &BlockActivity) {
        self.with(|v| {
            for (kind, ok) in &activity.transactions {
                let result = if *ok { "ok" } else { "failed" };
                *v.transactions.entry((kind, result)).or_default() += 1;
            }
            v.gas_used = v.gas_used.saturating_add(activity.gas_used);
            v.evidence = v.evidence.saturating_add(activity.evidence);
            for kind in &activity.root_controls {
                *v.root_controls.entry(kind).or_default() += 1;
            }
        });
    }
    /// One commit: its time, the new head, the retained window, the block
    /// records it removed, the supply it leaves and the admission queue.
    pub(crate) fn committed(
        &self,
        seconds: f64,
        height: u64,
        retained_from: u64,
        pruned: u64,
        supply: &NativeSupply,
        admission_entries: u64,
    ) {
        let mut buckets: Vec<(&'static str, String, u128)> = vec![
            ("udrt", "genesis".into(), supply.drt.genesis),
            ("udrt", "emitted".into(), supply.drt.emitted),
            ("udrt", "burned".into(), supply.drt.burned),
            ("udrt", "total".into(), supply.drt.total),
            ("udrt", "liquid".into(), supply.drt.liquid),
            ("udrt", "withheld_fees".into(), supply.drt.withheld_fees),
        ];
        for (pool, amount) in &supply.drt.pools {
            buckets.push(("udrt", format!("pool_{pool}"), *amount));
        }
        let dgt = &supply.dgt;
        buckets.extend([
            ("udgt", "issued".into(), dgt.issued),
            ("udgt", "liquid".into(), dgt.liquid),
            ("udgt", "staked".into(), dgt.staked),
            ("udgt", "pending_bonded".into(), dgt.pending_bonded),
            ("udgt", "unbonding".into(), dgt.unbonding),
            ("udgt", "penalty_reserve".into(), dgt.penalty_reserve),
            ("udgt", "governance_escrow".into(), dgt.governance_escrow.unwrap_or(0)),
        ]);
        self.with(|v| {
            v.commit.observe(seconds);
            v.height = height;
            v.retained_from = retained_from;
            v.records_pruned += pruned;
            v.supply = buckets;
            v.admission_entries = admission_entries;
        });
    }

    /// The text format: each family once, then the write time.
    pub fn render(&self, now_unix_millis: u64, snapshot: Option<SnapshotState>) -> String {
        let v = self.with(|v| v.clone());
        let mut out = String::new();
        let mut gauge = |name: &str, value: String| {
            let _ = write!(out, "# TYPE dytallix_app_{name} gauge\ndytallix_app_{name} {value}\n");
        };
        gauge("height", v.height.to_string());
        gauge("retained_from_height", v.retained_from.to_string());
        gauge("admission_queue_entries", v.admission_entries.to_string());
        if let Some(seconds) = v.startup_check_seconds {
            gauge("startup_check_seconds", seconds.to_string());
        }
        if let Some(snapshot) = snapshot {
            gauge("snapshot_latest_height", snapshot.height.to_string());
            gauge(
                "snapshot_latest_written_timestamp_seconds",
                snapshot.written_unix_seconds.to_string(),
            );
        }
        let _ = write!(
            out,
            "# TYPE dytallix_app_block_records_pruned_total counter\n\
             dytallix_app_block_records_pruned_total {}\n",
            v.records_pruned
        );
        for (name, summary) in [
            ("block_execution_seconds", v.block_execution),
            ("commit_seconds", v.commit),
        ] {
            let _ = write!(
                out,
                "# TYPE dytallix_app_{name} summary\n\
                 dytallix_app_{name}_sum {}\ndytallix_app_{name}_count {}\n",
                summary.sum, summary.count
            );
        }
        let mut counter = |name: &str, rows: Vec<(String, u64)>| {
            let _ = writeln!(out, "# TYPE dytallix_app_{name} counter");
            for (labels, value) in rows {
                let _ = writeln!(out, "dytallix_app_{name}{labels} {value}");
            }
        };
        counter(
            "transactions_total",
            v.transactions
                .iter()
                .map(|((kind, result), n)| (format!("{{kind=\"{kind}\",result=\"{result}\"}}"), *n))
                .collect(),
        );
        counter("gas_used_total", vec![(String::new(), v.gas_used)]);
        counter(
            "validator_evidence_total",
            vec![(String::new(), v.evidence)],
        );
        counter(
            "root_controls_total",
            v.root_controls
                .iter()
                .map(|(kind, n)| (format!("{{kind=\"{kind}\"}}"), *n))
                .collect(),
        );
        // Signature verifications: ML-DSA-65 by the runtime crypto, SLH-DSA
        // by the root helper.
        let ml_dsa = dytallix_runtime_crypto::stats::verifications();
        let helper = (
            HELPER_VALID.load(Relaxed),
            HELPER_INVALID.load(Relaxed),
            HELPER_NANOS.load(Relaxed),
        );
        let schemes = [
            ("ml_dsa_65", ml_dsa.valid, ml_dsa.invalid, ml_dsa.nanos),
            ("slh_dsa_shake_256s", helper.0, helper.1, helper.2),
        ];
        let _ = writeln!(
            out,
            "# TYPE dytallix_app_signature_verifications_total counter"
        );
        for (scheme, valid, invalid, _) in schemes {
            for (result, n) in [("valid", valid), ("invalid", invalid)] {
                let _ = writeln!(
                    out,
                    "dytallix_app_signature_verifications_total{{scheme=\"{scheme}\",result=\"{result}\"}} {n}"
                );
            }
        }
        let _ = writeln!(
            out,
            "# TYPE dytallix_app_signature_verification_seconds summary"
        );
        for (scheme, valid, invalid, nanos) in schemes {
            let _ = write!(
                out,
                "dytallix_app_signature_verification_seconds_sum{{scheme=\"{scheme}\"}} {}\n\
                 dytallix_app_signature_verification_seconds_count{{scheme=\"{scheme}\"}} {}\n",
                nanos as f64 / 1e9,
                valid + invalid
            );
        }
        for denomination in ["udrt", "udgt"] {
            let _ = writeln!(out, "# TYPE dytallix_app_supply_{denomination} gauge");
            for (_, bucket, amount) in v.supply.iter().filter(|(d, _, _)| *d == denomination) {
                let _ = writeln!(
                    out,
                    "dytallix_app_supply_{denomination}{{bucket=\"{bucket}\"}} {amount}"
                );
            }
        }
        let _ = write!(
            out,
            "# TYPE {FORMAT} gauge\n{FORMAT}{{process=\"app\"}} {FORMAT_VERSION}\n\
             # TYPE {WRITTEN} gauge\n{WRITTEN}{{process=\"app\"}} {}.{:03}\n",
            now_unix_millis / 1000,
            now_unix_millis % 1000
        );
        out
    }

    /// Replace `dir/dytallix-app.prom` through a temporary file and a rename.
    pub fn write_file(&self, dir: &Path, snapshot: Option<SnapshotState>) -> Result<()> {
        ensure!(dir.is_absolute(), "Metrics directory must be absolute");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let text = self.render(u64::try_from(now)?, snapshot);
        let temporary = dir.join(format!(".{FILE_NAME}-{}", std::process::id()));
        {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(text.as_bytes())?;
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        }
        std::fs::rename(&temporary, dir.join(FILE_NAME))?;
        Ok(())
    }
}

/// The latest published snapshot under `dir`, from its metadata file.
pub fn latest_snapshot(dir: &Path) -> Option<SnapshotState> {
    let height = *crate::snapshot::published(dir).ok()?.last()?;
    let metadata = crate::snapshot::snapshot_dir(dir, height).join(crate::snapshot::METADATA_FILE);
    let written = std::fs::metadata(metadata)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(SnapshotState {
        height,
        written_unix_seconds: written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_writes_each_family_once_and_the_write_time() {
        let metrics = AppMetrics::new();
        metrics.startup_checked(0.25);
        metrics.block_executed(0.5);
        metrics.block_executed(1.0);
        let text = metrics.render(1_700_000_000_250, Some(SnapshotState {
            height: 20,
            written_unix_seconds: 1_699_999_990,
        }));
        for line in [
            "dytallix_app_height 0",
            "dytallix_app_startup_check_seconds 0.25",
            "dytallix_app_block_execution_seconds_sum 1.5",
            "dytallix_app_block_execution_seconds_count 2",
            "dytallix_app_snapshot_latest_height 20",
            "dytallix_app_block_records_pruned_total 0",
            "dytallix_metrics_written_timestamp_seconds{process=\"app\"} 1700000000.250",
            "dytallix_metrics_format_version{process=\"app\"} 1",
        ] {
            assert!(text.lines().any(|l| l == line), "missing {line} in\n{text}");
        }
        let types: Vec<_> = text.lines().filter(|l| l.starts_with("# TYPE")).collect();
        let mut unique = types.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(types.len(), unique.len());
        assert!(!metrics.render(0, None).contains("snapshot_latest"));
    }

    #[test]
    fn committed_activity_and_verifications_are_counted() {
        let metrics = AppMetrics::new();
        let empty = metrics.render(0, None);
        for line in [
            "dytallix_app_gas_used_total 0",
            "dytallix_app_validator_evidence_total 0",
            "# TYPE dytallix_app_transactions_total counter",
            "# TYPE dytallix_app_root_controls_total counter",
        ] {
            assert!(
                empty.lines().any(|l| l == line),
                "missing {line} in\n{empty}"
            );
        }
        metrics.block_activity(&BlockActivity {
            transactions: vec![
                ("ordinary", true),
                ("ordinary", false),
                ("root_control", true),
            ],
            gas_used: 70,
            evidence: 1,
            root_controls: vec!["kit_replacement"],
        });
        metrics.block_activity(&BlockActivity {
            transactions: vec![("ordinary", true)],
            gas_used: 30,
            ..BlockActivity::default()
        });
        helper_verified(true, Duration::from_millis(5));
        helper_verified(false, Duration::from_millis(5));
        let text = metrics.render(0, None);
        for line in [
            "dytallix_app_transactions_total{kind=\"ordinary\",result=\"ok\"} 2",
            "dytallix_app_transactions_total{kind=\"ordinary\",result=\"failed\"} 1",
            "dytallix_app_transactions_total{kind=\"root_control\",result=\"ok\"} 1",
            "dytallix_app_gas_used_total 100",
            "dytallix_app_validator_evidence_total 1",
            "dytallix_app_root_controls_total{kind=\"kit_replacement\"} 1",
        ] {
            assert!(text.lines().any(|l| l == line), "missing {line} in\n{text}");
        }
        // Both schemes are always present; the counters are process-wide.
        for scheme in ["ml_dsa_65", "slh_dsa_shake_256s"] {
            for result in ["valid", "invalid"] {
                let prefix = format!(
                    "dytallix_app_signature_verifications_total{{scheme=\"{scheme}\",result=\"{result}\"}} "
                );
                assert!(text.lines().any(|l| l.starts_with(&prefix)), "{prefix}");
            }
        }
        let helper_count = text
            .lines()
            .find_map(|l| {
                l.strip_prefix(
                    "dytallix_app_signature_verification_seconds_count{scheme=\"slh_dsa_shake_256s\"} ",
                )
            })
            .unwrap();
        assert!(helper_count.parse::<u64>().unwrap() >= 2);
        let types: Vec<_> = text.lines().filter(|l| l.starts_with("# TYPE")).collect();
        let mut unique = types.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(types.len(), unique.len());
    }

    #[test]
    fn write_file_replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let metrics = AppMetrics::new();
        metrics.write_file(dir.path(), None).unwrap();
        metrics.write_file(dir.path(), None).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [FILE_NAME]);
        assert!(metrics.write_file(Path::new("relative"), None).is_err());
    }
}
