//! `dynacore_run` — the CLI-triggerable entry point `plan/design-dynacore-logic-pack.md`
//! and `tests/dynacore_live_server.rs`'s header comment both call out as the one
//! remaining piece of task 2. Loads (or builds, for the demo case) a dynacore logic
//! pack, verifies it against the target server's real `OPERATION_CATALOG` shape, runs
//! it, and prints the outcome — from an actual terminal, against an actual running
//! `agenterm` server, no test harness required.
//!
//! This lives under `examples/` rather than as a new `[[bin]]` or `agenterm-cli`
//! subcommand because `Cargo.toml`/`src/client/mod.rs`/`src/commands.rs` were, at the
//! time this was written, mid-flight-modified by a concurrent session (see
//! `tests/dynacore_live_server.rs`'s header for the full account) — the same
//! constraint still applies, re-confirmed via `git status` before writing this file.
//! Cargo auto-discovers root `examples/*.rs` as independently runnable targets
//! (`autoexamples` is NOT disabled in `Cargo.toml`, unlike `autobins`), so this needs
//! no `Cargo.toml` edit at all: `cargo run --example dynacore_run -- ...`.
//!
//! Usage:
//!   cargo run --example dynacore_run -- --address 127.0.0.1:PORT
//!   cargo run --example dynacore_run -- --address 127.0.0.1:PORT --pack-store DIR --pack-hash HASH
//!
//! Without `--pack-store`/`--pack-hash`, runs the built-in
//! `agenterm::script_dynacore_host::demo_pack_module()` — two real `fleet.tabs.list`
//! round trips with a real branch between them — so the command is usable out of the
//! box against any running server, no hand-built pack required first.

use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::Arc;

use agenterm::script_dynacore_host::{self, DynacoreFleetBridgeFn};
use agenterm_dynacore::eval_core::Termination;
use agenterm_dynacore::pack::{self, PackManifest};
use agenterm_dynacore::store::Store;

struct Args {
    address: String,
    pack_store: Option<PathBuf>,
    pack_hash: Option<String>,
}

fn usage() -> &'static str {
    "usage: cargo run --example dynacore_run -- --address HOST:PORT [--pack-store DIR --pack-hash HASH]\n\n\
     --address HOST:PORT   required; the target agenterm server's IPC address.\n\
     --pack-store DIR      optional; a dynacore Store directory to load a pack from.\n\
     --pack-hash HASH      optional; the content hash of the pack to load from --pack-store.\n\
                            --pack-store and --pack-hash must be given together, or not at all.\n\
                            Without them, runs the built-in demo pack (two real\n\
                            fleet.tabs.list round trips against the target server)."
}

