//! agenterm-qjs — CLI verb shape aligned with `agenterm-rh`
//! (`crates/agenterm-rh/src/main.rs`) and `agenterm-lua`
//! (`crates/agenterm-lua/src/main.rs`): `version`, `check`, `eval`, `run`,
//! `hash`, `pack build`/`pack load`, `qualify`, `check-many`,
//! `corpus-scan`, `run-smoke`, `task`, `--help`. Same typed exit-code
//! convention: `0` success, `2` usage/argv error, `1` for a
//! runtime/qualify/pack/corpus-scan failure (matching lua's convention,
//! not rh's `RhError`-typed one — qjs shares lua's untyped
//! `Result<u8, String>` dispatch shape here, see `run` below).
//!
//! Deliberately absent, forever: rh's `caller-inventory`/`compile`/
//! `transpile`/`--worker`/`--internal-incremental-finalize` — all
//! native-codegen tooling for rh's AOT pipeline, out of the "capability
//! alignment" contract by design (see `lib.rs`'s module doc).
//!
//! `task` is an honest stub, not a re-implementation: real task dispatch
//! for the qjs backend already goes through `agenterm::script_backend`'s
//! `try_execute_qjs_invocation` from the root `agenterm` binary/worker
//! (`src/script_worker.rs`), the same place lua's task dispatch lives —
//! `agenterm-lua`'s own `task` subcommand is the same kind of stub, not a
//! coincidence.
//!
//! Argv parsing goes through `agenterm_script_common::cli`'s slice-based
//! helpers (re-exported from `agenterm_qjs`'s lib — `main.rs` is compiled
//! as a `[[bin]]` of the root `agenterm` package, which doesn't depend on
//! `agenterm-script-common` directly, see that re-export's doc). This
//! replaced a local, iterator-based `find_flag_value`/`optional_flag_value`
//! pair that caused a real bug: `optional_flag_value` internally collected
//! (draining) its `&mut impl Iterator`, so calling it twice on the same
//! iterator for two different flags always lost the second one (see the
//! regression tests at the bottom of this file, still exercised here
//! against the shared `find_flag_value`).

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use agenterm_qjs::{
    QJS_VERSION, QjsError, QjsHostFunctions, check, check_with_project_validation,
    eval_entry, eval_entry_with_host, eval_module_entry_with_host, find_flag_value, has_flag,
    parse_check_many_cli, positional, read_manifest, require_flag_value, run_check_many,
    wants_module_mode,
};

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<u8, QjsError> {
    let Some(command) = args.first() else {
        print_usage();
        return Ok(0);
    };
    let rest = &args[1..];

    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!("agenterm-qjs {QJS_VERSION}");
        }
        "check" => {
            let path = PathBuf::from(
                positional(rest, 0, "usage: agenterm-qjs check <file.js>")
                    .map_err(QjsError::Check)?,
            );
            let project_root = find_flag_value(&rest[1..], "--project-root").map(PathBuf::from);
            let source = read_source(&path)?;
            let label = path.display().to_string();
            // No implicit default here (unlike `eval`/`run` below): plain
            // `check()` intentionally has no project-root concept (see its
            // doc comment), so a script using import/export must opt in
            // with `--project-root` explicitly, same as `check-many`
            // already requires — consistent with that established
            // convention rather than inventing a silent default for one
            // verb and not the other.
            match project_root {
                Some(root) => check_with_project_validation(&source, &path, &root)?,
                None => check(&source, &label)?,
            }
            println!("qjs check ok: {label}");
        }
        "eval" => {
            let path = PathBuf::from(
                positional(rest, 0, "usage: agenterm-qjs eval <file.js>")
                    .map_err(QjsError::Check)?,
            );
            let project_root = find_flag_value(&rest[1..], "--project-root").map(PathBuf::from);
            let source = read_source(&path)?;
            let label = path.display().to_string();
            let outcome = if wants_module_mode(&source) {
                let root = resolve_project_root(project_root, &path);
                eval_module_entry_with_host(&source, &path, &root, &QjsHostFunctions::default())?
            } else {
                eval_entry(&source, &label)?
            };
            let rendered = render_value(outcome.value.as_ref());
            if !outcome.stdout.is_empty() {
                print!("{}", outcome.stdout);
            }
            println!("qjs eval ok: {label} -> {rendered}");
        }
        "hash" => {
            let path = PathBuf::from(
                positional(rest, 0, "usage: agenterm-qjs hash <file.js>")
                    .map_err(QjsError::Check)?,
            );
            let source = read_source(&path)?;
            println!("{}  {}", agenterm_qjs::hash_source(&source), path.display());
        }
        "run" => {
            return run_run_command(rest);
        }
        "pack" => {
            return run_pack_command(rest);
        }
        "qualify" => {
            return run_qualify_command(rest);
        }
        "check-many" => {
            return run_check_many_command(rest);
        }
        "corpus-scan" => {
            return run_corpus_scan_command(rest);
        }
        "run-smoke" => {
            // Bytecode smoke test — delegates to `pack load`, same as
            // `agenterm-lua`'s `cmd_run_smoke` (a pack dir is what both
            // engines actually load, not a raw bytecode file). The
            // presence check happens here (for `run-smoke`'s own usage
            // message) before delegating, so `run_pack_load`'s own
            // "missing dir" case can never actually fire below.
            positional(rest, 0, "usage: agenterm-qjs run-smoke <dir>").map_err(QjsError::Check)?;
            return run_pack_load(rest);
        }
        "task" => {
            let joined = rest.join(" ");
            eprintln!(
                "task: use `agenterm task {joined}` (root binary; set \
                 AGENTERM_SCRIPT_BACKEND=qjs to route it through this engine) — \
                 agenterm-qjs itself doesn't re-implement task dispatch, see \
                 plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M3"
            );
            return Ok(0);
        }
        "--help" | "-h" | "help" => print_usage(),
        other => {
            return Err(QjsError::Check(format!(
                "unknown command `{other}`; try check | eval | run | hash | pack | qualify | \
                 check-many | corpus-scan | run-smoke | task | version"
            )));
        }
    }

    Ok(0)
}

