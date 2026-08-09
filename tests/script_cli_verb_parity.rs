//! Cross-engine **CLI-level** verb parity for the four script-engine binaries
//! (`agenterm-rh`, `agenterm-lua`, `agenterm-qjs`, `agenterm-sql` — all
//! `[[bin]]` entries of the root `agenterm` package, see root `Cargo.toml`).
//!
//! The capability-alignment contract (`plan/plan-v0.1.16.md` §1 Rh: "CLI
//! 动词对齐 ... 同样的 typed JSON 输出、退出码") has lib-level parity coverage
//! already (`tests/script_engine_parity.rs` drives each engine's
//! `check_many::{read_manifest, run_check_many}` directly), but nothing
//! before this file actually spawned the four real binaries and compared
//! their *observable* CLI behavior — argv shape, stdout, and exit code.
//! This file closes that gap. It does **not** duplicate the lib-level
//! `CheckManyReport` field-by-field assertions from `script_engine_parity.rs`
//! — it treats each engine as an opaque child process and asserts only what
//! a caller of the CLI can see: exit status and stdout/stderr substrings.
//!
//! `env!("CARGO_BIN_EXE_agenterm-*")` is used throughout (never
//! `cargo build` or a hand-rolled `target/` path) — because all four are
//! `[[bin]]`s of *this* package, Cargo builds them automatically before
//! running this integration test binary.
//!
//! # Verb × engine availability map
//!
//! Read from `crates/agenterm-{rh,lua,qjs,sql}/src/main.rs`'s verb dispatch.
//!
//! | verb                            | rh                                            | lua                                   | qjs                                              | sql                                     |
//! |----------------------------------|------------------------------------------------|----------------------------------------|----------------------------------------------------|-------------------------------------------|
//! | `version`                        | real                                            | real                                    | real                                                | real                                       |
//! | `check`                           | real — dedicated `run_public_check_command`, intercepted in `main()` *before* `run()`; supports `--project-root DIR`, `--json` | real — plain `check <file>`, no flags   | real — plain, or `--project-root DIR` (required only for `import`/`export` sources) | real — plain `check <file>`, no flags     |
//! | `check-many`                      | real (shared `agenterm_script_common::check_many` driver) | real (same shared driver)              | real (same shared driver)                          | real (same shared driver)                 |
//! | `corpus-scan`                     | real                                            | real                                    | real                                                | real                                       |
//! | `caller-inventory`                | real — rh-only, AOT-toolchain reporting          | absent (unknown command)                | absent                                              | absent                                     |
//! | `transpile` / `compile`           | real — rh-only AOT pipeline                      | absent                                  | absent                                              | absent                                     |
//! | `eval`                            | real                                             | real                                    | real                                                | **stub** — exits 2, "not implemented"      |
//! | `run`                             | real (`--project-root`, `--timeout-ms`, ..., `-- ARGS`) | real (`-- ARGS`)              | real (`--project-root`, `-- ARGS`)                 | **stub** — exits 2                          |
//! | `run-smoke`                       | real (dlopen native)                            | real (delegates to `pack load`)         | real (delegates to `pack load`)                    | absent (unknown command)                   |
//! | `pack build` / `pack load`        | real                                             | real                                    | real (module-aware, self-contained multi-file pack) | **stub** — exits 2 (whole `pack` verb, no subcommand dispatch) |
//! | `qualify`                         | real                                             | real                                    | real (module-aware)                                | **stub** — exits 2                          |
//! | `hash`                            | real                                             | real                                    | real                                                | absent — not a match arm at all, falls into `other => unknown command` |
//! | `task`                            | real dispatch (`main()` intercepts `task` *before* `run()`, routes through `agenterm::run_script_entry_with_args`) | stub — its own tiny `--manifest`-driven list/show/run, not the real Script Runtime | stub — prints a redirect-to-`agenterm task` message, **exits 0** | **stub** — exits 2 via the same `not_implemented_stub` as `eval`/`run`/`pack`/`qualify` (unlike qjs's `task`, which is a 0-exit informational stub, not a reserved-error one) |
//! | `--worker` / `--framed-worker`    | both real (legacy JSON + framed worker protocols) | `--framed-worker` only (no `--worker`) | absent | absent |
//! | `--internal-incremental-finalize` | real, rh-only, internal                          | absent                                  | absent                                              | absent                                     |
//!
//! # Exit-code conventions (read from each `main()`, not assumed)
//!
//! - **rh**: `main()` calls a typed `run() -> Result<(), RhError>` for most
//!   verbs: `Ok(())` → `ExitCode::SUCCESS` (0), `Err(_)` → eprint + generic
//!   `ExitCode::FAILURE` (1) — this is the path an *unknown verb* takes.
//!   `check` and `check-many` are special-cased in `main()` to their own
//!   `Result<u8, RhError>` handlers, translated by `public_command_exit_code`:
//!   `Ok(code)` → `ExitCode::from(code)` (so `check`'s own internal
//!   success/failure distinction is `Ok(0)`/`Ok(1)`, i.e. a syntax error is
//!   an `Ok`, not an `Err`), `Err(_)` (bad argv, unreadable manifest, etc.)
//!   → `ExitCode::from(2)`.
//! - **lua**: `main()` calls `dispatch() -> Result<u8, String>`; `Ok(code)`
//!   is passed straight to `std::process::exit`, `Err(_)` is **hardcoded to
//!   1** regardless of failure kind — lua has no typed 2/3 distinction at
//!   the top level the way rh/qjs/sql do. Notably, `cmd_check_many` also
//!   special-cases its *own* success/failure as `Ok(0)`/`Ok(1)` (ignoring
//!   `CheckManyReport::exit_code()`'s finer 2/3 taxonomy for
//!   configuration/limit failures), but a `read_manifest` failure (e.g. a
//!   wrong `kind`) happens before a report even exists and takes the
//!   generic `Err(String)` → hardcoded-1 path instead.
//! - **qjs / sql** (aligned 2026-08, see below): `main()` calls
//!   `run() -> Result<u8, QjsError | SqlError>`; `Ok(code)` →
//!   `ExitCode::from(code)`. `Err(_)` is no longer a blanket `2` — `main()`
//!   now reads the error's variant: `Parse`/`Check` (both script-level: the
//!   root cause is the script/pack content — syntax error, missing
//!   `entry()`, a thrown exception, a tampered pack, ...) → `1`; `Usage`
//!   (usage/configuration-level: bad argv, unknown verb, missing/unreadable
//!   flag or manifest, a `--project-root` that doesn't resolve/confine) →
//!   `2`. This mirrors the shared
//!   `agenterm_script_common::check_many::CheckManyReport::exit_code()`
//!   taxonomy's own `"script"` → 1 / `"configuration"` → 2 split, applied
//!   at the single-invocation CLI layer, not just inside `check-many`. See
//!   `crates/agenterm-{qjs,sql}/src/main.rs`'s module docs and
//!   `crates/agenterm-{qjs,sql}/src/error.rs`'s docs for the full
//!   call-site-by-call-site classification.
//!
//! # Divergences this file locks down (found by running the real binaries)
//!
//! | scenario                                   | rh | lua | qjs | sql |
//! |----------------------------------------------|----|-----|-----|-----|
//! | `check <broken file>`                         | 1  | 1   | 1   | 1   |
//! | `check-many` with another engine's manifest `kind` | 2  | 1   | 2   | 2   |
//! | unknown verb                                   | 1  | 1   | 2   | 2   |
//!
//! `check <broken file>` is now unified across all four engines at exit 1
//! (2026-08: qjs/sql used to exit 2 here — a real bug, since it made a
//! syntax error indistinguishable from a usage error on those two engines;
//! see `crates/agenterm-{qjs,sql}/src/main.rs`'s module docs for the fix).
//! qjs/sql now correctly distinguish script-level (1) from
//! usage/configuration-level (2) failures at every CLI call site — the two
//! remaining rows are not qjs/sql being wrong, they're **rh/lua's own,
//! separate, tracked debt**: both fold *all* top-level failures (unknown
//! verb, bad manifest `kind`, ...) into a single generic 1-or-Ok(1) path
//! with no usage/script distinction at all, so "exit code == 1 means a
//! script-level failure, not a usage error" still does NOT hold for rh's
//! unknown-verb path or lua's `check-many`/unknown-verb paths. Fixing that
//! is out of scope here deliberately (rh risks Lnx-side wrapper breakage;
//! lua's track is separate) — tracked as rh/lua's own residue, not
//! re-litigated by this file.