fn parse_args() -> Result<Args, String> {
    let mut address: Option<String> = None;
    let mut pack_store: Option<PathBuf> = None;
    let mut pack_hash: Option<String> = None;

    let mut it = std::env::args().skip(1);
    while let Some(argument) = it.next() {
        match argument.as_str() {
            "--address" => {
                address = Some(it.next().ok_or("--address requires a value")?);
            }
            "--pack-store" => {
                pack_store = Some(PathBuf::from(it.next().ok_or("--pack-store requires a value")?));
            }
            "--pack-hash" => {
                pack_hash = Some(it.next().ok_or("--pack-hash requires a value")?);
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unrecognized argument '{other}'")),
        }
    }

    let address = address.ok_or("--address is required")?;
    match (&pack_store, &pack_hash) {
        (Some(_), Some(_)) | (None, None) => {}
        _ => return Err("--pack-store and --pack-hash must be given together, or not at all".to_string()),
    }

    Ok(Args {
        address,
        pack_store,
        pack_hash,
    })
}

/// Locate the `agenterm-cli` executable this run's bridge shells out to.
///
/// `tests/dynacore_live_server.rs` uses `env!("CARGO_BIN_EXE_agenterm-cli")` — but that
/// is a Cargo integration-test/benchmark-only mechanism: confirmed by probe (`cargo
/// build --example` against a trial file using it fails at compile time with
/// "environment variable `CARGO_BIN_EXE_agenterm-cli` not defined at compile time... may
/// not be available for the current Cargo target"), it is not defined for `[[example]]`
/// targets. This falls back to a runtime sibling-path lookup instead: an example binary
/// builds to `target/<profile>/examples/dynacore_run(.exe)`, and `agenterm-cli` builds
/// to `target/<profile>/agenterm-cli(.exe)` — one directory up. If that sibling path
/// does not exist (e.g. a stripped/relocated install), falls back to bare `PATH`
/// resolution by executable name, same as any other shell-invoked tool.
fn locate_agenterm_cli() -> PathBuf {
    let exe_name = format!("agenterm-cli{}", std::env::consts::EXE_SUFFIX);
    if let Ok(current) = std::env::current_exe()
        && let Some(sibling) = current.parent().and_then(|examples_dir| examples_dir.parent())
    {
        let candidate = sibling.join(&exe_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(exe_name)
}

/// Same bridge SEMANTICS as `tests/dynacore_live_server.rs::live_bridge`: only
/// `tabs.list` is wired — it shells out to the already-existing, unmodified
/// `agenterm-cli ui-snapshot` command against `address` and projects the real `tabs`
/// field out of its JSON output (the same translation `src/client/mod.rs`'s
/// `handle_script_broker` performs for the `tabs.list` broker operation). Every other
/// `operation_id` returns a typed `Err` describing exactly that — this bridge does not
/// pretend to support anything it doesn't. Duplicated rather than shared with the test
/// file: an example and a test are separate compilation units, and the test's
/// `live_bridge` is a private `fn`, not something an example can `use`.
fn live_bridge(agenterm_cli: PathBuf, address: String) -> DynacoreFleetBridgeFn {
    Arc::new(move |operation_id: &str, _params_json: &str| -> Result<String, String> {
        if operation_id != "tabs.list" {
            return Err(format!(
                "dynacore_run's live bridge has no real mapping wired for '{operation_id}' \
                 (only 'tabs.list' is supported)"
            ));
        }
        let output = Command::new(&agenterm_cli)
            .args(["--address", &address, "ui-snapshot"])
            .output()
            .map_err(|error| format!("failed to spawn {}: {error}", agenterm_cli.display()))?;
        if !output.status.success() {
            return Err(format!(
                "agenterm-cli ui-snapshot exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("ui-snapshot did not print valid JSON: {error}"))?;
        let tabs = snapshot.get("tabs").cloned().unwrap_or_else(|| serde_json::json!([]));
        Ok(tabs.to_string())
    })
}

/// Load-or-build the module to run. With `--pack-store`/`--pack-hash`: opens the store,
/// builds the minimal manifest a hash-only caller can supply (per `store.rs`'s own
/// design — the store takes a hash the caller already has, no name->hash lookup;
/// `operation_ids` is left empty because `pack::load`/`load_and_verify_pack` never read
/// it, only `manifest.hash` — that field exists for a build-time producer's own audit
/// bookkeeping, not for loading), and runs it through
/// `script_dynacore_host::load_and_verify_pack` — the real host binding's load+verify
/// step, not a hand-rolled shortcut around it. Without them: the built-in demo pack.
fn load_module(args: &Args) -> Result<agenterm_dynacore::ir::Module, String> {
    match (&args.pack_store, &args.pack_hash) {
        (Some(store_dir), Some(hash)) => {
            let store = Store::open(store_dir)
                .map_err(|error| format!("failed to open pack store {}: {error}", store_dir.display()))?;
            let manifest = PackManifest {
                schema_version: pack::PACK_SCHEMA_VERSION,
                hash: hash.clone(),
                operation_ids: Vec::new(),
            };
            script_dynacore_host::load_and_verify_pack(&store, &manifest)
        }
        _ => Ok(script_dynacore_host::demo_pack_module()),
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("error: {message}\n");
            }
            eprintln!("{}", usage());
            return ExitCode::FAILURE;
        }
    };

    let module = match load_module(&args) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("error: failed to load pack: {error}");
            return ExitCode::FAILURE;
        }
    };

    // `load_and_verify_pack` (the --pack-store path) already ran this same check once
    // as its own load-time gate, but it can only hand back the owned `Module` — a
    // `VerifiedModule` borrows the `Module` it verifies, so it cannot outlive that
    // function's return. Re-verifying here (cheap: a produce-time structural gate, no
    // execution — see `verify.rs`) is what actually produces the borrowed
    // `VerifiedModule` this run needs, for both the demo path and the loaded-pack path
    // alike.
    let verified = match script_dynacore_host::verify_pack(&module) {
        Ok(verified) => verified,
        Err(fault) => {
            eprintln!("error: pack failed verification: {fault:?}");
            return ExitCode::FAILURE;
        }
    };

    let agenterm_cli = locate_agenterm_cli();
    let bridge = live_bridge(agenterm_cli, args.address.clone());

    let outcome = script_dynacore_host::run_pack(&verified, &bridge);

    for (index, call) in outcome.calls.iter().enumerate() {
        match &call.result {
            Ok(result) => println!(
                "[{index}] {} params={} -> Ok {result}",
                call.operation_id, call.params_json
            ),
            Err(error) => println!(
                "[{index}] {} params={} -> Err {error}",
                call.operation_id, call.params_json
            ),
        }
    }

    match outcome.termination {
        Termination::Exited(value) => {
            println!("Termination: Exited({value})");
            ExitCode::SUCCESS
        }
        Termination::StepLimitExceeded => {
            eprintln!("Termination: StepLimitExceeded — the run was forcibly cut off before finishing");
            ExitCode::FAILURE
        }
    }
}
