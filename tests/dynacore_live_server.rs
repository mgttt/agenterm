//! Black-box proof for `plan/design-dynacore-logic-pack.md`'s task-2
//! acceptance: a dynacore pack, loaded/verified/interpreted through the real
//! `agenterm::script_dynacore_host` binding, makes its `fleet.tabs.list`
//! calls against a REAL, separately-spawned `agenterm` server process — not
//! the mock bridge `tests/dynacore_host.rs` uses. The round trip goes
//! through the exact external IPC client surface every other black-box test
//! in this repo already uses: `env!("CARGO_BIN_EXE_agenterm")` +
//! `cli --address`, following `tests/headless_ui_geometry.rs`'s `Harness`
//! pattern — not a new, reinvented connection mechanism, and not the
//! internal `script_worker.rs`/`BrokerClient` path (out of scope per the
//! task's own rule against touching that file while it is mid-refactor).
//!
//! **What this file's bridge deliberately does NOT do**: forward an
//! arbitrary `operation_id`/`params_json` generically the way
//! `src/client/mod.rs`'s `script_fleet_call` already does for rh/lua/qjs
//! scripts. Wiring a NEW CLI-triggerable entry point onto that real,
//! general bridge (a `dynacore-run` subcommand, or a standalone
//! `src/bin/agenterm-dynacore-run.rs`) needs to touch `src/client/mod.rs`
//! and `src/commands.rs` (subcommand dispatch + the `COMMAND_CATALOG`/
//! `control_command_spec` validation tables a new subcommand must be
//! registered in) and/or the root `Cargo.toml` (`autobins = false` — a new
//! binary target needs an explicit `[[bin]]` entry). As of this session's
//! `git status`, all three are ALREADY mid-flight-modified by a concurrent
//! session (the same files the git-log "Trait-M3/M4 fold" work touches,
//! plus what `plan/archive/design-agenterm-cli-merge.md` /
//! `plan/archive/design-agenterm-bin-separation.md` show was the then
//! in-progress CLI binary restructuring, since shipped). This environment's git tooling has no
//! non-interactive way to commit only NEW hunks in an already-dirty file
//! (no `git add -p`/`git rebase -i`), so editing any of those three right
//! now would mean committing a mix of this task's work and the other
//! session's unrelated, unfinished, uncommitted work under one commit —
//! worse than reporting the gap plainly. This test proves the mechanism
//! that WOULD sit behind that front door genuinely reaches a live server;
//! wiring the CLI-triggerable entry point itself is the one piece of task 2
//! left undone, and exactly why, rather than being quietly worked around.

use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use agenterm::script_dynacore_host::{self, DynacoreFleetBridgeFn};
use agenterm_dynacore::eval_core::Termination;

/// Same isolation shape as `tests/headless_ui_geometry.rs`'s `Harness`
/// (trimmed to what this file needs): a real, isolated headless `agenterm`
/// server process this test owns end to end.
struct Harness {
    root: PathBuf,
    address: String,
    workspace: PathBuf,
    settings: PathBuf,
    instances: PathBuf,
    server: Option<Child>,
    // OS-enforced backstop for the manual kill in `Drop` below: a Job Object
    // (Windows) / process-group containment (elsewhere) with kill-on-close
    // semantics. `Drop::drop` only runs when this test process exits or
    // panics normally; an external force-kill of the test binary (a CI
    // timeout, a hung `wait_ready` loop someone Ctrl-C's) skips Rust `Drop`
    // entirely and orphans `agenterm.exe server` — this guard's containment
    // handle is closed by the OS during process teardown regardless of how
    // that teardown happens, so the child dies either way.
    _server_tree_guard: Option<agenterm_platform::process::ProcessTreeGuard>,
}

