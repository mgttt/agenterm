#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const OUTPUT_ITERATIONS: usize = 8192;
const OUTPUT_CHUNK_BYTES: usize = 16 * 255 + 2;
const OUTPUT_BYTES: u64 = (OUTPUT_ITERATIONS * OUTPUT_CHUNK_BYTES) as u64;
const MIN_BYTES_PER_SECOND: u64 = 4 * 1024 * 1024;
const OUTPUT_DEADLINE: Duration = Duration::from_secs(30);
const SIBLING_DEADLINE: Duration = Duration::from_secs(5);

static UNIQUE: AtomicU64 = AtomicU64::new(1);

struct OwnedGui(Child);

impl Drop for OwnedGui {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must follow Unix epoch")
        .as_nanos();
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

fn agenterm_con_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable path");
    path.pop();
    path.pop();
    path.push(format!("agenterm-con{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "agenterm-con is missing at {}",
        path.display()
    );
    path
}

fn invoke(exe: &Path, endpoint: &str, arguments: &[&str]) -> Output {
    Command::new(exe)
        .args(["cli", "--control", endpoint])
        .args(arguments)
        .output()
        .expect("agenterm-con CLI must start")
}

fn error_text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn cli_json(exe: &Path, endpoint: &str, arguments: &[&str]) -> Value {
    let output = invoke(exe, endpoint, arguments);
    assert!(
        output.status.success(),
        "CLI failed: {}",
        error_text(&output)
    );
    serde_json::from_slice(&output.stdout).expect("successful CLI output must be JSON")
}

fn wait_until_ready(exe: &Path, endpoint: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let output = invoke(exe, endpoint, &["list-tabs"]);
        if output.status.success() {
            return serde_json::from_slice(&output.stdout).expect("list-tabs output must be JSON");
        }
        assert!(
            Instant::now() < deadline,
            "control endpoint did not become ready: {}",
            error_text(&output)
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn tab_id(value: &Value) -> &str {
    value.as_str().expect("tab ID must be a string")
}

#[test]
fn sustained_long_output_keeps_control_and_sibling_responsive() {
    let exe = agenterm_con_binary();
    let exe = exe.as_path();
    let endpoint = format!(r"pipe:\\.\pipe\agenterm-con-throughput-{}", unique_suffix());
    let child = Command::new(exe)
        .args([
            "--no-activate",
            "--control",
            &endpoint,
            "-e",
            "powershell.exe",
            "-NoLogo",
            "-NoProfile",
            "-NoExit",
            "-Command",
            "[Console]::Out.WriteLine('THROUGHPUT_READY')",
        ])
        .spawn()
        .expect("agenterm-con GUI must start");
    let mut gui = OwnedGui(child);

    let listed = wait_until_ready(exe, &endpoint, Duration::from_secs(15));
    let producer = tab_id(&listed["tabs"][0]["id"]).to_owned();
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &producer,
            "--timeout-ms",
            "10000",
            "THROUGHPUT_READY",
        ],
    );
    let created = cli_json(exe, &endpoint, &["new-tab", "--parent", &producer]);
    let sibling = tab_id(&created["id"]).to_owned();
    cli_json(exe, &endpoint, &["reset-perf-stats"]);

    let command = format!(
        "$b=[Text.Encoding]::ASCII.GetBytes(((('0123456789ABCDEF'*255)+\"`r`n\")*{OUTPUT_ITERATIONS}));\
         $o=[Console]::OpenStandardOutput();\
         $o.Write($b,0,$b.Length);\
         [Console]::Out.WriteLine(('THROUGHPUT_'+'DONE_32M'))\r"
    );
    let started = Instant::now();
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &producer, &command],
    );

    let sibling_started = Instant::now();
    cli_json(
        exe,
        &endpoint,
        &[
            "send-text",
            "--target",
            &sibling,
            "echo SIBLING_RESPONSIVE\r",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &sibling,
            "--timeout-ms",
            "5000",
            "SIBLING_RESPONSIVE",
        ],
    );
    assert!(
        sibling_started.elapsed() <= SIBLING_DEADLINE,
        "sibling response exceeded {SIBLING_DEADLINE:?} during sustained output"
    );

    for _ in 0..8 {
        let control_started = Instant::now();
        cli_json(exe, &endpoint, &["list-tabs"]);
        cli_json(exe, &endpoint, &["perf-stats"]);
        assert!(
            control_started.elapsed() <= Duration::from_secs(2),
            "control observation stalled during sustained output"
        );
    }
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &producer,
            "--timeout-ms",
            "30000",
            "THROUGHPUT_DONE_32M",
        ],
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed <= OUTPUT_DEADLINE,
        "{OUTPUT_BYTES} output bytes exceeded {OUTPUT_DEADLINE:?}: {elapsed:?}"
    );
    let bytes_per_second = OUTPUT_BYTES.saturating_mul(1_000_000_000)
        / elapsed.as_nanos().max(1).min(u128::from(u64::MAX)) as u64;
    assert!(
        bytes_per_second >= MIN_BYTES_PER_SECOND,
        "sustained rate {bytes_per_second} B/s is below {MIN_BYTES_PER_SECOND} B/s"
    );

    let perf = cli_json(exe, &endpoint, &["perf-stats"]);
    assert!(
        perf["pty_drained_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= OUTPUT_BYTES),
        "PTY receipt did not cover the fixed payload: {perf}"
    );
    assert!(
        perf["pty_budget_yields"]
            .as_u64()
            .is_some_and(|yields| yields > 0),
        "fixed payload never exercised bounded PTY scheduling: {perf}"
    );
    assert_eq!(perf["present_failure"], 0);
    assert_eq!(perf["host_copy_frames"], 0);

    eprintln!(
        "AGENTERM_CON_EVIDENCE agenterm_con_throughput::sustained_long_output_keeps_control_and_sibling_responsive {}",
        serde_json::json!({
            "bytes": OUTPUT_BYTES,
            "elapsed_ms": elapsed.as_millis(),
            "bytes_per_second": bytes_per_second,
            "pty_drained_bytes": perf["pty_drained_bytes"],
            "pty_budget_yields": perf["pty_budget_yields"],
            "present_failure": perf["present_failure"],
            "host_copy_frames": perf["host_copy_frames"]
        })
    );

    cli_json(exe, &endpoint, &["close-window"]);
    let close_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = gui.0.try_wait().expect("poll closed GUI") {
            assert!(status.success(), "close-window failed with {status:?}");
            break;
        }
        assert!(
            Instant::now() < close_deadline,
            "close-window did not release the sustained-output host"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