use std::path::Path;
use std::process::{Command, Output};

const RH_BIN: &str = env!("CARGO_BIN_EXE_agenterm-rh");
const LUA_BIN: &str = env!("CARGO_BIN_EXE_agenterm-lua");
const QJS_BIN: &str = env!("CARGO_BIN_EXE_agenterm-qjs");
const SQL_BIN: &str = env!("CARGO_BIN_EXE_agenterm-sql");

/// One engine's fixed CLI facts. Fixtures (`valid_source`/`broken_source`)
/// and `kind` are copied verbatim from `tests/script_engine_parity.rs`'s
/// `EngineSpec` consts (`RH`/`LUA`/`QJS`/`SQL`) — proven-valid/proven-broken
/// per-engine sources, not reinvented here.
struct Engine {
    name: &'static str,
    bin: &'static str,
    ext: &'static str,
    kind: &'static str,
    valid_source: &'static str,
    broken_source: &'static str,
    /// Actual observed exit code of `<bin> check <broken_source_file>`.
    check_broken_exit: i32,
    /// Actual observed exit code of `<bin> definitely-not-a-verb`.
    unknown_verb_exit: i32,
    /// Actual observed exit code of `<bin> check-many` given a manifest
    /// tagged with a *different* engine's `kind`.
    wrong_kind_check_many_exit: i32,
}

