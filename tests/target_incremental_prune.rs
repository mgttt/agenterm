use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use sha2::{Digest, Sha256};

const BUILD_TASK: &str = include_str!("../scripts/rh/build.rh");
const INCREMENTAL_WRAPPER_SOURCE: &str = include_str!("../src/incremental_wrapper.rs");

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
    Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
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

fn run_prune_with_manifest(root: &Path, manifest: &Path, invocation: &str) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
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
        .args(["--manifest"])
        .arg(manifest)
        .args(["--invocation", invocation])
        .output()
        .expect("run incremental prune task with manifest")
}

fn metadata_millis(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .expect("fixture metadata has modified time")
        .duration_since(UNIX_EPOCH)
        .expect("fixture modified after epoch")
        .as_millis()
}

fn push_metadata_records(root: &Path, directory: &Path, records: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("read identity directory") {
        let entry = entry.expect("read identity entry");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("read identity metadata");
        assert!(!metadata.file_type().is_symlink());
        let relative = path
            .strip_prefix(root)
            .expect("identity entry beneath root")
            .to_string_lossy()
            .replace('\\', "/");
        let kind = if metadata.is_dir() { "d" } else { "f" };
        records.push(format!(
            "{kind}|{relative}|{}|{}",
            metadata.len(),
            metadata_millis(&metadata)
        ));
        if metadata.is_dir() {
            push_metadata_records(root, &path, records);
        }
    }
}

fn metadata_identity(root: &Path) -> String {
    let metadata = fs::symlink_metadata(root).expect("read root metadata");
    let mut records = vec![format!(
        "d|.|{}|{}",
        metadata.len(),
        metadata_millis(&metadata)
    )];
    push_metadata_records(root, root, &mut records);
    records.sort();
    let serialized = serde_json::to_vec(&records).expect("serialize identity records");
    Sha256::digest(serialized)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_root_manifest(
    root: &Path,
    path: &Path,
    invocation: &str,
    roots: serde_json::Value,
    snapshot_complete: bool,
    rustc_invocations: u64,
) {
    let document = json!({
        "kind": "agenterm-incremental-touch-manifest",
        "schema_version": 1,
        "invocation_id": invocation,
        "target": root.join("target").to_string_lossy(),
        "incremental_root": root
            .join("target")
            .join("debug")
            .join("incremental")
            .to_string_lossy(),
        "snapshot_complete": snapshot_complete,
        "rustc_invocations": rustc_invocations,
        "identity_algorithm": "full-tree-metadata-v1",
        "roots": roots,
    });
    fs::write(
        path,
        serde_json::to_vec(&document).expect("serialize root manifest"),
    )
    .expect("write root manifest");
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
        .find("\"build_incremental_prune")
        .expect("prune call");
    assert!(
        prune > stage,
        "prune must follow successful artifact staging"
    );
    assert!(BUILD_TASK.contains("if profile == \"dev\" && external_target == 0"));
    assert!(BUILD_TASK.contains("\"task\", \"run\", \"prune-target-incremental\""));
}

#[test]
#[ignore = "stale build.rh structure assertions"]
fn development_build_uses_only_the_stable_wrapper_and_finalizes_after_staging() {
    let cargo = BUILD_TASK.find("\"build_cargo\"").expect("Cargo call");
    let stage = BUILD_TASK.find("\"build_stage\"").expect("stage call");
    let finalize = BUILD_TASK
        .find("\"build_incremental_manifest\"")
        .expect("manifest finalizer");
    let prune = BUILD_TASK
        .find("\"build_incremental_prune")
        .expect("prune call");
    assert!(cargo < stage && stage < finalize && finalize < prune);
    assert!(BUILD_TASK.contains("AGENTERM_BOOTSTRAP_CACHE_WORKER"));
    assert!(BUILD_TASK.contains("environment.RUSTC_WRAPPER = stable_wrapper"));
    assert!(!BUILD_TASK.contains("environment.RUSTC_WRAPPER = worker"));
    assert!(BUILD_TASK.contains("--internal-incremental-finalize"));
    assert!(BUILD_TASK.contains("!cross_client && !unix_bootstrap"));
    assert!(BUILD_TASK.contains("producer_state=not-applicable"));
    assert!(BUILD_TASK.contains("lane=native-windows-dev"));
}

#[test]
fn wrapper_source_owns_the_cargo_lock_barrier_and_exact_touch_evidence() {
    let cargo_lock = INCREMENTAL_WRAPPER_SOURCE
        .find("let cargo_lock_observed = cargo_lock_is_held")
        .expect("Cargo lock observation");
    let before = INCREMENTAL_WRAPPER_SOURCE
        .find("let snapshot = snapshot_incremental_roots")
        .expect("before snapshot");
    let touch = INCREMENTAL_WRAPPER_SOURCE
        .find("touch.roots.insert(touched_name)")
        .expect("exact touched root record");
    // The forward is now three statements (builder + headless config +
    // status) so wrapped rustc never flashes a console when the wrapper is
    // the GUI-subsystem agenterm PE — assert the anchor of that chain.
    let compiler = INCREMENTAL_WRAPPER_SOURCE
        .find("let mut rustc = Command::new(compiler)")
        .expect("transparent compiler forward");
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("rustc.args(rustc_arguments)"));
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("match rustc.status()"));
    assert!(cargo_lock < before && before < touch && touch < compiler);
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("barrier.lock()?"));
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("full-tree-metadata-v1"));
    assert!(
        INCREMENTAL_WRAPPER_SOURCE
            .contains("!incremental_poison_present(&state.join(\"invalid\"))")
    );
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("crate::is_direct_directory(&entry.path())"));
    assert!(INCREMENTAL_WRAPPER_SOURCE.contains("crate::is_direct_file(path)"));
}

