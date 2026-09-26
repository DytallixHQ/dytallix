//! Private directories for filesystem tests. Declare the guard before its users.
use std::path::{Path, PathBuf};

pub struct TestDirectory(PathBuf);
impl TestDirectory {
    pub fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("dytallix-core-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove test directory: {error}");
        }
    }
}
