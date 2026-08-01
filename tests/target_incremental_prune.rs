use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const BUILD_TASK: &str = include_str!("../scripts/rhai/build.rhai");

fn fixture_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "agenterm-target-prune-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn base36(mut value: u128) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut encoded = Vec::new();
    loop {
        encoded.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    encoded.reverse();
    String::from_utf8(encoded).expect("base36 is ASCII")
}

fn old_timestamp(seconds_ago: u128) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_micros();
    base36(now - seconds_ago * 1_000_000)
}

fn initialize_fixture(name: &str) -> PathBuf {
    let root = fixture_root(name);
    fs::create_dir_all(root.join("target/debug/incremental")).expect("create target fixture");
    fs::write(root.join("target/debug/.cargo-lock"), b"").expect("create Cargo lock");
    let initialized = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "--quiet"])
        .output()
        .expect("initialize fixture repository");
    assert!(initialized.status.success());
    root
}

fn session(unit: &Path, timestamp: &str, random: &str, hash: &str) -> PathBuf {
    let path = unit.join(format!("s-{timestamp}-{random}-{hash}"));
    fs::create_dir_all(&path).expect("create session");
    fs::write(path.join("dep-graph.bin"), vec![b'x'; 4096]).expect("write session payload");
    path
}

fn session_lock(unit: &Path, timestamp: &str, random: &str) -> PathBuf {
    let path = unit.join(format!("s-{timestamp}-{random}.lock"));
    fs::write(&path, b"").expect("create session lock");
    path
}

fn run_prune(root: &Path) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-script"))
        .current_dir(repo)
        .args(["task", "run", "prune-target-incremental", "--manifest"])
        .arg(repo.join("agenterm.tasks.json"))
        .args([
            "--timeout-ms",
            "30000",
            "--max-operations",
            "10000000",
            "--",
        ])
        .arg(root)
        .arg(root.join("target"))
        .output()
        .expect("run incremental prune task")
}

fn open_locked(path: &Path) -> File {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open lock fixture");
    file.lock().expect("lock fixture");
    file
}

#[test]
fn development_build_prunes_only_after_successful_artifact_staging() {
    let stage = BUILD_TASK.find("\"build_stage\"").expect("stage call");
    let prune = BUILD_TASK
        .find("\"build_incremental_prune\"")
        .expect("prune call");
    assert!(
        prune > stage,
        "prune must follow successful artifact staging"
    );
    assert!(BUILD_TASK.contains("if profile == \"dev\" && !external_target"));
    assert!(BUILD_TASK.contains("\"task\", \"run\", \"prune-target-incremental\""));
}

#[test]
fn incremental_prune_keeps_each_units_newest_and_fail_closes_without_a_lock() {
    let root = initialize_fixture("generations");
    let incremental = root.join("target/debug/incremental");

    let complete = incremental.join("agenterm-complete");
    fs::create_dir(&complete).expect("create complete unit");
    let old_time = old_timestamp(180);
    let new_time = old_timestamp(120);
    let old = session(&complete, &old_time, "old", "oldhash");
    session_lock(&complete, &old_time, "old");
    let newest = session(&complete, &new_time, "new", "newhash");
    session_lock(&complete, &new_time, "new");

    let incomplete = incremental.join("agenterm-missing-lock");
    fs::create_dir(&incomplete).expect("create incomplete unit");
    let missing_time = old_timestamp(240);
    let retained_without_lock = session(&incomplete, &missing_time, "missing", "oldhash");
    let newest_time = old_timestamp(90);
    let incomplete_newest = session(&incomplete, &newest_time, "new", "newhash");
    session_lock(&incomplete, &newest_time, "new");

    let working = complete.join(format!("s-{}-work-working", old_timestamp(300)));
    fs::create_dir(&working).expect("create working session");

    let output = run_prune(&root);
    assert!(
        output.status.success(),
        "prune failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!old.exists(), "obsolete finalized session was retained");
    assert!(newest.exists(), "newest finalized session was removed");
    assert!(
        retained_without_lock.exists(),
        "session without its exact rustc lock was removed"
    );
    assert!(incomplete_newest.exists());
    assert!(working.exists(), "working session must never be pruned");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("removed_sessions=1"));
    assert!(stdout.contains("skipped_missing_lock=1"));

    fs::remove_dir_all(root).expect("remove generation fixture");
}

#[test]
fn incremental_prune_respects_cargo_and_rustc_locks_then_retries_after_drop() {
    let root = initialize_fixture("locks");
    let unit = root.join("target/debug/incremental/agenterm-locks");
    fs::create_dir(&unit).expect("create locked unit");
    let old_time = old_timestamp(180);
    let new_time = old_timestamp(120);
    let old = session(&unit, &old_time, "old", "oldhash");
    let old_lock_path = session_lock(&unit, &old_time, "old");
    let newest = session(&unit, &new_time, "new", "newhash");
    session_lock(&unit, &new_time, "new");

    let cargo_lock = open_locked(&root.join("target/debug/.cargo-lock"));
    let cargo_contended = run_prune(&root);
    assert!(cargo_contended.status.success());
    assert!(old.exists());
    assert!(String::from_utf8_lossy(&cargo_contended.stdout).contains(".cargo-lock is active"));
    drop(cargo_lock);

    let session_lock_guard = open_locked(&old_lock_path);
    let session_contended = run_prune(&root);
    assert!(session_contended.status.success());
    assert!(old.exists());
    assert!(String::from_utf8_lossy(&session_contended.stdout).contains("skipped_active_lock=1"));
    drop(session_lock_guard);

    let released = run_prune(&root);
    assert!(
        released.status.success(),
        "released prune failed: {}",
        String::from_utf8_lossy(&released.stderr)
    );
    assert!(!old.exists(), "released obsolete session was retained");
    assert!(newest.exists(), "newest session was removed after retry");

    fs::remove_dir_all(root).expect("remove lock fixture");
}