/// `run <file.js> [--project-root DIR] [-- <args>...]` — evaluate with CLI
/// arguments wired to `__host.args_len`/`__host.arg`, mirroring
/// `agenterm-lua`'s `cmd_run`. `--project-root` (like `eval`'s) only
/// matters for scripts using `import`/`export`; it's scanned for in
/// `file_args` only, never in the script's own trailing `-- <args>`, so a
/// script argument that happens to be the literal string `--project-root`
/// isn't misread as this command's own flag.
fn run_run_command(args: &[String]) -> Result<u8, QjsError> {
    let sep = args.iter().position(|a| a == "--");
    let (file_args, script_args): (&[String], &[String]) = match sep {
        Some(pos) => (&args[..pos], &args[pos + 1..]),
        None => (&args[..], &[]),
    };
    let path = PathBuf::from(
        positional(
            file_args,
            0,
            "usage: agenterm-qjs run <file.js> [-- <args>...]",
        )
        .map_err(QjsError::Check)?,
    );
    let project_root = find_flag_value(&file_args[1..], "--project-root").map(PathBuf::from);
    let source = read_source(&path)?;
    let label = path.display().to_string();

    let mut host = QjsHostFunctions::default();
    let script_args = script_args.to_vec();
    let args_for_len = script_args.clone();
    host.args_len = Some(std::sync::Arc::new(move || args_for_len.len() as i64));
    let args_for_arg = script_args.clone();
    host.arg = Some(std::sync::Arc::new(move |index: i64| {
        usize::try_from(index)
            .ok()
            .and_then(|i| args_for_arg.get(i))
            .cloned()
            .ok_or_else(|| format!("argument {index} is unavailable"))
    }));

    let outcome = if wants_module_mode(&source) {
        let root = resolve_project_root(project_root, &path);
        eval_module_entry_with_host(&source, &path, &root, &host)?
    } else {
        eval_entry_with_host(&source, &label, &host)?
    };
    if !outcome.stdout.is_empty() {
        print!("{}", outcome.stdout);
    }
    println!(
        "qjs run ok: {label} -> {}",
        render_value(outcome.value.as_ref())
    );
    Ok(0)
}