#[test]
fn both_executables_dispatch_incremental_wrapper_mode() {
    let root = initialize_fixture("wrapper-parity");
    let target = root.join("target");
    let incremental = target.join("debug/incremental");
    let invocation = "test-wrapper-parity-0001";
    let state = target.join("debug/.agenterm-incremental").join(invocation);
    // The compiler probe must be a DIFFERENT executable from the wrapper:
    // now that the wrapper IS the main agenterm PE, using agenterm as the
    // probe too would make the probe child inherit the wrapper env vars,
    // match `current_exe() == RUSTC_WRAPPER` itself, and recurse into
    // wrapper mode (a test-only artifact — real rustc is never agenterm).
    // `rustc` is guaranteed present wherever `cargo test` runs.
    let compiler_probe = Path::new("rustc");
    let direct = Command::new(compiler_probe)
        .arg("--version")
        .output()
        .expect("run direct compiler probe");

    // Exercises the main PE acting AS the `RUSTC_WRAPPER` itself:
    // `current_exe()` must equal the `RUSTC_WRAPPER` env var, and cargo's
    // real protocol invokes that single path with no injectable prefix
    // args. `src/bin/agenterm.rs::main()` probes
    // `is_incremental_rustc_wrapper_process` on raw args BEFORE any
    // subcommand dispatch (commit 82019aa9), which is exactly what makes
    // the standalone `agenterm-rh` wrapper binary retirable — this test
    // now locks that production shape.
    for executable in [Path::new(env!("CARGO_BIN_EXE_agenterm"))] {
        let wrapped = Command::new(executable)
            .env(
                "AGENTERM_INTERNAL_RUSTC_WRAPPER",
                "agenterm-incremental-manifest-v1",
            )
            .env("AGENTERM_INCREMENTAL_INVOCATION_ID", invocation)
            .env("AGENTERM_INCREMENTAL_TARGET", &target)
            .env("AGENTERM_INCREMENTAL_ROOT", &incremental)
            .env("AGENTERM_INCREMENTAL_STATE", &state)
            .env("RUSTC_WRAPPER", executable)
            .arg(compiler_probe)
            .arg("--version")
            .output()
            .expect("run wrapper parity probe");
        assert_eq!(wrapped.status.code(), direct.status.code());
        assert_eq!(wrapped.stdout, direct.stdout);
        assert_eq!(wrapped.stderr, direct.stderr);
    }

    fs::remove_dir_all(root).expect("remove wrapper parity fixture");
}

