//! The process working directory must follow the open workspace.
//!
//! This lives in its own test binary: changing the process working directory
//! from the parallel unit-test threads would race, so the real change is
//! exercised once, in isolation, and restored afterwards.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn scratch() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scico-working-directory-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("scratch workspace");
    path
}

#[test]
fn entering_a_workspace_changes_the_process_working_directory() {
    let workspace = scratch();
    let previous = std::env::current_dir().expect("previous working directory");

    let entered = scico::workspace::enter(Some(&workspace)).expect("enter workspace");
    assert_eq!(entered, workspace);
    assert_eq!(
        std::env::current_dir().expect("current directory"),
        workspace,
        "relative paths now resolve from the workspace"
    );

    std::env::set_current_dir(previous).expect("restore working directory");
    let _ = std::fs::remove_dir_all(workspace);
}