/// `pack build <file.js> --dir <out> [--project-root DIR]` / `pack load <dir>`.
/// A script using `import`/`export` requires `--project-root` (same
/// explicit-only convention as `check`, not `eval`/`run`'s friendlier
/// default — a pack build is a durable artifact, worth being explicit
/// about what it was scoped to) and builds a self-contained
/// [`agenterm_qjs::QjsModulePack`] instead of the single-file
/// [`agenterm_qjs::QjsPack`].
fn run_pack_command(args: &[String]) -> Result<u8, QjsError> {
    let Some(subcommand) = args.first() else {
        return Err(QjsError::Check(
            "usage: agenterm-qjs pack build <file.js> --dir <out> [--project-root DIR] | \
             pack load <dir>"
                .into(),
        ));
    };
    let rest = &args[1..];
    match subcommand.as_str() {
        "build" => {
            let path = PathBuf::from(
                positional(rest, 0, "usage: agenterm-qjs pack build <file.js>")
                    .map_err(QjsError::Check)?,
            );
            // Slice-based lookups — no iterator to accidentally drain, so
            // both flags can be looked up independently from the same
            // slice (see `find_flag_value`'s doc in
            // `agenterm-script-common/src/cli.rs` for the bug class this
            // structurally rules out).
            let flags = &rest[1..];
            let dir = find_flag_value(flags, "--dir")
                .map(PathBuf::from)
                .ok_or_else(|| QjsError::Check("pack build requires --dir <out>".into()))?;
            let project_root = find_flag_value(flags, "--project-root").map(PathBuf::from);
            let source = read_source(&path)?;
            if wants_module_mode(&source) {
                let root = project_root.ok_or_else(|| {
                    QjsError::Check(
                        "pack build: source uses import/export; requires --project-root DIR"
                            .into(),
                    )
                })?;
                let manifest_path =
                    agenterm_qjs::build_module_pack_dir(&source, &path, &root, &dir)
                        .map_err(QjsError::Check)?;
                println!("pack build ok (module): {}", path.display());
                println!("  manifest: {}", manifest_path.display());
            } else {
                agenterm_qjs::build_pack_dir(&source, &dir).map_err(QjsError::Check)?;
                println!("pack build ok: {}", path.display());
                println!("  output: {}", dir.display());
            }
            Ok(0)
        }
        "load" => run_pack_load(rest),
        other => Err(QjsError::Check(format!(
            "pack: unknown subcommand `{other}`; try build or load"
        ))),
    }
}

/// `pack load <dir>` — shared by `pack load` and `run-smoke` (lua's
/// `run-smoke` is the same delegation, see `cmd_run_smoke`'s doc comment).
/// Dispatches on the manifest's own `schema` field (single-file
/// `agenterm.qjs-pack-manifest/v1` vs. multi-file
/// `agenterm.qjs-module-pack-manifest/v1`) — both kinds write to the same
/// `manifest.json` filename inside their pack directory, so the schema
/// string is what actually distinguishes them, not the directory shape.
fn run_pack_load(args: &[String]) -> Result<u8, QjsError> {
    let dir = PathBuf::from(
        positional(args, 0, "usage: agenterm-qjs pack load <dir>").map_err(QjsError::Check)?,
    );
    let host = QjsHostFunctions::default();
    if pack_manifest_schema(&dir)?.starts_with("agenterm.qjs-module-pack-manifest/") {
        let pack = agenterm_qjs::QjsModulePack::load(&dir).map_err(QjsError::Check)?;
        let result = pack.eval(&host)?;
        if !result.stdout.is_empty() {
            print!("{}", result.stdout);
        }
        println!(
            "pack load ok (module, {} files): {} -> {}",
            pack.manifest.files.len(),
            dir.display(),
            render_value(result.value.as_ref())
        );
    } else {
        let pack = agenterm_qjs::QjsPack::load(&dir).map_err(QjsError::Check)?;
        let result = pack.eval(&host)?;
        if !result.stdout.is_empty() {
            print!("{}", result.stdout);
        }
        println!(
            "pack load ok: {} -> {}",
            dir.display(),
            render_value(result.value.as_ref())
        );
    }
    Ok(0)
}

/// Peek a pack directory's `manifest.json` for its `schema` field, without
/// committing to parsing it as either manifest shape yet.
fn pack_manifest_schema(dir: &Path) -> Result<String, QjsError> {
    let bytes = fs::read(dir.join("manifest.json"))
        .map_err(|err| QjsError::Check(format!("pack_manifest_read: {err}")))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| QjsError::Check(format!("pack_manifest_parse: {err}")))?;
    value
        .get("schema")
        .and_then(|schema| schema.as_str())
        .map(str::to_owned)
        .ok_or_else(|| QjsError::Check("pack_manifest_missing_schema".into()))
}