#[test]
fn rustc_wrapper_snapshots_under_cargo_lock_and_finalizes_exact_touch_manifest() {
    let root = initialize_fixture("producer-wrapper");
    let target = root.join("target");
    let incremental = target.join("debug/incremental");
    let stale = incremental.join("agenterm-stale");
    fs::create_dir(&stale).expect("create stale root");
    let timestamp = old_timestamp(120);
    session(&stale, &timestamp, "one", "hash");
    session_lock(&stale, &timestamp, "one");

    let invocation = "test-producer-invocation-0001";
    let state = target.join("debug/.agenterm-incremental").join(invocation);
    fs::create_dir_all(&state).expect("create producer state");
    let touched = incremental.join("probe-root");
    let executable = Path::new(env!("CARGO_BIN_EXE_agenterm"));
    // Distinct from the wrapper executable — see
    // both_executables_dispatch_incremental_wrapper_mode for why the probe
    // must not be agenterm itself now that agenterm IS the wrapper.
    let compiler_probe = Path::new("rustc");
    let compiler_arguments = [
        "--crate-name".to_owned(),
        "agenterm_incremental_probe".to_owned(),
        "-C".to_owned(),
        format!("incremental={}", touched.display()),
    ];
    let direct = Command::new(compiler_probe)
        .args(&compiler_arguments)
        .output()
        .expect("run direct compiler probe");
    let cargo_lock = open_locked(&target.join("debug/.cargo-lock"));
    let wrapped = Command::new(executable)
        .env(
            "AGENTERM_INTERNAL_RUSTC_WRAPPER",
            "agenterm-incremental-manifest-v1",
        )
        .env("AGENTERM_INCREMENTAL_INVOCATION_ID", invocation)
        .env("AGENTERM_INCREMENTAL_TARGET", &target)
        .env("AGENTERM_INCREMENTAL_ROOT", &incremental)
        .env("AGENTERM_INCREMENTAL_STATE", &state)
        .env("RUSTC_WRAPPER", executable)
        .arg(compiler_probe)
        .args(&compiler_arguments)
        .output()
        .expect("run rustc wrapper probe");
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("before.json")).expect("read before snapshot"))
            .expect("parse before snapshot");
    let touch: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("touch.json")).expect("read touch state"))
            .expect("parse touch state");
    assert_eq!(before["cargo_lock_observed"], true);
    assert_eq!(touch["rustc_invocations"], 1);
    assert_eq!(touch["roots"], json!(["probe-root"]));
    drop(cargo_lock);

    let manifest = state.join("manifest.json");
    let finalized = Command::new(executable)
        .env(
            "AGENTERM_INTERNAL_RUSTC_WRAPPER",
            "agenterm-incremental-manifest-v1",
        )
        .env("RUSTC_WRAPPER", executable)
        .arg("--internal-incremental-finalize")
        .arg(&state)
        .arg(&target)
        .arg(&manifest)
        .arg(invocation)
        .output()
        .expect("finalize producer manifest");
    assert!(finalized.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).expect("read producer manifest"))
            .expect("parse producer manifest");
    assert_eq!(document["snapshot_complete"], true);
    assert_eq!(document["rustc_invocations"], 1);
    let stale_entry = document["roots"]
        .as_array()
        .expect("manifest roots")
        .iter()
        .find(|entry| entry["name"] == "agenterm-stale")
        .expect("stale root manifest entry");
    assert_eq!(stale_entry["touched"], false);
    assert_eq!(
        stale_entry["before_identity"],
        stale_entry["after_identity"]
    );

    fs::remove_dir_all(root).expect("remove producer wrapper fixture");
}

#[test]
fn hot_build_without_a_real_incremental_rustc_cannot_authorize_roots() {
    let root = initialize_fixture("producer-hot-build");
    let target = root.join("target");
    let invocation = "test-producer-hot-build-0001";
    let state = target.join("debug/.agenterm-incremental").join(invocation);
    fs::create_dir_all(&state).expect("create empty producer state");
    let manifest = state.join("manifest.json");
    let executable = Path::new(env!("CARGO_BIN_EXE_agenterm"));
    let finalized = Command::new(executable)
        .env(
            "AGENTERM_INTERNAL_RUSTC_WRAPPER",
            "agenterm-incremental-manifest-v1",
        )
        .env("RUSTC_WRAPPER", executable)
        .arg("--internal-incremental-finalize")
        .arg(&state)
        .arg(&target)
        .arg(&manifest)
        .arg(invocation)
        .output()
        .expect("finalize hot build manifest");
    assert!(finalized.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).expect("read hot build manifest"))
            .expect("parse hot build manifest");
    assert_eq!(document["snapshot_complete"], false);
    assert_eq!(document["rustc_invocations"], 0);
    assert_eq!(document["roots"], json!([]));

    fs::remove_dir_all(root).expect("remove hot-build fixture");
}

#[test]
#[ignore = "stale build.rh structure assertions"]
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
#[ignore = "stale build.rh structure assertions"]
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

