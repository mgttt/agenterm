//! `nativecore_run` — the CLI-triggerable entry point for `agenterm-nativecore`
//! packs, mirroring `examples/dynacore_run.rs`'s shape (own small `main`,
//! parse args, load-or-build a module, verify, run, print outcome, exit code
//! reflects termination) but SIMPLER: nativecore has no fleet bridge and no
//! live server round trip at all (`plan/design-dynacore-native-core.md` §7.1
//! — "没有 bridge 要穿过去"). It runs entirely locally against real Win32
//! APIs through `agenterm_nativecore::seam::do_intent`.
//!
//! Lives under `examples/` for the same reason `dynacore_run.rs` does (see
//! that file's header): `Cargo.toml`/`src/commands.rs` etc. were mid-flight
//! modified by a concurrent session at the time this was written (re-confirmed
//! via `git status` before writing this file). Cargo auto-discovers root
//! `examples/*.rs` (`autoexamples` is not disabled, unlike `autobins`), so no
//! `Cargo.toml` edit is needed: `cargo run --example nativecore_run -- ...`.
//!
//! This deliberately does NOT go through `src/script_backend.rs`'s
//! `try_execute_nativecore_pack_invocation` — that function requires a full
//! `ScriptOperation`/`execute_inner` context this example doesn't have.
//! Instead it calls `agenterm_nativecore::verify::verify` and
//! `agenterm_nativecore::eval_core::run` directly, the same way
//! `crates/agenterm-nativecore/tests/native_intents.rs` and
//! `tests/nativecore_pack_spawn_echo_live.rs` do — a real, non-mocked way to
//! exercise the crate (same reasoning `dynacore_run.rs`'s own header gives for
//! calling `script_dynacore_host` functions directly instead of going through
//! a spawned `agenterm` binary; nativecore has even less reason to indirect
//! through the product binary since there is no server round trip at all).
//!
//! Usage:
//!   cargo run --example nativecore_run -- --payload pure_compute|read_hash_print|spawn_echo|filewrite_demo
//!   cargo run --example nativecore_run -- --pack-store DIR --pack-hash HASH
//!
//! Unlike `dynacore_run.rs` (which runs a demo pack by default when no pack
//! is named), this REQUIRES the caller to pick one of the two: nativecore has
//! four different demo payloads, not one obvious default. With no
//! `--payload` and no `--pack-store`/`--pack-hash`, this prints usage and
//! exits with a failure code.

use std::path::PathBuf;
use std::process::ExitCode;

use agenterm_nativecore::eval_core::{self, Termination};
use agenterm_nativecore::ir::Module;
use agenterm_nativecore::pack;
use agenterm_nativecore::payloads;
use agenterm_nativecore::store::Store;
use agenterm_nativecore::verify;

#[derive(Clone, Copy)]
enum Payload {
    PureCompute,
    ReadHashPrint,
    SpawnEcho,
    FilewriteDemo,
}

impl Payload {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "pure_compute" => Ok(Payload::PureCompute),
            "read_hash_print" => Ok(Payload::ReadHashPrint),
            "spawn_echo" => Ok(Payload::SpawnEcho),
            "filewrite_demo" => Ok(Payload::FilewriteDemo),
            other => Err(format!(
                "unknown --payload '{other}' (expected one of: pure_compute, read_hash_print, spawn_echo, filewrite_demo)"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Payload::PureCompute => "pure_compute",
            Payload::ReadHashPrint => "read_hash_print",
            Payload::SpawnEcho => "spawn_echo",
            Payload::FilewriteDemo => "filewrite_demo",
        }
    }
}

struct Args {
    payload: Option<Payload>,
    pack_store: Option<PathBuf>,
    pack_hash: Option<String>,
}

