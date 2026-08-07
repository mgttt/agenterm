//! `agenterm-lua` binary — LuaJIT script worker for AgenTerm.
//!
//! Commands:
//!   agenterm-lua --framed-worker
//!   agenterm-lua check <file.lua>
//!   agenterm-lua eval <file.lua>
//!   agenterm-lua hash <file.lua>
//!   agenterm-lua pack build <file.lua> --dir <out>
//!   agenterm-lua pack load <dir>
//!   agenterm-lua qualify <file.lua> --dir <out>
//!   agenterm-lua run-smoke <pack.luac>

use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match dispatch(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agenterm-lua: {e}");
            1
        }
    };
    std::process::exit(code as i32);
}

fn dispatch(args: &[String]) -> Result<u8, String> {
    if args.len() < 2 {
        eprintln!("agenterm-lua: usage: agenterm-lua <command> [args...]");
        eprintln!("  commands: check check-many eval hash pack build pack load qualify task run-smoke --framed-worker");
        return Ok(1);
    }

    match args[1].as_str() {
        "--framed-worker" => run_framed_worker(),
        "check" => cmd_check(&args[2..]),
        "check-many" => cmd_check_many(&args[2..]),
        "eval" => cmd_eval(&args[2..]),
        "hash" => cmd_hash(&args[2..]),
        "pack" => {
            if args.len() < 3 {
                return Err("pack: expected subcommand: build or load".into());
            }
            match args[2].as_str() {
                "build" => cmd_pack_build(&args[3..]),
                "load" => cmd_pack_load(&args[3..]),
                other => Err(format!("pack: unknown subcommand: {other}")),
            }
        }
        "qualify" => cmd_qualify(&args[2..]),
        "task" => cmd_task(&args[2..]),
        "run-smoke" => cmd_run_smoke(&args[2..]),
        other => Err(format!("unknown command: {other}")),
    }
}

// ── CLI subcommands ────────────────────────────────────────────────────

/// `check <file.lua>` — syntax check.
fn cmd_check(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "check <file.lua>")?;
    let source = read_file(&path)?;
    let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;
    engine.check(&source).map_err(|e| e.to_string())?;
    println!("rh check ok: {}", path.display());
    Ok(0)
}

/// `eval <file.lua>` — compile and evaluate (with bytecode cache).
fn cmd_eval(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "eval <file.lua>")?;
    let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;
    let host = agenterm_lua::LuaHostFunctions::default();
    let (result, cache_hit) = engine.eval_file_cached(&path, &host)
        .map_err(|e| e.to_string())?;
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    let cache_note = if cache_hit { " [cache hit]" } else { "" };
    println!("rh eval ok: {} -> {}{cache_note}", path.display(), result.value);
    Ok(0)
}

/// `hash <file.lua>` — print SHA256 of source.
fn cmd_hash(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "hash <file.lua>")?;
    let source = read_file(&path)?;
    let hash = agenterm_lua::LuaEngine::hash_source(&source);
    println!("{hash}  {path}", path = path.display());
    Ok(0)
}

/// `pack build <file.lua> --dir <out>` — compile source to bytecode pack.
fn cmd_pack_build(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "pack build <file.lua> --dir <out>")?;
    let dir = require_flag_value(args, "--dir", "pack build requires --dir <out>")?;
    let source = read_file(&path)?;
    let _bc_path = agenterm_lua::pack::build_pack_dir(&source, &dir)
        .map_err(|e| e.to_string())?;
    println!("pack build ok: {}", path.display());
    println!("  output: {}", dir.display());
    Ok(0)
}

/// `pack load <dir>` — load and execute a pack directory.
fn cmd_pack_load(args: &[String]) -> Result<u8, String> {
    let dir = require_arg(args, 0, "pack load <dir>")?;
    let pack = agenterm_lua::pack::LuaPack::load(&dir).map_err(|e| e.to_string())?;
    let host = agenterm_lua::LuaHostFunctions::default();
    let result = pack.eval(&host).map_err(|e| e.to_string())?;
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    println!("pack load ok: {} -> {}", dir.display(), result.value);
    Ok(0)
}

/// `qualify <file.lua> --dir <out>` — build + load + entry → receipt.
fn cmd_qualify(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "qualify <file.lua> --dir <out>")?;
    let dir = require_flag_value(args, "--dir", "qualify requires --dir <out>")?;
    let source = read_file(&path)?;
    let host = agenterm_lua::LuaHostFunctions::default();
    let receipt = agenterm_lua::qualify::qualify_pack_dir(&source, &dir, &host)
        .map_err(|e| e.to_string())?;
    let receipt_path = dir.join("receipt.json");
    receipt.write(&receipt_path).map_err(|e| e.to_string())?;
    println!("qualify ok: {} -> {}", path.display(), receipt.entry_value);
    println!("  receipt: {}", receipt_path.display());
    Ok(0)
}