const RH: Engine = Engine {
    name: "rh",
    bin: RH_BIN,
    ext: "rh",
    kind: "agenterm-rh-check-manifest",
    valid_source: "40 + 2",
    broken_source: "fn {{{",
    check_broken_exit: 1,
    unknown_verb_exit: 1,
    wrong_kind_check_many_exit: 2,
};

const LUA: Engine = Engine {
    name: "lua",
    bin: LUA_BIN,
    ext: "lua",
    kind: "agenterm-lua-check-manifest",
    valid_source: "return 42",
    broken_source: "return !!",
    check_broken_exit: 1,
    unknown_verb_exit: 1,
    wrong_kind_check_many_exit: 1,
};

const QJS: Engine = Engine {
    name: "qjs",
    bin: QJS_BIN,
    ext: "js",
    kind: "agenterm-qjs-check-manifest",
    valid_source: "function entry() { return 42; }",
    broken_source: "this is not valid js (((",
    // 2026-08: was 2 (blanket `Err -> 2` in `main()`). `check <broken
    // file>` is a script-level failure (`QjsError::Parse`) — aligned to
    // exit 1, matching the shared `check_many` taxonomy's `script` ->  1
    // convention. See `crates/agenterm-qjs/src/main.rs`'s module doc.
    check_broken_exit: 1,
    // Unknown verb is a usage-level failure (`QjsError::Usage`) — stays 2.
    unknown_verb_exit: 2,
    wrong_kind_check_many_exit: 2,
};

const SQL: Engine = Engine {
    name: "sql",
    bin: SQL_BIN,
    ext: "sql",
    kind: "agenterm-sql-check-manifest",
    valid_source: "SELECT 1;",
    broken_source: "SELEC 1 FORM;",
    // 2026-08: was 2, same fix as qjs (see that const's comment) —
    // `SqlError::Parse` is script-level, now exits 1.
    check_broken_exit: 1,
    // Unknown verb is a usage-level failure (`SqlError::Usage`) — stays 2.
    unknown_verb_exit: 2,
    wrong_kind_check_many_exit: 2,
};

fn engines() -> [Engine; 4] {
    [RH, LUA, QJS, SQL]
}

/// Spawn `bin` with `args`, scrubbing `AGENTERM_SCRIPT_BACKEND` so host env
/// (e.g. a shared-checkout CI runner or a developer's shell) never skews
/// which backend a verb resolves to.
fn run(bin: &str, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .env_remove("AGENTERM_SCRIPT_BACKEND")
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn {bin} {args:?}: {err}"))
}

/// Same as [`run`], but also sets the child's working directory.
///
/// History: this helper exists because the first run of this suite caught
/// `agenterm-lua`'s `cmd_check_many` silently IGNORING `--project-root`
/// (it hand-parsed only `--manifest`/`--json` and passed
/// `CheckManyOptions::default()` to `run_check_many`, so manifest labels
/// resolved against the process CWD — a wrapper script relying on the
/// aligned-CLI contract was silently broken, not rejected). That bug is
/// now FIXED (`cmd_check_many` goes through the shared
/// `parse_check_many_cli` like rh/qjs/sql), and
/// [`check_many_project_root_honored_from_foreign_cwd`] locks the fix
/// cross-engine. Setting `cwd` here is kept as belt-and-braces isolation:
/// the check-many scenarios should pass regardless of where the child
/// process happens to run.
fn run_in_dir(bin: &str, args: &[&str], dir: &Path) -> Output {
    Command::new(bin)
        .args(args)
        .current_dir(dir)
        .env_remove("AGENTERM_SCRIPT_BACKEND")
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn {bin} {args:?} in {dir:?}: {err}"))
}

