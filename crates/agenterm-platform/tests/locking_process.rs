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
    let child_directory = directory.join("child");
    std::fs::create_dir(&child_directory).expect("create process-lock alias fixture");
    let path = directory.join("state.lock");
    let alias = child_directory.join("..").join("STATE.LOCK");
    let guard = PathLock::acquire(&path).expect("parent lock");
    run_test_child(&alias, "contended");
    drop(guard);
    run_test_child(&path, "available");
    run_test_child(&path, "exit-without-drop");
    PathLock::try_acquire(&path).expect("lock released when owner process exits");

    let unicode_path = directory.join("Å-state.lock");
    std::fs::write(&unicode_path, b"unicode-process-lock")
        .expect("create Unicode process-lock target");
    let unicode_alias = child_directory.join("..").join("å-STATE.LOCK");
    let unicode_guard = PathLock::acquire(&unicode_path).expect("Unicode parent lock");
    run_test_child(&unicode_alias, "contended");
    drop(unicode_guard);
    run_test_child(&unicode_alias, "available");

    let hard_link_path = directory.join("hard-link-original.lock");
    let hard_link_alias = directory.join("hard-link-alias.lock");
    std::fs::write(&hard_link_path, b"hard-link-process-lock")
        .expect("create hard-link process-lock target");
    std::fs::hard_link(&hard_link_path, &hard_link_alias)
        .expect("create hard-link process-lock alias");
    let hard_link_guard = PathLock::acquire(&hard_link_path).expect("hard-link parent lock");
    run_test_child(&hard_link_alias, "contended");
    drop(hard_link_guard);
    run_test_child(&hard_link_alias, "available");

    #[cfg(windows)]
    {
        let replacement = directory.join("replacement.lock");
        std::fs::write(&replacement, b"old-replacement")
            .expect("create replacement process-lock target");
        let replacement_guard = PathLock::acquire(&replacement).expect("replacement parent lock");
        std::fs::remove_file(&replacement).expect("remove locked replacement target");
        std::fs::write(&replacement, b"new-replacement")
            .expect("recreate replacement process-lock target");
        run_test_child(&replacement, "contended");
        drop(replacement_guard);
        run_test_child(&replacement, "available");
    }

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
        "exit-without-drop" => {
            let _guard = PathLock::acquire(path).expect("child acquires crash-release lock");
            std::process::exit(0);
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
