//! `agenterm-lua` CLI logic — extracted from `main.rs` (SUB-M1, see
//! `plan/design-script-engine-subcommands.md` §2/§5) so the root `agenterm`
//! binary can later call [`run`] directly as a subcommand entry point
//! without going through a separate process (SUB-M3, not this pass).
//!
//! Behavior is byte-identical to the pre-extraction `main.rs`: [`run`]
//! computes the same `Result<u8, String>` dispatch that `main.rs`'s
//! `dispatch()` used to, and the `eprintln!("agenterm-lua: {e}")` + exit
//! code fold that used to live in `main()` now lives in [`run`] itself, so
//! `main.rs` is a pure `exit(cli::run(args))` shell.
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

use crate::{find_flag_value, positional, require_flag_value};

/// Run the `agenterm-lua` CLI over `args` (including argv\[0\], matching
/// `std::env::args()`'s shape — same convention `dispatch()` used) and
/// return the process exit code. Mirrors `main.rs`'s former
/// `eprintln!("agenterm-lua: {e}")` + exit-1-on-error behavior for any
/// `Err` returned by the internal dispatch.
pub fn run(args: &[String]) -> u8 {
    match dispatch(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agenterm-lua: {e}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<u8, String> {
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return Ok(0);
    }

    match args[1].as_str() {
        "--framed-worker" => run_framed_worker(),
        "check" => cmd_check(&args[2..]),
        "check-many" => cmd_check_many(&args[2..]),
        "corpus-scan" => cmd_corpus_scan(&args[2..]),
        "eval" => cmd_eval(&args[2..]),
        "hash" => cmd_hash(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "version" => { println!("agenterm-lua {}", env!("CARGO_PKG_VERSION")); Ok(0) },
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
    let path = PathBuf::from(positional(args, 0, "missing argument: check <file.lua>")?);
    let source = read_file(&path)?;
    let engine = crate::LuaEngine::new().map_err(|e| e.to_string())?;
    engine.check(&source).map_err(|e| e.to_string())?;
    println!("rh check ok: {}", path.display());
    Ok(0)
}

/// `eval <file.lua>` — compile and evaluate (with bytecode cache).
fn cmd_eval(args: &[String]) -> Result<u8, String> {
    let path = PathBuf::from(positional(args, 0, "missing argument: eval <file.lua>")?);
    let engine = crate::LuaEngine::new().map_err(|e| e.to_string())?;
    let host = crate::LuaHostFunctions::default();
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
    let path = PathBuf::from(positional(args, 0, "missing argument: hash <file.lua>")?);
    let source = read_file(&path)?;
    let hash = crate::LuaEngine::hash_source(&source);
    println!("{hash}  {path}", path = path.display());
    Ok(0)
}

/// `pack build <file.lua> --dir <out>` — compile source to bytecode pack.
fn cmd_pack_build(args: &[String]) -> Result<u8, String> {
    let path = PathBuf::from(positional(
        args,
        0,
        "missing argument: pack build <file.lua> --dir <out>",
    )?);
    let dir = PathBuf::from(require_flag_value(
        args,
        "--dir",
        "pack build requires --dir <out>",
    )?);
    let source = read_file(&path)?;
    let _bc_path = crate::pack::build_pack_dir(&source, &dir)
        .map_err(|e| e.to_string())?;
    println!("pack build ok: {}", path.display());
    println!("  output: {}", dir.display());
    Ok(0)
}

/// `pack load <dir>` — load and execute a pack directory.
fn cmd_pack_load(args: &[String]) -> Result<u8, String> {
    let dir = PathBuf::from(positional(args, 0, "missing argument: pack load <dir>")?);
    let pack = crate::pack::LuaPack::load(&dir).map_err(|e| e.to_string())?;
    let host = crate::LuaHostFunctions::default();
    let result = pack.eval(&host).map_err(|e| e.to_string())?;
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    println!("pack load ok: {} -> {}", dir.display(), result.value);
    Ok(0)
}

/// `qualify <file.lua> --dir <out>` — build + load + entry → receipt.
fn cmd_qualify(args: &[String]) -> Result<u8, String> {
    let path = PathBuf::from(positional(
        args,
        0,
        "missing argument: qualify <file.lua> --dir <out>",
    )?);
    let dir = PathBuf::from(require_flag_value(
        args,
        "--dir",
        "qualify requires --dir <out>",
    )?);
    let source = read_file(&path)?;
    let host = crate::LuaHostFunctions::default();
    let receipt = crate::qualify::qualify_pack_dir(&source, &dir, &host)
        .map_err(|e| e.to_string())?;
    let receipt_path = dir.join("receipt.json");
    receipt.write(&receipt_path).map_err(|e| e.to_string())?;
    println!("qualify ok: {} -> {}", path.display(), receipt.entry_value);
    println!("  receipt: {}", receipt_path.display());
    Ok(0)
}

/// `check-many --manifest <file> [--project-root DIR] [--timeout-ms N] [--json]`
/// — bounded multi-file validation.
///
/// Parses through the shared `parse_check_many_cli` (same flag surface as
/// rh/qjs/sql). The previous version parsed only `--manifest`/`--json` by
/// hand and silently DROPPED `--project-root`/`--timeout-ms`/
/// `--max-output-bytes` — a caller following the aligned-CLI contract got
/// their project root ignored (paths resolved against the process CWD
/// instead), caught live by `tests/script_cli_verb_parity.rs`.
fn cmd_check_many(args: &[String]) -> Result<u8, String> {
    let parsed = crate::check_many::parse_check_many_cli(args.iter().cloned())?;
    let manifest = crate::check_many::read_manifest(&parsed.manifest_path)
        .map_err(|e| format!("check_many_manifest: {e}"))?;
    let report = crate::check_many::run_check_many(manifest, parsed.options);
    if parsed.json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    } else if report.failures.is_empty() {
        println!("check-many: {} files ok", report.checked_files);
    } else {
        eprintln!("check-many: {} files checked, {} failures", report.checked_files, report.failures.len());
        for f in &report.failures {
            eprintln!("  {}: {} — {}", f.path, f.code, f.message);
        }
    }
    if report.ok { Ok(0) } else { Ok(1) }
}

/// `corpus-scan [--dir <dir>]` — scan directory for .lua files and check syntax.
fn cmd_corpus_scan(args: &[String]) -> Result<u8, String> {
    let dir = if let Some(d) = find_flag_value(args, "--dir") {
        PathBuf::from(d)
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?
    };
    let report = crate::corpus_scan::scan_directory(&dir)
        .map_err(|e| format!("corpus_scan: {e}"))?;
    if report.failures == 0 {
        println!("corpus-scan: {} scripts ok", report.total_scripts);
    } else {
        eprintln!(
            "corpus-scan: {} scripts checked, {} failures",
            report.total_scripts, report.failures
        );
        for f in &report.failed_files {
            eprintln!("  {} — {}", f.path, f.message);
        }
    }
    if report.failures == 0 { Ok(0) } else { Ok(1) }
}

/// `run <file.lua> -- <arg1> <arg2>` — execute Lua script with CLI arguments.
fn cmd_run(args: &[String]) -> Result<u8, String> {
    // Find the `--` separator
    let sep = args.iter().position(|a| a == "--");
    let (file_args, script_args): (&[String], &[String]) = if let Some(pos) = sep {
        (&args[..pos], &args[pos + 1..])
    } else {
        (args, &[])
    };
    let path = PathBuf::from(positional(
        file_args,
        0,
        "missing argument: run <file.lua> [-- <args>...]",
    )?);
    let source = read_file(&path)?;
    let engine = crate::LuaEngine::new().map_err(|e| e.to_string())?;
    let base_host = crate::LuaHostFunctions::default();
    let args_vec: Vec<String> = script_args.to_vec();
    let result = engine.eval_with_args(&source, &base_host, &args_vec)
        .map_err(|e| e.to_string())?;
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    println!("rh run ok: {} -> {}", path.display(), result.value);
    Ok(0)
}

/// `task <subcommand>` — task manifest operations.
fn cmd_task(args: &[String]) -> Result<u8, String> {
    if args.is_empty() {
        eprintln!("task: expected subcommand: list, run, show");
        return Ok(1);
    }
    let manifest_path = PathBuf::from(require_flag_value(
        args,
        "--manifest",
        "task requires --manifest <file>",
    )?);
    let manifest = read_file(&manifest_path)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|e| format!("manifest_json: {e}"))?;
    let tasks = parsed["tasks"]
        .as_array()
        .ok_or("task_manifest_tasks: expected array")?;

    match args[0].as_str() {
        "list" => {
            for task in tasks {
                let id = task["id"].as_str().unwrap_or("?");
                let entry = task["entry"].as_str().unwrap_or("?");
                println!("{id}  {entry}");
            }
            Ok(0)
        }
        "show" => {
            let id = args.get(1).ok_or("task show <id>")?;
            for task in tasks {
                if task["id"].as_str() == Some(id) {
                    println!("{}", serde_json::to_string_pretty(task).map_err(|e| e.to_string())?);
                    return Ok(0);
                }
            }
            Err(format!("task_not_found: {id}"))
        }
        "run" => {
            let id = args.get(1).ok_or("task run <id>")?;
            for task in tasks {
                if task["id"].as_str() == Some(id) {
                    let entry = task["entry"]
                        .as_str()
                        .ok_or("task_entry_missing")?;
                    let _cwd = task["cwd"].as_str().unwrap_or(".");
                    let task_args: Vec<String> = task["args"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|v| v.as_str().unwrap_or("").to_string())
                                .collect()
                        })
                        .unwrap_or_default();

                    let source = std::fs::read_to_string(entry)
                        .map_err(|e| format!("read_entry {entry}: {e}"))?;
                    let engine = crate::LuaEngine::new().map_err(|e| e.to_string())?;
                    let base_host = crate::LuaHostFunctions::default();
                    let result = engine
                        .eval_with_args(&source, &base_host, &task_args)
                        .map_err(|e| e.to_string())?;
                    if !result.stdout.is_empty() {
                        print!("{}", result.stdout);
                    }
                    println!("task run ok: {id} -> {}", result.value);
                    return Ok(result.value as u8);
                }
            }
            Err(format!("task_not_found: {id}"))
        }
        _ => {
            eprintln!("task: unknown subcommand: {}", args[0]);
            Ok(1)
        }
    }
}