fn write_manifest(dir: &Path, kind: &str, files: &[&str]) -> std::path::PathBuf {
    let path = dir.join("manifest.json");
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": kind,
        "files": files,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

// ── 1. version ───────────────────────────────────────────────────────────

#[test]
fn version_verb_works_everywhere() {
    for engine in engines() {
        let output = run(engine.bin, &["version"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: version should exit 0; stderr={}",
            engine.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "{}: version should print non-empty stdout",
            engine.name
        );
    }
}

// ── 2. check <valid> ────────────────────────────────────────────────────

#[test]
fn check_valid_exits_zero() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(format!("valid.{}", engine.ext));
        std::fs::write(&file, engine.valid_source).unwrap();

        // All four engines accept the plain `check <file>` shape for a
        // source with no import/export — the argv difference (rh's
        // `--project-root`/`--json`, qjs's `--project-root`) only matters
        // for module-mode sources, not exercised by these trivial fixtures.
        let output = run(engine.bin, &["check", file.to_str().unwrap()]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: check <valid> should exit 0; stderr={}",
            engine.name,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

// ── 3. check <broken> ───────────────────────────────────────────────────

#[test]
fn check_broken_exits_nonzero() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(format!("broken.{}", engine.ext));
        std::fs::write(&file, engine.broken_source).unwrap();

        let output = run(engine.bin, &["check", file.to_str().unwrap()]);
        assert_eq!(
            output.status.code(),
            Some(engine.check_broken_exit),
            "{}: check <broken> exit code diverged from the recorded contract; \
             stdout={} stderr={}",
            engine.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

// ── 4. check-many taxonomy ──────────────────────────────────────────────

#[test]
fn check_many_taxonomy_parity() {
    let engines = engines();
    for (index, engine) in engines.iter().enumerate() {
        // (a) all-green manifest -> exit 0, "ok": true in stdout JSON.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let file_a = format!("a.{}", engine.ext);
            let file_b = format!("b.{}", engine.ext);
            std::fs::write(dir.path().join(&file_a), engine.valid_source).unwrap();
            std::fs::write(dir.path().join(&file_b), engine.valid_source).unwrap();
            let manifest = write_manifest(dir.path(), engine.kind, &[&file_a, &file_b]);

            let output = run_in_dir(
                engine.bin,
                &[
                    "check-many",
                    "--manifest",
                    manifest.to_str().unwrap(),
                    "--project-root",
                    dir.path().to_str().unwrap(),
                    "--json",
                ],
                dir.path(),
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}: all-green check-many should exit 0; stdout={stdout} stderr={}",
                engine.name,
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                stdout.contains("\"ok\": true"),
                "{}: all-green check-many stdout should report ok:true; got {stdout}",
                engine.name
            );
        }

        // (b) one syntax-error file -> exit 1, "ok": false in stdout JSON.
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let file_ok = format!("ok.{}", engine.ext);
            let file_bad = format!("bad.{}", engine.ext);
            std::fs::write(dir.path().join(&file_ok), engine.valid_source).unwrap();
            std::fs::write(dir.path().join(&file_bad), engine.broken_source).unwrap();
            let manifest = write_manifest(dir.path(), engine.kind, &[&file_ok, &file_bad]);

            let output = run_in_dir(
                engine.bin,
                &[
                    "check-many",
                    "--manifest",
                    manifest.to_str().unwrap(),
                    "--project-root",
                    dir.path().to_str().unwrap(),
                    "--json",
                ],
                dir.path(),
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{}: check-many with a syntax error should exit 1 (shared \
                 CheckManyReport::exit_code() convention); stdout={stdout} stderr={}",
                engine.name,
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                stdout.contains("\"ok\": false"),
                "{}: check-many with a syntax error should report ok:false; got {stdout}",
                engine.name
            );
        }

        // (c) manifest tagged with a DIFFERENT engine's `kind` -> rejected,
        // nonzero, per-engine actual code recorded (see module doc's
        // divergence table — this is where rh/qjs/sql diverge from lua).
        {
            let other = &engines[(index + 1) % engines.len()];
            let dir = tempfile::tempdir().expect("tempdir");
            let manifest = write_manifest(dir.path(), other.kind, &["x"]);

            let output = run(
                engine.bin,
                &[
                    "check-many",
                    "--manifest",
                    manifest.to_str().unwrap(),
                    "--project-root",
                    dir.path().to_str().unwrap(),
                ],
            );
            assert_eq!(
                output.status.code(),
                Some(engine.wrong_kind_check_many_exit),
                "{}: check-many given {}'s manifest kind (`{}`) diverged from the \
                 recorded contract; stdout={} stderr={}",
                engine.name,
                other.name,
                other.kind,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

// ── 5. unknown verb ─────────────────────────────────────────────────────

#[test]
fn unknown_verb_rejected() {
    for engine in engines() {
        let output = run(engine.bin, &["definitely-not-a-verb"]);
        assert_eq!(
            output.status.code(),
            Some(engine.unknown_verb_exit),
            "{}: unknown verb exit code diverged from the recorded contract; stderr={}",
            engine.name,
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("unknown command"),
            "{}: expected stderr to mention the unknown command; got {stderr}",
            engine.name
        );
    }
}

// ── 5b. qjs's `task` stub is an honest failure, not a silent success ────

/// `agenterm-qjs task` is an informational stub (real task dispatch lives
/// in the root `agenterm` binary, see `main.rs`'s module doc) — it used to
/// print its redirect message and exit `0`, which is a lie to any
/// automation caller that only checks the exit code. 2026-08: aligned with
/// `agenterm-sql`'s reserved-verb stubs (see [`sql_reserved_verbs_stable_error`]
/// below) to exit `2` instead, keeping the same helpful message.
#[test]
fn qjs_task_stub_exits_nonzero_with_redirect_message() {
    let output = run(QJS_BIN, &["task", "list"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "qjs task: honest stub is documented to exit 2, not silently succeed; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("agenterm task list"),
        "qjs task: expected the redirect message to name the equivalent root-binary \
         invocation; got {stderr}"
    );
    assert!(
        stderr.contains("AGENTERM_SCRIPT_BACKEND=qjs"),
        "qjs task: expected the redirect message to mention the backend env var; got {stderr}"
    );
}

// ── 6. sql's reserved-but-not-implemented verbs ─────────────────────────

#[test]
fn sql_reserved_verbs_stable_error() {
    for verb in ["eval", "run", "pack", "qualify", "task"] {
        let output = run(SQL_BIN, &[verb, "x.sql"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "sql {verb}: reserved verbs are documented to exit 2; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("not implemented"),
            "sql {verb}: expected the stable \"not implemented\" substring; got {stderr}"
        );
        assert!(
            stderr.contains("no decided execution target yet"),
            "sql {verb}: expected the stable open-design-question substring; got {stderr}"
        );
    }
}

// ── 7. check-many --project-root cross-engine ────────────────────────────

/// All four engines honor `--project-root` even when the process CWD is
/// somewhere unrelated. Regression lock for the lua bug this suite caught
/// on its first run (see [`run_in_dir`]'s history note): lua's
/// `cmd_check_many` used to silently drop `--project-root`, resolving
/// manifest labels against the CWD instead.
#[test]
fn check_many_project_root_honored_from_foreign_cwd() {
    for engine in engines() {
        let project = tempfile::tempdir().expect("project dir");
        let foreign_cwd = tempfile::tempdir().expect("foreign cwd");
        let source_name = format!("ok.{}", engine.ext);
        std::fs::write(project.path().join(&source_name), engine.valid_source)
            .expect("write source");
        let manifest = write_manifest(project.path(), engine.kind, &[&source_name]);

        // CWD deliberately points at an empty, unrelated directory — only
        // `--project-root` can make the manifest's relative label resolve.
        let output = run_in_dir(
            engine.bin,
            &[
                "check-many",
                "--manifest",
                &manifest.display().to_string(),
                "--project-root",
                &project.path().display().to_string(),
                "--json",
            ],
            foreign_cwd.path(),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: check-many must honor --project-root from a foreign CWD; stdout={stdout} stderr={}",
            engine.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("\"ok\": true"),
            "{}: expected an ok report; got {stdout}",
            engine.name
        );
    }
}
