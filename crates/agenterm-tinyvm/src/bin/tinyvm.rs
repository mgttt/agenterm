//! Minimal VM evaluator and TinyArcade cartridge conformance front door.
//!
//! Assembles one-instruction-per-line text and runs it on a fresh [`Vm`],
//! printing the resulting stack top. Assembly comes from the arguments after
//! `eval` (joined with newlines) or, if none are given, from stdin.
//!
//! This is intentionally not a REPL framework — the persistent-image REPL is
//! the library's `Vm::eval` loop; this binary is a thin one-shot front door.

use std::io::Read;
use std::process::ExitCode;

use agenterm_tinyvm::{
    CartridgeManifest, GameInput, GameLimits, GameRuntime, Grid3dFrame, Limits, ToneBatch, Vm,
    WasmError,
};

const MEM_CELLS: usize = 4_096;
const MAX_CARTRIDGE_BYTES: u64 = 2 * 1024 * 1024;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("eval") => {
            let rest: Vec<String> = args.collect();
            let src = if rest.is_empty() {
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("tinyvm: reading stdin: {e}");
                    return ExitCode::FAILURE;
                }
                buf
            } else {
                rest.join("\n")
            };
            run_eval(&src)
        }
        Some("cartridge") => match (args.next().as_deref(), args.next(), args.next()) {
            (Some("inspect"), Some(path), None) => run_cartridge(&path, false),
            (Some("check"), Some(path), None) => run_cartridge(&path, true),
            _ => usage(),
        },
        Some(other) => {
            eprintln!("tinyvm: unknown command `{other}`");
            usage()
        }
        None => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage:");
    eprintln!("  tinyvm eval [asm...]");
    eprintln!("  tinyvm cartridge inspect FILE.wasm");
    eprintln!("  tinyvm cartridge check FILE.wasm");
    ExitCode::FAILURE
}

fn read_cartridge(path: &str) -> Result<Vec<u8>, &'static str> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "cannot stat cartridge")?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err("cartridge is not a non-empty regular file");
    }
    if metadata.len() > MAX_CARTRIDGE_BYTES {
        return Err("cartridge exceeds 2 MiB converter limit");
    }
    std::fs::read(path).map_err(|_| "cannot read cartridge")
}

fn run_cartridge(path: &str, execute: bool) -> ExitCode {
    let bytes = match read_cartridge(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            eprintln!("tinyvm: {message}");
            return ExitCode::FAILURE;
        }
    };
    let manifest = match CartridgeManifest::from_wasm(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => return cartridge_error(error),
    };
    println!("game_id={}", manifest.game_id);
    println!("game_version={}", manifest.game_version);
    println!("abi_version={}", manifest.abi_version);
    println!("state_version={}", manifest.state_version);
    println!("wasm_bytes={}", bytes.len());
    if !manifest.capabilities.is_empty() {
        println!("native_capabilities={}", manifest.capabilities.join(","));
    } else {
        println!("native_capabilities=(none)");
    }
    if !execute {
        println!("OK: canonical TinyArcade manifest");
        return ExitCode::SUCCESS;
    }

    let vm_limits = Limits {
        max_table_elems: 1_024,
        max_memory_pages: 64,
        max_steps: 1_000_000,
    };
    let game_limits = GameLimits {
        max_render_bytes: 64 * 1024,
        max_audio_bytes: 16 * 1024,
        max_state_bytes: 256 * 1024,
    };
    let mut first =
        match GameRuntime::from_private_bytes(&bytes, vm_limits, game_limits, 0x5441_4331) {
            Ok(runtime) => runtime,
            Err(error) => return cartridge_error(error),
        };
    let initial = match first.tick(GameInput {
        buttons: 0,
        clock_ms: 0,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    if let Err(error) = validate_media(&initial.render, &initial.audio) {
        return cartridge_error(error);
    }
    let snapshot = match first.suspend() {
        Ok(snapshot) => snapshot,
        Err(error) => return cartridge_error(error),
    };
    let expected = match first.tick(GameInput {
        buttons: 0,
        clock_ms: 16,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    let mut restored =
        match GameRuntime::from_private_bytes(&bytes, vm_limits, game_limits, 0x5441_4331) {
            Ok(runtime) => runtime,
            Err(error) => return cartridge_error(error),
        };
    if let Err(error) = restored.resume(&snapshot) {
        return cartridge_error(error);
    }
    let replay = match restored.tick(GameInput {
        buttons: 0,
        clock_ms: 16,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    if let Err(error) = validate_media(&replay.render, &replay.audio) {
        return cartridge_error(error);
    }
    if expected.render != replay.render || expected.audio != replay.audio {
        eprintln!("tinyvm: suspend/resume replay is not byte-deterministic");
        return ExitCode::FAILURE;
    }
    println!("initial_render_bytes={}", initial.render.len());
    println!("initial_audio_bytes={}", initial.audio.len());
    println!("snapshot_bytes={}", snapshot.len());
    println!("OK: private-import converter conformance v1");
    ExitCode::SUCCESS
}

fn validate_media(render: &[u8], audio: &[u8]) -> Result<(), WasmError> {
    Grid3dFrame::decode(render)?;
    if !audio.is_empty() {
        ToneBatch::decode(audio)?;
    }
    Ok(())
}

fn cartridge_error(error: WasmError) -> ExitCode {
    eprintln!("tinyvm: {}", error.message());
    ExitCode::FAILURE
}

fn run_eval(src: &str) -> ExitCode {
    let mut vm = Vm::new(MEM_CELLS);
    match vm.eval(src) {
        Ok(Some(top)) => {
            println!("{top}");
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("(empty)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("tinyvm: {}", e.message());
            ExitCode::FAILURE
        }
    }
}