/// `corpus-scan [--dir <dir>]` — scan a directory for `.js`/`.mjs` files
/// and check syntax, mirroring `agenterm-lua`'s `cmd_corpus_scan`. Unlike
/// lua's corpus-scan (which silently falls back to cwd if `--dir` has no
/// following value), qjs's has always distinguished "no `--dir`" from
/// "`--dir` with nothing after it" — the latter is a hard error. Preserved
/// here via `has_flag` + `require_flag_value` rather than the
/// absent/no-value-collapsing `find_flag_value`.
fn run_corpus_scan_command(args: &[String]) -> Result<u8, QjsError> {
    let dir = if has_flag(args, "--dir") {
        // `usage` is unreachable here: `has_flag` already guarantees the
        // flag is present, so the only way `require_flag_value` can fail
        // is the "no value follows" branch.
        PathBuf::from(require_flag_value(args, "--dir", "unreachable").map_err(QjsError::Check)?)
    } else {
        std::env::current_dir()
            .map_err(|err| QjsError::Check(format!("corpus_scan_cwd: {err}")))?
    };
    let report = agenterm_qjs::scan_directory(&dir)
        .map_err(|err| QjsError::Check(format!("corpus_scan: {err}")))?;
    if report.failures == 0 {
        println!("corpus-scan: {} scripts ok", report.total_scripts);
        Ok(0)
    } else {
        eprintln!(
            "corpus-scan: {} scripts checked, {} failures",
            report.total_scripts, report.failures
        );
        for failed in &report.failed_files {
            eprintln!("  {} — {}", failed.path, failed.message);
        }
        Ok(1)
    }
}

/// `qualify <file.js> --dir <out>` — build + load + entry → receipt.
/// `qualify <file.js> --dir <out> [--project-root DIR]`. Same
/// explicit-`--project-root`-required convention as `pack build`/`check`
/// for scripts using `import`/`export`.
fn run_qualify_command(args: &[String]) -> Result<u8, QjsError> {
    let path = PathBuf::from(
        positional(args, 0, "usage: agenterm-qjs qualify <file.js>")
            .map_err(QjsError::Check)?,
    );
    let flags = &args[1..];
    let dir = find_flag_value(flags, "--dir")
        .map(PathBuf::from)
        .ok_or_else(|| QjsError::Check("qualify requires --dir <out>".into()))?;
    let project_root = find_flag_value(flags, "--project-root").map(PathBuf::from);
    let source = read_source(&path)?;
    let host = QjsHostFunctions::default();
    let receipt_path = dir.join("receipt.json");
    if wants_module_mode(&source) {
        let root = project_root.ok_or_else(|| {
            QjsError::Check(
                "qualify: source uses import/export; requires --project-root DIR".into(),
            )
        })?;
        let receipt = agenterm_qjs::qualify_module_pack_dir(&source, &path, &root, &dir, &host)
            .map_err(QjsError::Check)?;
        receipt.write(&receipt_path).map_err(QjsError::Check)?;
        println!(
            "qualify ok (module, {} files): {} -> {}",
            receipt.file_count,
            path.display(),
            render_value(receipt.entry_value.as_ref())
        );
    } else {
        let receipt =
            agenterm_qjs::qualify_pack_dir(&source, &dir, &host).map_err(QjsError::Check)?;
        receipt.write(&receipt_path).map_err(QjsError::Check)?;
        println!(
            "qualify ok: {} -> {}",
            path.display(),
            render_value(receipt.entry_value.as_ref())
        );
    }
    println!("  receipt: {}", receipt_path.display());
    Ok(0)
}

fn run_check_many_command(args: &[String]) -> Result<u8, QjsError> {
    let parsed = parse_check_many_cli(args.iter().cloned())?;
    let manifest = read_manifest(&parsed.manifest_path)?;
    let report = run_check_many(manifest, parsed.options);
    if parsed.json {
        let encoded = serde_json::to_string_pretty(&report)
            .map_err(|err| QjsError::Check(err.to_string()))?;
        println!("{encoded}");
    } else if report.ok {
        println!("OK ({} files)", report.checked_files);
    } else {
        for failure in &report.failures {
            eprintln!(
                "{}: {}",
                failure.path,
                serde_json::json!({
                    "code": failure.code,
                    "message": failure.message,
                    "invocation_id": failure.invocation_id,
                    "exit_class": failure.exit_class,
                })
            );
        }
    }
    Ok(report.exit_code())
}