fn usage() -> &'static str {
    "usage: cargo run --example nativecore_run -- --payload NAME\n   or: cargo run --example nativecore_run -- --pack-store DIR --pack-hash HASH\n\n\
     --payload NAME        one of: pure_compute, read_hash_print, spawn_echo, filewrite_demo.\n\
                            Builds the named built-in payload, packs it into a fresh temp\n\
                            Store, verifies it, and runs it.\n\
     --pack-store DIR      an agenterm-nativecore Store directory to load a pack from.\n\
     --pack-hash HASH      the content hash of the pack to load from --pack-store.\n\
                            --pack-store and --pack-hash must be given together.\n\n\
     Exactly one of --payload, or --pack-store+--pack-hash together, is required —\n\
     there is no default demo (nativecore has four, not one obvious choice)."
}

enum ParsedArgs {
    Run(Args),
    Help,
}

fn parse_args() -> Result<ParsedArgs, String> {
    let mut payload: Option<Payload> = None;
    let mut pack_store: Option<PathBuf> = None;
    let mut pack_hash: Option<String> = None;

    let mut it = std::env::args().skip(1);
    while let Some(argument) = it.next() {
        match argument.as_str() {
            "--payload" => {
                let value = it.next().ok_or("--payload requires a value")?;
                payload = Some(Payload::parse(&value)?);
            }
            "--pack-store" => {
                pack_store = Some(PathBuf::from(it.next().ok_or("--pack-store requires a value")?));
            }
            "--pack-hash" => {
                pack_hash = Some(it.next().ok_or("--pack-hash requires a value")?);
            }
            "-h" | "--help" => return Ok(ParsedArgs::Help),
            other => return Err(format!("unrecognized argument '{other}'")),
        }
    }

    let has_pack_store_pair = match (&pack_store, &pack_hash) {
        (Some(_), Some(_)) => true,
        (None, None) => false,
        _ => return Err("--pack-store and --pack-hash must be given together".to_string()),
    };

    match (payload.is_some(), has_pack_store_pair) {
        (true, true) => Err("--payload cannot be combined with --pack-store/--pack-hash — pick one".to_string()),
        (false, false) => Err("nothing to run: pass --payload NAME, or --pack-store DIR --pack-hash HASH".to_string()),
        _ => Ok(ParsedArgs::Run(Args { payload, pack_store, pack_hash })),
    }
}

/// A fresh, unique scratch directory under the OS temp dir — same naming
/// scheme `tests/nativecore_pack_spawn_echo_live.rs` and this crate's own
/// tests use (pid + nanos, so concurrent runs never collide).
fn fresh_temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "agenterm-nativecore-run-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ))
}

/// Build the named built-in payload, pack it into a fresh real `Store` on
/// disk, and load the bytes back (never reuse the in-memory `Module` the
/// builder produced) — the same store round trip
/// `crates/agenterm-nativecore/tests/native_intents.rs::run_through_full_pipeline`
/// uses, not a shortcut that skips it.
fn build_and_pack_payload(payload: Payload) -> Result<Module, String> {
    let dir = fresh_temp_dir(payload.name());
    let module = match payload {
        Payload::PureCompute => payloads::pure_compute(),
        Payload::ReadHashPrint => payloads::read_hash_print(),
        Payload::SpawnEcho => payloads::spawn_echo(),
        Payload::FilewriteDemo => {
            let target = dir.join("nativecore_run_filewrite_demo_out.txt");
            payloads::filewrite_demo(&target.to_string_lossy())
        }
    };

    let store = Store::open(&dir).map_err(|error| format!("failed to open fresh pack store {}: {error}", dir.display()))?;
    let manifest = pack::pack(&store, &module).map_err(|error| format!("failed to pack '{}' into {}: {error}", payload.name(), dir.display()))?;
    println!("packed '{}' into {} as {}", payload.name(), dir.display(), manifest.hash);
    pack::load(&store, &manifest)
}

