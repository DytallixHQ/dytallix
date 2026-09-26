
fn diagnostic(before: &str, after: &str) -> serde_json::Value {
    let b = bounds();
    let maps = parse_maps(before.as_bytes(), &b).unwrap();
    serde_json::from_str(&mapping_diagnostic::summary(
        before.as_bytes(), after.as_bytes(), &maps, &b,
        &StartIdentity { pid: 42, start_ticks: 12345 },
        ObservationScope::OwnedChild, "application", Duration::from_millis(10),
    )).unwrap()
}

#[test]
fn mapping_diagnostic_identical_input_has_no_changes() {
    let d = diagnostic(MAPS, MAPS);
    assert_eq!(d["before_sha256"], d["after_sha256"]);
    assert_eq!(d["pid"], 42);
    assert_eq!(d["start_ticks"], 12345);
    assert_eq!(d["scope"], "owned_child");
    assert_eq!(d["second_parse_rejected"], false);
    assert_eq!(d["changes"]["changed_before"]["anonymous"], 0);
}

#[test]
fn mapping_diagnostic_classifies_same_start_range_change() {
    let d = diagnostic(MAPS, &MAPS.replace("3000-4000", "3000-3800"));
    assert_eq!(d["changes"]["same_start_range_changes"], 1);
    assert_eq!(d["changes"]["changed_before"]["anonymous"], 1);
    assert_eq!(d["changes"]["permission_changes"], 0);
    assert_ne!(d["before_sha256"], d["after_sha256"]);
}

#[test]
fn mapping_diagnostic_classifies_permission_and_backing_changes() {
    let d = diagnostic(MAPS, &MAPS.replace("2000-3000 r--p", "2000-3000 rw-p")
        .replace("08:01 100", "08:01 101"));
    assert_eq!(d["changes"]["permission_changes"], 1);
    assert_eq!(d["changes"]["backing_identity_changes"], 2);
    assert_eq!(d["changes"]["changed_before"]["file"], 2);
}

#[test]
fn mapping_diagnostic_counts_moved_start_as_removal_and_addition() {
    let d = diagnostic(MAPS, &MAPS.replace("3000-4000", "3100-4000"));
    assert_eq!(d["changes"]["removed"]["anonymous"], 1);
    assert_eq!(d["changes"]["added"]["anonymous"], 1);
    assert_eq!(d["changes"]["same_start_range_changes"], 0);
}

#[test]
fn mapping_diagnostic_rejected_second_input_has_no_partial_counts() {
    for after in ["malformed\n", "1000-2000 rwxp 0 00:00 0\n", ""] {
        let d = diagnostic(MAPS, after);
        assert_eq!(d["second_parse_rejected"], true);
        assert!(d["changes"].is_null());
        assert!(d["after_entries"].is_null());
        assert_eq!(d["identity_rechecked_after_change"], false);
    }
}

#[test]
fn mapping_diagnostic_output_is_bounded_and_omits_paths() {
    let b = bounds();
    let before = MAPS.replace("/fixture/app", &format!("/{}", "sensitive".repeat(300)));
    let after = before.replace("3000-4000", "3000-3800");
    let maps = parse_maps(before.as_bytes(), &b).unwrap();
    let text = mapping_diagnostic::summary(before.as_bytes(), after.as_bytes(), &maps, &b,
        &StartIdentity { pid: u32::MAX, start_ticks: u64::MAX },
        ObservationScope::CurrentProcess, &"\u{0000}".repeat(1000), Duration::MAX);
    assert!(text.len() < 4096);
    assert!(!text.contains("sensitive"));
    let d: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(d["role_truncated"], true);
    assert_eq!(d["raw_maps_emitted"], false);
    assert_eq!(d["deadline_exceeded_before_diagnostics"], true);
    assert_eq!(d["elapsed_ms_before_diagnostics"], u64::MAX);
}

#[test]
fn mapping_diagnostic_raw_only_change_still_has_distinct_hashes() {
    // The strict parser classifies both labels as anonymous. No zero-count
    // diagnostic can override the caller's raw-byte equality rejection.
    let d = diagnostic(MAPS, &MAPS.replace("[heap]", "[stack]"));
    assert_ne!(d["before_sha256"], d["after_sha256"]);
    assert_eq!(d["changes"]["changed_before"]["anonymous"], 0);
}
