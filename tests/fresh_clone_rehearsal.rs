use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn fresh_clone_rehearsal_policy_is_public_and_fail_closed() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = repo.join("scripts/rh/fresh-clone-rehearsal.rh");
    let manifest = repo.join("agenterm.tasks.json");
    // `--timeout-ms` is mandatory here: `output()` waits without a deadline, and
    // this task's declared budget is 3600s. When the self-test branch was missed
    // (a fabricated `args.len` selected the FULL rehearsal instead) this test
    // sat through a clone-and-build of the whole repository and took the windows
    // unit-tests gate down with it -- 900s+ with no output. A child that
    // misbehaves must now die in 120s and report `host_hard_timeout`, which is a
    // failure this test can print rather than a hang the gate absorbs.
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "rh"])
        .current_dir(repo)
        .args(["task", "run", "fresh-clone-rehearsal", "--manifest"])
        .arg(&manifest)
        .args(["--timeout-ms", "120000"])
        .args(["--json", "--", "--self-test"])
        .output()
        .expect("run fresh-clone rehearsal policy self-test");
    assert!(
        output.status.success(),
        "self-test failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse self-test task report");
    assert_eq!(report["ok"], true);
    assert_eq!(
        report["stdout"],
        "PASS: fresh-clone process policy fails closed\n"
    );

    let source = fs::read_to_string(&script).expect("read fresh-clone rehearsal");
    let manifest_text = fs::read_to_string(&manifest).expect("read manifest");
    assert!(manifest_text.contains("\"entry\": \"scripts/rh/fresh-clone-rehearsal.rh\""));
    assert!(!manifest_text.contains("\"entry\": \"scripts/rhai/fresh-clone-rehearsal.rhai\""));
    // See tests/rhai_migration.rs's matching comment: the individually
    // archived .rhai files were compacted into one tarball (b0045922) --
    // this asserts the archival trace still exists, not the loose-file
    // layout that predated the compaction.
    assert!(repo.join("scripts/archive/rhai-old.tgz").is_file());
    assert!(
        !repo
            .join("scripts/rhai/fresh-clone-rehearsal.rhai")
            .exists()
    );
    for contract in [
        "AGENTERM_FRESH_CLONE_REHEARSAL_ACTIVE",
        "fresh_clone_recursive_invocation",
        "release.arg(\"release.cmd\")",
        "release.arg(\"--rehearse\")",
        "AGENTERM_NO_ACTIVATE",
        "origin_restored",
        "origin_redacted_before_cleanup",
        "delivery_sequence",
        "terminal_compatibility_payloads",
        "automation_processes",
        "fresh_clone_untyped_powershell",
        "fresh_clone_terminal_payload_not_unique",
        "fresh_clone_owned_processes_remained",
        "remaining_after_cleanup",
        "child_deadline_ms",
        "command.capture_limit(262144)",
        "scan_interval_ms: 500",
        "fresh-clone-workspace",
        "unique_list.len > 2",
        "args.len == 2 && args[0] == \".\" && args[1] == \"--self-test\"",
        "debug/agenterm.exe",
        "list_has_value(names, \"agenterm.exe\")",
        "release_archive_path",
        "receipt.gates.len == expected_gate_count",
        "qualification_all_required",
        "receipt.provenance.git_head == head",
        "!receipt.provenance.source_dirty",
        "remote_refs_unchanged",
        "remove_owned_clone",
        "fresh-clone-rehearsal.json",
    ] {
        assert!(
            source.contains(contract),
            "fresh-clone rehearsal lost contract: {contract}"
        );
    }
}
