use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn fresh_clone_rehearsal_policy_is_public_and_fail_closed() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = repo.join("scripts/rhai/fresh-clone-rehearsal.rhai");
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(repo)
        .args(["run"])
        .arg(&script)
        .args(["--", "--self-test"])
        .output()
        .expect("run fresh-clone rehearsal policy self-test");
    assert!(
        output.status.success(),
        "self-test failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "PASS: fresh-clone process policy fails closed"
    );

    let source = fs::read_to_string(script).expect("read fresh-clone rehearsal");
    for contract in [
        "AGENTERM_FRESH_CLONE_REHEARSAL_ACTIVE",
        "fresh_clone_recursive_invocation",
        "release.cmd\", \"--rehearse",
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
        "rhai::runtime::temp_dir()",
        "unique.keys().len > 2",
        "release_archive_path",
        "receipt.gates.len == 34",
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