/// Load an existing packed module from a real store on disk. Reuses
/// `agenterm::script_nativecore_pack::load_nativecore_pack` (load+verify,
/// already correct) rather than re-implementing that logic.
fn load_module(args: &Args) -> Result<Module, String> {
    match (args.payload, &args.pack_store, &args.pack_hash) {
        (Some(payload), _, _) => build_and_pack_payload(payload),
        (None, Some(store_dir), Some(hash)) => {
            let loaded = agenterm::script_nativecore_pack::load_nativecore_pack(store_dir, hash)?;
            Ok(loaded.module)
        }
        (None, _, _) => unreachable!("parse_args guarantees exactly one path is selected"),
    }
}

/// Redirect the REAL process `STD_OUTPUT_HANDLE` to a pipe for the duration
/// of `f`, restore it afterward, and return `f`'s result plus everything
/// written to the pipe — copied verbatim (same technique, same Win32 calls)
/// from `tests/nativecore_pack_spawn_echo_live.rs`'s own `capture_real_stdout`,
/// itself copied from `crates/agenterm-nativecore/tests/native_intents.rs`.
/// This intercepts raw Win32 `WriteFile` calls (`seam.rs`'s path) — it does
/// NOT rely on Rust's `print!`/`io::stdout()` capturing, which never
/// observes these writes at all.
#[cfg(windows)]
fn capture_real_stdout<T>(f: impl FnOnce() -> T) -> (T, String) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut u8;
        fn SetStdHandle(which: u32, h: *mut u8) -> i32;
        fn CreatePipe(read_h: *mut *mut u8, write_h: *mut *mut u8, sa: *mut u8, size: u32) -> i32;
        fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
        fn CloseHandle(h: *mut u8) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11 as u32
    unsafe {
        let mut read_h: *mut u8 = std::ptr::null_mut();
        let mut write_h: *mut u8 = std::ptr::null_mut();
        assert_ne!(
            CreatePipe(&mut read_h, &mut write_h, std::ptr::null_mut(), 0),
            0,
            "CreatePipe failed"
        );
        let original = GetStdHandle(STD_OUTPUT_HANDLE);
        assert_ne!(SetStdHandle(STD_OUTPUT_HANDLE, write_h), 0, "SetStdHandle failed");

        let result = f();

        SetStdHandle(STD_OUTPUT_HANDLE, original);
        CloseHandle(write_h); // our copy must close so ReadFile below sees EOF

        let mut collected = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut got: u32 = 0;
            let r = ReadFile(read_h, buf.as_mut_ptr(), buf.len() as u32, &mut got, std::ptr::null_mut());
            if r == 0 || got == 0 {
                break;
            }
            collected.extend_from_slice(&buf[..got as usize]);
        }
        CloseHandle(read_h);
        (result, String::from_utf8_lossy(&collected).to_string())
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(ParsedArgs::Help) => {
            println!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Ok(ParsedArgs::Run(args)) => args,
        Err(message) => {
            eprintln!("error: {message}\n");
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

    // `load_nativecore_pack` (the --pack-store path) already ran this same
    // check once as its own load-time gate, but it can only hand back the
    // owned `Module` — a `VerifiedModule` borrows the `Module` it verifies,
    // so it cannot outlive that function's return. Re-verifying here (cheap:
    // a produce-time structural gate, no execution — see `verify.rs`'s own
    // doc) is what actually produces the borrowed `VerifiedModule` this run
    // needs, for both the built-payload path and the loaded-pack path alike.
    let verified = match verify::verify(&module) {
        Ok(verified) => verified,
        Err(fault) => {
            eprintln!("error: pack failed verification: {fault:?}");
            return ExitCode::FAILURE;
        }
    };

    let (outcome, stdout) = capture_real_stdout(|| eval_core::run(&verified));

    if !stdout.is_empty() {
        print!("{stdout}");
    }

    for (index, call) in outcome.calls.iter().enumerate() {
        println!("[{index}] {:?} args={:?} -> {}", call.intent, call.args, call.result);
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