fn render_value(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "undefined".to_owned(),
    }
}

fn read_source(path: &PathBuf) -> Result<String, QjsError> {
    fs::read_to_string(path).map_err(|err| QjsError::Check(format!("{}: {err}", path.display())))
}

/// `--project-root`, if given; otherwise the entry file's own parent
/// directory — a script using `import`/`export` almost always wants its
/// siblings resolved relative to itself, and requiring `--project-root`
/// on every single-invocation `eval`/`run` for the common case would be
/// friction for no real benefit. `check` deliberately does NOT use this
/// default (see its handler) — it requires `--project-root` explicitly,
/// matching `check-many`'s existing convention.
fn resolve_project_root(explicit: Option<PathBuf>, entry_path: &Path) -> PathBuf {
    explicit.unwrap_or_else(|| {
        entry_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

fn print_usage() {
    println!(
        "agenterm-qjs {QJS_VERSION} — QuickJS script engine, capability-aligned with agenterm-rh\n\n\
Usage:\n  \
agenterm-qjs check <file.js> [--project-root DIR]\n  \
agenterm-qjs eval <file.js> [--project-root DIR]\n  \
agenterm-qjs run <file.js> [--project-root DIR] [-- <args>...]\n  \
agenterm-qjs hash <file.js>\n  \
agenterm-qjs pack build <file.js> --dir <out> [--project-root DIR]\n  \
agenterm-qjs pack load <dir>\n  \
agenterm-qjs qualify <file.js> --dir <out> [--project-root DIR]\n  \
agenterm-qjs check-many --manifest <file.json> [--project-root DIR] [--timeout-ms N] [--json]\n  \
agenterm-qjs corpus-scan [--dir <dir>]\n  \
agenterm-qjs run-smoke <pack-dir>\n  \
agenterm-qjs task ...  (stub — see plan/plan-v0.1.16.md \u{a7}1 Rh, QJS-M3)\n  \
agenterm-qjs version\n\n\
Scripts using top-level `import`/`export` are detected automatically and\n\
routed through a real (root-confined) ES module resolver — see\n\
plan/design-qjs-module-imports.md. All of check/eval/run/check-many/pack/\n\
qualify support this now; `pack build`/`qualify` require --project-root\n\
explicitly for such scripts (same convention as `check`) and produce a\n\
self-contained multi-file pack (its own manifest schema, distinct from the\n\
single-file pack format) that copies its whole import graph in, so `pack\n\
load` never needs the original --project-root again."
    );
}

#[cfg(test)]
mod tests {
    use agenterm_qjs::find_flag_value;

    #[test]
    fn find_flag_value_reads_multiple_distinct_flags_from_one_collected_slice() {
        // Regression test for a real bug caught by manual CLI testing:
        // `pack build --dir X --project-root Y` silently lost Y because
        // an earlier version called two iterator-draining helpers back to
        // back on the same `args` iterator — the first one (searching for
        // `--dir`) collected (and thereby drained) the whole tail, so the
        // second call (searching for `--project-root`) always found
        // nothing. `find_flag_value` operating on an already-collected
        // slice, called twice on the SAME slice, is the fix — this test
        // is what `run_pack_command`/`run_qualify_command` actually rely
        // on now (via the shared `agenterm_script_common::cli` helpers).
        let collected = vec![
            "--dir".to_owned(),
            "out".to_owned(),
            "--project-root".to_owned(),
            "proj".to_owned(),
        ];
        assert_eq!(find_flag_value(&collected, "--dir"), Some("out"));
        assert_eq!(find_flag_value(&collected, "--project-root"), Some("proj"));
    }

    #[test]
    fn find_flag_value_order_independent() {
        let collected = vec![
            "--project-root".to_owned(),
            "proj".to_owned(),
            "--dir".to_owned(),
            "out".to_owned(),
        ];
        assert_eq!(find_flag_value(&collected, "--dir"), Some("out"));
        assert_eq!(find_flag_value(&collected, "--project-root"), Some("proj"));
    }

    #[test]
    fn find_flag_value_absent_flag_is_none() {
        let collected = vec!["--dir".to_owned(), "out".to_owned()];
        assert_eq!(find_flag_value(&collected, "--project-root"), None);
    }
}
