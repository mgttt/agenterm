use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> Fixture {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "agenterm-performance-summary-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create fixture");
    Fixture { root }
}

fn write_timing(path: &Path, total_wall_ms: u64, fingerprint: &str) {
    let task_wall_ms = total_wall_ms - 1_200;
    let report = json!({
        "schema_version": 2,
        "kind": "agenterm-quality-timing",
        "lane": "quick",
        "profile": "quick",
        "status": "passed",
        "total_wall_ms": task_wall_ms,
        "bootstrap": {
            "schema_version": 1,
            "kind": "agenterm-bootstrap-timing",
            "state": "measured",
            "reason": null,
            "setup_ms": 1200,
            "cargo_build_ms": 900,
            "worker_copy_ms": 100,
            "other_setup_ms": 200,
            "clock_resolution_ms": 10,
            "worker": {
                "state": "rebuilt",
                "compatibility_fingerprint":
                    "0123456789abcdef0123456789abcdef01234567"
            },
            "cargo_lock_wait": {
                "state": "included_not_separable",
                "duration_ms": null,
                "included_in": "cargo_build_ms"
            }
        },
        "wall_time": {
            "state": "partial",
            "task_ms": task_wall_ms,
            "accounted_ms": total_wall_ms
        },
        "source": {"commit": SOURCE_SHA},
        "workload": {"fingerprint": fingerprint}
    });
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&report).expect("serialize timing")
        ),
    )
    .expect("write timing");
}

fn summarize(root: &Path, cache: &str, samples: &[PathBuf; 3]) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_agenterm-rhai"))
        .current_dir(repo)
        .arg("run")
        .arg(repo.join("scripts/rhai/performance-summary.rhai"))
        .args(["--project-root"])
        .arg(repo)
        .args(["--"])
        .arg(root.join("summary.json"))
        .arg("standard")
        .arg(cache)
        .args(samples)
        .args([
            root.join("sccache-1.json"),
            root.join("sccache-2.json"),
            root.join("sccache-3.json"),
        ])
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .expect("run performance summary")
}

#[test]
fn performance_summary_is_typed_and_rejects_mixed_workloads() {
    let fixture = fixture();
    let samples = [
        fixture.root.join("sample-1.json"),
        fixture.root.join("sample-2.json"),
        fixture.root.join("sample-3.json"),
    ];
    for (path, total) in samples.iter().zip([30_000, 10_000, 8_000]) {
        write_timing(path, total, "fnv1a64:fixture");
    }

    let accepted = summarize(&fixture.root, "target", &samples);
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let summary: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("summary.json")).expect("read summary"))
            .expect("parse summary");
    assert_eq!(summary["kind"], "agenterm-performance-experiment");
    assert_eq!(summary["schema_version"], 2);
    assert_eq!(summary["workload_scope"], "quick");
    assert_eq!(summary["cold_total_wall_ms"], 30_000);
    assert_eq!(summary["samples"][0]["task_wall_ms"], 28_800);
    assert_eq!(summary["samples"][0]["bootstrap"]["setup_ms"], 1_200);
    assert_eq!(summary["warm_median_total_wall_ms"], 9_000);
    assert_eq!(summary["warm_min_total_wall_ms"], 8_000);
    assert_eq!(summary["warm_max_total_wall_ms"], 10_000);
    assert_eq!(summary["warm_speedup_percent"], 70);
    assert_eq!(summary["sccache"]["available"], false);
    assert_eq!(
        summary["sample_model"],
        "cold_then_same_runner_incremental_warm"
    );

    write_timing(&samples[2], 8_000, "fnv1a64:different");
    let rejected = summarize(&fixture.root, "target", &samples);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("performance_summary_identity"));
}

#[test]
fn sccache_summary_requires_and_normalizes_raw_statistics() {
    let fixture = fixture();
    let samples = [
        fixture.root.join("sample-1.json"),
        fixture.root.join("sample-2.json"),
        fixture.root.join("sample-3.json"),
    ];
    for (path, total) in samples.iter().zip([30_000, 12_000, 10_000]) {
        write_timing(path, total, "fnv1a64:sccache");
    }
    for (ordinal, (hits, misses)) in [(0, 10), (8, 2), (9, 1)].into_iter().enumerate() {
        fs::write(
            fixture.root.join(format!("sccache-{}.json", ordinal + 1)),
            format!(
                "{}\n",
                serde_json::json!({
                    "stats": {
                        "compile_requests": hits + misses,
                        "cache_hits": {"Rust": hits},
                        "cache_misses": {"Rust": misses},
                        "cache_read_errors": 0,
                        "cache_write_errors": 0
                    }
                })
            ),
        )
        .expect("write sccache stats");
    }

    let accepted = summarize(&fixture.root, "sccache", &samples);
    assert!(
        accepted.status.success(),
        "{}{}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr)
    );
    let summary: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("summary.json")).expect("read summary"))
            .expect("parse summary");
    assert_eq!(summary["cache_namespace_scope"], "unique_per_workflow_run");
    assert_eq!(summary["sccache"]["available"], true);
    assert_eq!(summary["sccache"]["samples"][0]["cache_hits"], 0);
    assert_eq!(summary["sccache"]["samples"][1]["cache_hits"], 8);
    assert_eq!(summary["sccache"]["samples"][2]["cache_misses"], 1);
    assert!(
        summary["sccache"]["samples"][0]["raw_sha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
    );

    fs::remove_file(fixture.root.join("sccache-3.json")).expect("remove stats");
    let rejected = summarize(&fixture.root, "sccache", &samples);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("performance_summary_sccache_missing:3")
    );
}
