//! Exact process security admission. This supplements retained ownership and
//! code/mapping observations; it never grants release authority.
use crate::observation::OwnedProcessIdentity;
use anyhow::{ensure, Context, Result};
use std::{fs::File, io::Read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityState {
    pub uid: u32,
    pub gid: u32,
    pub apparmor_label: String,
    pub mount_namespace: u64,
}
fn read_bounded(path: &str, limit: usize) -> Result<String> {
    let mut raw = Vec::new();
    File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut raw)?;
    ensure!(
        !raw.is_empty() && raw.len() <= limit,
        "Security record size invalid"
    );
    Ok(String::from_utf8(raw)?)
}
fn start(pid: u32) -> Result<u64> {
    let raw = read_bounded(&format!("/proc/{pid}/stat"), 8192)?;
    let prefix = format!("{pid} (");
    ensure!(raw.starts_with(&prefix), "Security record PID differs");
    let (_, tail) = raw
        .rsplit_once(") ")
        .context("Process stat delimiter missing")?;
    let ticks: u64 = tail
        .split_whitespace()
        .nth(19)
        .context("Start time missing")?
        .parse()?;
    ensure!(ticks > 0, "Invalid start time");
    Ok(ticks)
}
fn parse(status: &str, attr: &str, mount: &str) -> Result<SecurityState> {
    fn field<'a>(status: &'a str, name: &str) -> Result<&'a str> {
        let mut rows = status.lines().filter_map(|l| l.strip_prefix(name));
        let value = rows.next().context("Security status field missing")?;
        ensure!(rows.next().is_none(), "Duplicate security status field");
        Ok(value.trim())
    }
    fn ids(status: &str, key: &str) -> Result<u32> {
        let values = field(status, key)?
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            values.len() == 4 && values[0] != 0 && values.iter().all(|v| *v == values[0]),
            "Process identity mismatch"
        );
        Ok(values[0])
    }
    ensure!(
        field(status, "NoNewPrivs:")? == "1" && field(status, "Seccomp:")? == "2",
        "Required NNP or seccomp state missing"
    );
    let line = attr.strip_suffix('\n').unwrap_or(attr);
    let label = line
        .strip_suffix(" (enforce)")
        .context("AppArmor enforce label required")?;
    ensure!(
        !label.is_empty()
            && label.len() <= 256
            && !label.bytes().any(|b| b.is_ascii_whitespace() || b == 0),
        "Invalid AppArmor label"
    );
    let raw = mount
        .strip_prefix("mnt:[")
        .and_then(|v| v.strip_suffix(']'))
        .context("Mount namespace identity invalid")?;
    let mount_namespace: u64 = raw.parse()?;
    ensure!(
        mount_namespace > 0 && mount_namespace.to_string() == raw,
        "Noncanonical mount namespace"
    );
    Ok(SecurityState {
        uid: ids(status, "Uid:")?,
        gid: ids(status, "Gid:")?,
        apparmor_label: label.to_owned(),
        mount_namespace,
    })
}
fn capture(pid: u32, expected_start: Option<u64>) -> Result<SecurityState> {
    ensure!(
        cfg!(target_os = "linux") && pid > 0,
        "Linux owned process security required"
    );
    let before = start(pid)?;
    if let Some(expected) = expected_start {
        ensure!(before == expected, "Owned process start identity differs");
    }
    let status = read_bounded(&format!("/proc/{pid}/status"), 65536)?;
    let attr = read_bounded(&format!("/proc/{pid}/attr/current"), 512)?;
    let mount = std::fs::read_link(format!("/proc/{pid}/ns/mnt"))?;
    let value = parse(
        &status,
        &attr,
        mount.to_str().context("Mount namespace is not UTF-8")?,
    )?;
    ensure!(
        before == start(pid)?,
        "Process identity changed during security admission"
    );
    Ok(value)
}
pub fn capture_current() -> Result<SecurityState> {
    capture(std::process::id(), None)
}
impl SecurityState {
    pub fn validate_exact(&self, label: &str, uid: u32, gid: u32) -> Result<()> {
        ensure!(
            self.apparmor_label == label
                && self.uid == uid
                && self.gid == gid
                && self.mount_namespace > 0,
            "Expected process security differs"
        );
        Ok(())
    }
    pub fn check_current(&self) -> Result<()> {
        ensure!(
            *self == capture_current()?,
            "Current security state changed"
        );
        Ok(())
    }
    pub fn check_owned(&self, identity: &OwnedProcessIdentity) -> Result<()> {
        let start = identity.start();
        ensure!(
            *self == capture(start.pid, Some(start.start_ticks))?,
            "Owned process security differs"
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn status() -> &'static str {
        "Uid:\t42\t42\t42\t42\nGid:\t43\t43\t43\t43\nNoNewPrivs:\t1\nSeccomp:\t2\n"
    }
    #[test]
    fn helper_role_labels_are_exact_and_same_unit() {
        let s = format!("dyt-role-{}-node0-supervisor", "a".repeat(20));
        let l = format!("{s}//&dyt-role-{}-node0-workload", "a".repeat(20));
        validate_role_labels(&s, &l).unwrap();
        HelperSecurityPolicy {
            supervisor_label: s.clone(),
            application_owner_label: format!("dyt-role-{}-node0-application-owner//&{s}", "a".repeat(20)),
            workload_label: l.clone(),
            helper_label: format!("dyt-role-{}-node0-application-owner//&dyt-role-{}-node0-helper//&{s}", "a".repeat(20), "a".repeat(20)),
            uid: 42,
            gid: 43,
        }
        .validate()
        .unwrap();
        for bad in [
            l.replace("node0-workload", "node1-workload"),
            l.replace("//&", "&"),
            format!("{l}//&extra"),
            "unconfined".to_owned(),
        ] {
            assert!(validate_role_labels(&s, &bad).is_err());
        }
        assert!(validate_role_labels("S", "S//&W").is_err());
        assert!(HelperSecurityPolicy {
            supervisor_label: s.clone(),
            application_owner_label: format!("dyt-role-{}-node0-application-owner//&{s}", "a".repeat(20)),
            workload_label: l,
            helper_label: "invalid".into(),
            uid: 0,
            gid: 43
        }
        .validate()
        .is_err());
    }
    #[test]
    fn strict_security_parser() {
        let s = parse(status(), "S//&W (enforce)\n", "mnt:[99]").unwrap();
        s.validate_exact("S//&W", 42, 43).unwrap();
        assert!(s.validate_exact("W", 42, 43).is_err());
        for attr in [
            "S//&W (complain)\n",
            "S//&W\n",
            "S//&W (enforce)\n\n",
            " (enforce)",
        ] {
            assert!(parse(status(), attr, "mnt:[99]").is_err());
        }
        for mount in ["mnt:[0]", "mnt:[099]", "net:[99]", "mnt:[99]\n"] {
            assert!(parse(status(), "S (enforce)", mount).is_err());
        }
        for bad in [
            status().replace("NoNewPrivs:\t1", "NoNewPrivs:\t0"),
            status().replace("Seccomp:\t2", "Seccomp:\t0"),
            status().replace("42\t42\t42\t42", "42\t43\t42\t42"),
            status().replace("43\t43\t43\t43", "0\t0\t0\t0"),
            format!("{}Seccomp:\t2\n", status()),
        ] {
            assert!(parse(&bad, "S (enforce)", "mnt:[99]").is_err());
        }
    }
    #[test]
    fn profile_inventory_requires_exact_enforce_entries() {
        let labels = ["unit-supervisor", "unit-application-owner", "unit-workload", "unit-helper"];
        let raw = labels.iter().map(|label| format!("{label} (enforce)\n")).collect::<String>();
        validate_profile_inventory(&raw, &labels).unwrap();
        for invalid in [
            raw.replace("unit-helper (enforce)", "unit-helper (complain)"),
            raw.replace("unit-helper (enforce)\n", ""),
            raw.replace("unit-helper (enforce)", "unit-helper-alt (enforce)"),
        ] {
            assert!(validate_profile_inventory(&invalid, &labels).is_err());
        }
        assert!(validate_profile_inventory(&raw,
            &[labels[0], labels[1], labels[2], labels[2]]).is_err());
    }
}

/// Canonical role pair. Labels are exact identities, never patterns or aliases.
pub fn validate_role_labels(supervisor: &str, workload: &str) -> Result<()> {
    let body = supervisor
        .strip_prefix("dyt-role-")
        .and_then(|v| v.strip_suffix("-supervisor"))
        .context("Supervisor role label invalid")?;
    let (digest, unit) = body
        .split_once('-')
        .context("Role label identity missing")?;
    ensure!(
        digest.len() == 20
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Role label digest invalid"
    );
    ensure!(
        !unit.is_empty()
            && unit.len() <= 32
            && unit
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
        "Role unit invalid"
    );
    let overlay = format!("dyt-role-{body}-workload");
    ensure!(
        workload == format!("{supervisor}//&{overlay}"),
        "Exact retained supervisor/workload label required"
    );
    Ok(())
}
/// Validate four distinct, same-unit AppArmor labels.
pub fn validate_four_role_labels(
    supervisor: &str,
    application_owner: &str,
    workload: &str,
    helper: &str,
) -> Result<()> {
    let body = supervisor
        .strip_prefix("dyt-role-")
        .and_then(|value| value.strip_suffix("-supervisor"))
        .context("Supervisor role label invalid")?;
    let legacy_workload = format!("{supervisor}//&dyt-role-{body}-workload");
    validate_role_labels(supervisor, &legacy_workload)?;
    let component = |role: &str| format!("dyt-role-{body}-{role}");
    ensure!(
        application_owner == format!("{}//&{supervisor}", component("application-owner"))
            && workload == legacy_workload
            && helper == format!("{}//&{}//&{supervisor}",
                component("application-owner"), component("helper")),
        "Exact additive four-role labels required"
    );
    Ok(())
}

/// Read fresh kernel state. A rendered profile list is not enforcement evidence.
pub fn require_enforced_profiles(labels: &[&str]) -> Result<()> {
    ensure!(cfg!(target_os = "linux") && labels.len() == 4,
        "Four Linux AppArmor profiles required");
    validate_four_role_labels(labels[0], labels[1], labels[2], labels[3])?;
    let body = labels[0]
        .strip_prefix("dyt-role-")
        .and_then(|value| value.strip_suffix("-supervisor"))
        .context("Supervisor role label invalid")?;
    let application = format!("dyt-role-{body}-application-owner");
    let workload = format!("dyt-role-{body}-workload");
    let helper = format!("dyt-role-{body}-helper");
    let raw = read_bounded("/sys/kernel/security/apparmor/profiles", 4 * 1024 * 1024)?;
    validate_profile_inventory(&raw, &[labels[0], &application, &workload, &helper])
}
fn validate_profile_inventory(raw: &str, labels: &[&str]) -> Result<()> {
    ensure!(labels.len() == 4, "Four profile labels required");
    let loaded: std::collections::BTreeSet<&str> = raw.lines().collect();
    let unique: std::collections::BTreeSet<&str> = labels.iter().copied().collect();
    ensure!(unique.len() == 4, "Duplicate required AppArmor profile");
    for label in labels {
        ensure!(loaded.contains(format!("{label} (enforce)").as_str()),
            "Required AppArmor profile absent or not enforced: {label}");
    }
    Ok(())
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct HelperSecurityPolicy {
    pub supervisor_label: String,
    pub application_owner_label: String,
    pub workload_label: String,
    pub helper_label: String,
    pub uid: u32,
    pub gid: u32,
}
impl HelperSecurityPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.uid > 0 && self.gid > 0,
            "Non-root helper identity required"
        );
        validate_four_role_labels(&self.supervisor_label, &self.application_owner_label,
            &self.workload_label, &self.helper_label)
    }
    pub fn expected_child(&self) -> Result<SecurityState> {
        self.validate()?;
        let mut current = capture_current()?;
        ensure!(current.apparmor_label == self.application_owner_label,
            "Application owner role required for helper");
        current.validate_exact(&current.apparmor_label, self.uid, self.gid)?;
        current.apparmor_label = self.helper_label.clone();
        Ok(current)
    }
}