impl Harness {
    fn start(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenterm-dynacore-live-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_millis()
        ));
        fs::create_dir_all(&root).expect("create isolation root");
        let address = TcpListener::bind("127.0.0.1:0")
            .expect("reserve loopback address")
            .local_addr()
            .expect("loopback address")
            .to_string();
        let instances = root.join("instances");
        fs::create_dir_all(&instances).expect("create instance directory");
        let mut harness = Self {
            workspace: root.join("workspace.json"),
            settings: root.join("settings.json"),
            instances,
            address,
            root,
            server: None,
            _server_tree_guard: None,
        };
        let server = harness
            .server_command()
            .args(["server", "--address", &harness.address])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start isolated headless server");
        harness._server_tree_guard = Some(
            agenterm_platform::process::ProcessTreeGuard::attach(&server)
                .expect("attach kill-on-close containment to isolated server"),
        );
        harness.server = Some(server);
        harness.wait_ready();
        harness
    }

    fn server_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm"));
        command
            .env("AGENTERM_IPC_ADDRESS", &self.address)
            .env("AGENTERM_WORKSPACE_PATH", &self.workspace)
            .env("AGENTERM_SETTINGS_PATH", &self.settings)
            .env("AGENTERM_INSTANCE_DIR", &self.instances)
            .env("AGENTERM_NO_ACTIVATE", "1")
            .env("AGENTERM_HEADLESS_VIEWPORT", "1400x900");
        command
    }

    fn cli(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm"));
        command.arg("cli");
        command
            .env("AGENTERM_IPC_ADDRESS", &self.address)
            .env("AGENTERM_WORKSPACE_PATH", &self.workspace)
            .env("AGENTERM_SETTINGS_PATH", &self.settings)
            .env("AGENTERM_INSTANCE_DIR", &self.instances)
            .env("AGENTERM_NO_ACTIVATE", "1")
            .args(["--address", &self.address])
            .args(args)
            .output()
            .expect("run isolated CLI")
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let output = self.cli(&["protocol-info", "--running"]);
            if output.status.success() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "server never became reachable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = self.cli(&["kill-server"]);
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if matches!(server.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The real bridge for this proof: routes `fleet.tabs.list` to a real,
/// already-running server by shelling out to the ALREADY-EXISTING,
/// unmodified `agenterm cli ui-snapshot` command (this task added nothing to
/// that command) and projecting its real `tabs` field — the exact
/// translation `src/client/mod.rs`'s `handle_script_broker` already performs
/// for the `tabs.list` broker operation. This duplicates that one mapping,
/// not the IPC connection mechanism itself, which is entirely
/// `agenterm cli`'s own compiled, unmodified behavior.
fn live_bridge(cli_env: Vec<(&'static str, String)>, address: String) -> DynacoreFleetBridgeFn {
    Arc::new(move |operation_id: &str, _params_json: &str| -> Result<String, String> {
        if operation_id != "tabs.list" {
            return Err(format!(
                "dynacore_live_server test bridge has no real mapping wired for {operation_id}"
            ));
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm"));
        command.arg("cli");
        for (key, value) in &cli_env {
            command.env(key, value);
        }
        let output = command
            .args(["--address", &address, "ui-snapshot"])
            .output()
            .map_err(|error| format!("failed to spawn agenterm cli: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "agenterm cli ui-snapshot exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("ui-snapshot did not print JSON: {error}"))?;
        let tabs = snapshot
            .get("tabs")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        Ok(tabs.to_string())
    })
}

fn harness_cli_env(harness: &Harness) -> Vec<(&'static str, String)> {
    vec![
        ("AGENTERM_IPC_ADDRESS", harness.address.clone()),
        (
            "AGENTERM_WORKSPACE_PATH",
            harness.workspace.to_string_lossy().into_owned(),
        ),
        (
            "AGENTERM_SETTINGS_PATH",
            harness.settings.to_string_lossy().into_owned(),
        ),
        (
            "AGENTERM_INSTANCE_DIR",
            harness.instances.to_string_lossy().into_owned(),
        ),
        ("AGENTERM_NO_ACTIVATE", "1".to_owned()),
    ]
}

/// Acceptance: the demo pack's Ok arm — two real `fleet.tabs.list` round
/// trips with a real `BrCond` between them — run against a REAL, separately
/// spawned agenterm server and come back with REAL data, not a fixed mock
/// string.
#[test]
fn demo_pack_ok_arm_makes_two_real_calls_against_a_live_server() {
    let harness = Harness::start("ok-arm");
    let module = script_dynacore_host::demo_pack_module();
    let verified =
        script_dynacore_host::verify_pack(&module).expect("the demo pack must verify against the real OPERATION_CATALOG");
    let bridge = live_bridge(harness_cli_env(&harness), harness.address.clone());

    let outcome = script_dynacore_host::run_pack(&verified, &bridge);

    assert_eq!(
        outcome.termination,
        Termination::Exited(1),
        "against a reachable server tabs.list succeeds, so the pack takes the Ok arm and \
         exits with the SECOND call's dest word (1)"
    );
    assert_eq!(
        outcome.calls.len(),
        2,
        "the Ok arm makes exactly two real fleet.tabs.list round trips (the branch, then the \
         call inside the Ok block)"
    );
    for call in &outcome.calls {
        assert_eq!(call.operation_id, "tabs.list");
        assert_eq!(call.params_json, "{}");
        let result = call
            .result
            .as_ref()
            .expect("a real, reachable server answers tabs.list with Ok");
        let tabs: serde_json::Value =
            serde_json::from_str(result).expect("a real server's tabs.list result is JSON");
        assert!(
            tabs.is_array(),
            "a real ui-snapshot's tabs field is a JSON array, not a hand-written mock payload: {tabs}"
        );
    }
}

/// Acceptance: when no server is reachable, the SAME pack takes its Err arm
/// — one real (failed) call attempt, no second call, exit with the sentinel
/// — proving the branch genuinely depends on the live round trip's outcome
/// rather than always taking one hardcoded path.
#[test]
fn demo_pack_err_arm_when_no_server_is_reachable() {
    // Deliberately not starting a Harness — no server is listening here.
    // Mirrors `tests/control_center_contract.rs`'s own convention for a
    // guaranteed-unreachable loopback address.
    let unreachable_address = "127.0.0.1:9".to_string();
    let module = script_dynacore_host::demo_pack_module();
    let verified =
        script_dynacore_host::verify_pack(&module).expect("the demo pack must verify against the real OPERATION_CATALOG");
    let bridge = live_bridge(Vec::new(), unreachable_address);

    let outcome = script_dynacore_host::run_pack(&verified, &bridge);

    assert_eq!(
        outcome.termination,
        Termination::Exited(999),
        "with no server reachable, tabs.list fails, so the pack takes the Err arm and exits \
         with the sentinel"
    );
    assert_eq!(
        outcome.calls.len(),
        1,
        "the Err arm attempts exactly one call (the branch predicate) and never reaches the \
         Ok arm's second call"
    );
    assert!(
        outcome.calls[0].result.is_err(),
        "the one call attempted against an unreachable address must have really failed, not \
         been swapped for a canned Ok"
    );
}
