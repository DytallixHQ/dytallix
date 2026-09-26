//! The diagnostic feature must never reopen the supported timer executable.
#[cfg(any(feature = "pqc-real", feature = "pqc-fips204"))]
#[test]
fn timer_executable_rejects_before_data_directory_creation() {
    let directory = tempfile::tempdir().unwrap();
    let data_directory = directory.path().join("must-not-exist");
    for staking in ["0", "1"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_dytallix-fast-node"))
            .current_dir(directory.path())
            .env("DYT_DATA_DIR", &data_directory)
            .env("DYT_BLOCK_PROFILE", "development")
            .env("DYT_ENABLE_GOVERNANCE", "0")
            .env("DYT_ENABLE_DEV_ENDPOINTS", "0")
            .env("DYT_ENABLE_STAKING", staking)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Legacy node timer is retired"), "{error}");
        assert!(!data_directory.exists());
    }
}