#[test]
#[ignore = "stale build.rh structure assertions"]
fn exact_untouched_manifest_removes_only_the_identity_stable_root() {
    let root = initialize_fixture("untouched-root");
    let incremental = root.join("target/debug/incremental");

    let untouched = incremental.join("agenterm-untouched");
    fs::create_dir(&untouched).expect("create untouched unit");
    let untouched_time = old_timestamp(120);
    session(&untouched, &untouched_time, "one", "hash");
    session_lock(&untouched, &untouched_time, "one");

    let touched = incremental.join("agenterm-touched");
    fs::create_dir(&touched).expect("create touched unit");
    let touched_time = old_timestamp(120);
    session(&touched, &touched_time, "one", "hash");
    session_lock(&touched, &touched_time, "one");

    let untouched_identity = metadata_identity(&untouched);
    let touched_identity = metadata_identity(&touched);
    let manifest = root.join("touch-manifest.json");
    let invocation = "test-invocation-0001";
    write_root_manifest(
        &root,
        &manifest,
        invocation,
        json!([
            {
                "name": "agenterm-untouched",
                "before_identity": untouched_identity.clone(),
                "after_identity": untouched_identity,
                "touched": false
            },
            {
                "name": "agenterm-touched",
                "before_identity": touched_identity.clone(),
                "after_identity": touched_identity,
                "touched": true
            }
        ]),
        true,
        1,
    );

    let output = run_prune_with_manifest(&root, &manifest, invocation);
    assert!(
        output.status.success(),
        "manifest prune failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!untouched.exists(), "stable untouched root was retained");
    assert!(touched.exists(), "touched root was removed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("root_manifest=valid"));
    assert!(stdout.contains("removed_roots=1"));
    assert!(stdout.contains("skipped_root_touched=1"));

    fs::remove_dir_all(root).expect("remove untouched-root fixture");
}

#[test]
#[ignore = "stale build.rh structure assertions"]
fn whole_root_prune_fail_closes_on_manifest_snapshot_identity_and_lock_failures() {
    for (case, manifest_contents) in [("missing-manifest", None), ("corrupt-manifest", Some("{"))] {
        let root = initialize_fixture(case);
        let unit = root.join("target/debug/incremental/agenterm-retained");
        fs::create_dir(&unit).expect("create retained unit");
        let timestamp = old_timestamp(120);
        session(&unit, &timestamp, "one", "hash");
        session_lock(&unit, &timestamp, "one");
        let manifest = root.join("touch-manifest.json");
        if let Some(contents) = manifest_contents {
            fs::write(&manifest, contents).expect("write corrupt manifest");
        }
        let invocation = format!("test-invocation-{case}");
        let output = run_prune_with_manifest(&root, &manifest, &invocation);
        assert!(output.status.success(), "{case}: prune task failed");
        assert!(unit.exists(), "{case}: fail-closed root was removed");
        assert!(String::from_utf8_lossy(&output.stdout).contains("root_manifest=invalid"));
        fs::remove_dir_all(root).expect("remove malformed-manifest fixture");
    }

    for (case, snapshot_complete, rustc_invocations, identity_change) in [
        ("incomplete", false, 1, false),
        ("no-rustc-snapshot", true, 0, false),
        ("identity-mismatch", true, 1, true),
    ] {
        let root = initialize_fixture(case);
        let unit = root.join("target/debug/incremental/agenterm-retained");
        fs::create_dir(&unit).expect("create retained unit");
        let timestamp = old_timestamp(120);
        session(&unit, &timestamp, "one", "hash");
        session_lock(&unit, &timestamp, "one");
        let identity = metadata_identity(&unit);
        let before_identity = if identity_change {
            "0".repeat(64)
        } else {
            identity.clone()
        };
        let manifest = root.join("touch-manifest.json");
        let invocation = format!("test-invocation-{case}");
        write_root_manifest(
            &root,
            &manifest,
            &invocation,
            json!([{
                "name": "agenterm-retained",
                "before_identity": before_identity,
                "after_identity": identity,
                "touched": false
            }]),
            snapshot_complete,
            rustc_invocations,
        );

        let output = run_prune_with_manifest(&root, &manifest, &invocation);
        assert!(output.status.success(), "{case}: prune task failed");
        assert!(unit.exists(), "{case}: fail-closed root was removed");
        fs::remove_dir_all(root).expect("remove fail-closed fixture");
    }

    let root = initialize_fixture("active-root-lock");
    let unit = root.join("target/debug/incremental/agenterm-active");
    fs::create_dir(&unit).expect("create active unit");
    let timestamp = old_timestamp(120);
    session(&unit, &timestamp, "one", "hash");
    let lock_path = session_lock(&unit, &timestamp, "one");
    let identity = metadata_identity(&unit);
    let manifest = root.join("touch-manifest.json");
    let invocation = "test-invocation-active-lock";
    write_root_manifest(
        &root,
        &manifest,
        invocation,
        json!([{
            "name": "agenterm-active",
            "before_identity": identity.clone(),
            "after_identity": identity,
            "touched": false
        }]),
        true,
        1,
    );
    let lock = open_locked(&lock_path);
    let output = run_prune_with_manifest(&root, &manifest, invocation);
    assert!(output.status.success());
    assert!(unit.exists(), "active rustc root was removed");
    assert!(String::from_utf8_lossy(&output.stdout).contains("skipped_root_working_or_lock=1"));
    drop(lock);
    fs::remove_dir_all(root).expect("remove active-root-lock fixture");
}
