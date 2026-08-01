#![cfg(feature = "locking")]

use std::path::Path;
use std::process::Command;

use agenterm_platform::locking::{LockErrorKind, PathLock};

const CHILD_MODE: &str = "AGENTERM_PLATFORM_LOCK_CHILD";
const LOCK_PATH: &str = "AGENTERM_PLATFORM_LOCK_PATH";

#[test]
fn path_lock_is_cross_process_and_released() {
    if let Some(mode) = std::env::var_os(CHILD_MODE) {
        run_child(
            &mode,
            Path::new(&std::env::var_os(LOCK_PATH).expect("child lock path")),
        );
        return;
    }

    let directory = std::env::temp_dir().join(format!(
        "agenterm-platform-process-lock-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create process-lock fixture");
    let path = directory.join("state.lock");
    let guard = PathLock::acquire(&path).expect("parent lock");
    run_test_child(&path, "contended");
    drop(guard);
    run_test_child(&path, "available");
    std::fs::remove_dir_all(directory).expect("remove process-lock fixture");
}

fn run_child(mode: &std::ffi::OsStr, path: &Path) {
    match mode.to_str().expect("UTF-8 child mode") {
        "contended" => {
            let error = match PathLock::try_acquire(path) {
                Ok(_) => panic!("child acquired a lock held by its parent"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), LockErrorKind::Contended);
        }
        "available" => {
            PathLock::try_acquire(path).expect("child acquires released lock");
        }
        other => panic!("unknown lock child mode: {other}"),
    }
}

fn run_test_child(path: &Path, mode: &str) {
    let status = Command::new(std::env::current_exe().expect("integration test executable"))
        .args(["--exact", "path_lock_is_cross_process_and_released"])
        .env(CHILD_MODE, mode)
        .env(LOCK_PATH, path)
        .status()
        .expect("spawn lock child");
    assert!(status.success(), "lock child {mode} failed: {status}");
}