/// `run-smoke <pack.luac>` — bytecode smoke test (delegates to pack load).
fn cmd_run_smoke(args: &[String]) -> Result<u8, String> {
    let path = PathBuf::from(positional(args, 0, "missing argument: run-smoke <dir>")?);
    // Treat as pack dir
    cmd_pack_load(&[path.to_string_lossy().to_string()])
}

// ── helpers ────────────────────────────────────────────────────────────

fn read_file(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read_file {}: {e}", path.display()))
}

fn print_help() {
    println!("agenterm-lua {} — LuaJIT scripting backend for AgenTerm", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Commands:");
    println!("  check <file.lua>              Syntax check");
    println!("  check-many --manifest <file>   Batch syntax check (--json for JSON output)");
    println!("  corpus-scan [--dir <dir>]      Scan directory for .lua files and check");
    println!("  eval <file.lua>                Evaluate script (with bytecode cache)");
    println!("  hash <file.lua>                Print SHA256 hash of source");
    println!("  run <file.lua> [-- <args>...]  Execute script with CLI arguments");
    println!("  pack build <file.lua> --dir <out>  Build bytecode pack");
    println!("  pack load <dir>                Load and execute pack from directory");
    println!("  qual <file.lua> --dir <out>    Build + load + qualification receipt");
    println!("  qualify <file.lua> --dir <out> Build + load + qualification receipt");
    println!("  version                        Print version");
    println!("  --framed-worker                Run as framed stdio worker");
    println!("  --help, -h                     Print this help");
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

    let engine = crate::LuaEngine::new().map_err(|e| e.to_string())?;
    let host = crate::LuaHostFunctions::default();
    let mut stdout = std::io::stdout().lock();

    match operation {
        "check" => {
            match engine.check(source) {
                Ok(()) => write_json_result(&mut stdout, invocation_id, "check", true, "", None, None)?,
                Err(e) => write_json_result(
                    &mut stdout, invocation_id, "check", false, "",
                    None,
                    Some(&format!("lua_parse: {e}")),
                )?,
            }
            Ok(0)
        }
        "eval" | "run" => {
            match engine.eval(source, &host) {
                Ok(eval_result) => write_json_result(
                    &mut stdout, invocation_id, operation, true, &eval_result.stdout,
                    Some(eval_result.value), None,
                )
                .map(|_| eval_result.value as u8),
                Err(e) => {
                    write_json_result(
                        &mut stdout, invocation_id, operation, false, "",
                        None,
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
                    Some(eval_result.value), None,
                )
                .map(|_| eval_result.value as u8),
                Err(e) => {
                    write_json_result(
                        &mut stdout, invocation_id, operation, false, "",
                        None,
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
    value: Option<i64>,
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
    if let Some(v) = value {
        result["value"] = serde_json::json!(v);
    }
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