/// `check-many --manifest <file>` — bounded multi-file validation.
fn cmd_check_many(args: &[String]) -> Result<u8, String> {
    let manifest_path = require_flag_value(args, "--manifest", "check-many requires --manifest <file>")?;
    let manifest = agenterm_lua::check_many::read_manifest(&manifest_path)
        .map_err(|e| format!("check_many_manifest: {e}"))?;
    let options = agenterm_lua::check_many::CheckManyOptions::default();
    let report = agenterm_lua::check_many::run_check_many(manifest, options);
    if report.failures.is_empty() {
        println!("check-many: {} files ok", report.checked_files);
    } else {
        eprintln!("check-many: {} files checked, {} failures", report.checked_files, report.failures.len());
        for f in &report.failures {
            eprintln!("  {}: {} — {}", f.path, f.code, f.message);
        }
    }
    if report.ok { Ok(0) } else { Ok(1) }
}

/// `task <subcommand>` — delegate to agenterm main binary.
fn cmd_task(args: &[String]) -> Result<u8, String> {
    if args.is_empty() {
        eprintln!("task: expected subcommand: list, show, check, run");
        return Ok(1);
    }
    // Task execution is handled by the main agenterm binary (agenterm-rhai/agenterm-rh).
    // agenterm-lua supports individual task entries through the
    // AGENTERM_SCRIPT_BACKEND=lua worker path (--framed-worker).
    eprintln!(
        "task: use `agenterm-rhai task {}` or `AGENTERM_SCRIPT_BACKEND=lua agenterm-rhai task {}`",
        args.join(" "),
        args.join(" ")
    );
    Ok(0)
}

/// `run-smoke <pack.luac>` — bytecode smoke test (delegates to pack load).
fn cmd_run_smoke(args: &[String]) -> Result<u8, String> {
    let path = require_arg(args, 0, "run-smoke <dir>")?;
    // Treat as pack dir
    cmd_pack_load(&[path.to_string_lossy().to_string()])
}

// ── helpers ────────────────────────────────────────────────────────────

fn require_arg(args: &[String], index: usize, usage: &str) -> Result<PathBuf, String> {
    let value = args
        .get(index)
        .ok_or_else(|| format!("missing argument: {usage}"))?;
    Ok(PathBuf::from(value))
}

fn require_flag_value(args: &[String], flag: &str, usage: &str) -> Result<PathBuf, String> {
    let pos = args
        .iter()
        .position(|a| a == flag)
        .ok_or_else(|| usage.to_string())?;
    let value = args
        .get(pos + 1)
        .ok_or_else(|| format!("{flag} requires a value"))?;
    Ok(PathBuf::from(value))
}

fn read_file(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read_file {}: {e}", path.display()))
}

// ── framed worker ──────────────────────────────────────────────────────

fn run_framed_worker() -> Result<u8, String> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;

    let invocation: Option<serde_json::Value> = serde_json::from_slice(&input).ok();
    let source = invocation
        .as_ref()
        .and_then(|v| v.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let operation = invocation
        .as_ref()
        .and_then(|v| v.get("operation"))
        .and_then(|v| v.as_str())
        .unwrap_or("eval");

    let invocation_id = invocation
        .as_ref()
        .and_then(|v| v.get("invocation_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;
    let host = agenterm_lua::LuaHostFunctions::default();
    let mut stdout = std::io::stdout().lock();

    match operation {
        "check" => {
            match engine.check(source) {
                Ok(()) => write_json_result(&mut stdout, invocation_id, "check", true, "", None)?,
                Err(e) => write_json_result(
                    &mut stdout, invocation_id, "check", false, "",
                    Some(&format!("lua_parse: {e}")),
                )?,
            }
            Ok(0)
        }
        "eval" | "run" => {
            match engine.eval(source, &host) {
                Ok(eval_result) => write_json_result(
                    &mut stdout, invocation_id, operation, true, &eval_result.stdout,
                    None,
                )
                .map(|_| eval_result.value as u8),
                Err(e) => {
                    write_json_result(
                        &mut stdout, invocation_id, operation, false, "",
                        Some(&e.to_string()),
                    )?;
                    Ok(1)
                }
            }
        }
        _ => {
            match engine.eval(source, &host) {
                Ok(eval_result) => write_json_result(
                    &mut stdout, invocation_id, operation, true, &eval_result.stdout,
                    None,
                )
                .map(|_| eval_result.value as u8),
                Err(e) => {
                    write_json_result(
                        &mut stdout, invocation_id, operation, false, "",
                        Some(&e.to_string()),
                    )?;
                    Ok(1)
                }
            }
        }
    }
}

fn write_json_result(
    w: &mut impl Write,
    invocation_id: &str,
    operation: &str,
    ok: bool,
    stdout: &str,
    failure: Option<&str>,
) -> Result<(), String> {
    let mut result = serde_json::json!({
        "envelope_version": 2,
        "invocation_id": invocation_id,
        "api_version": 2,
        "ok": ok,
        "exit_class": if ok { "success" } else { "script" },
        "operation": operation,
        "stdout": stdout,
        "duration_ms": 1,
    });
    if let Some(msg) = failure {
        result["failure"] = serde_json::json!({
            "code": if operation == "check" { "lua_parse" } else { "lua_runtime" },
            "message": msg,
            "category": "script",
        });
    }
    serde_json::to_writer(&mut *w, &result).map_err(|e| e.to_string())?;
    w.write_all(b"\n").map_err(|e| e.to_string())?;
    Ok(())
}
