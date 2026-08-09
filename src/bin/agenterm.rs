#![cfg_attr(windows, windows_subsystem = "windows")]

const INTERNAL_CLI_SUBCOMMAND: &str = "__agenterm-internal-cli";
const INTERNAL_TUI_SUBCOMMAND: &str = "__agenterm-internal-tui";
const INTERNAL_ENGINE_SUBCOMMAND: &str = "__agenterm-internal-engine";
/// `agenterm rh|lua|qjs|sql <rest>` — argv-transparent aliases for the four
/// standalone `agenterm-{rh,lua,qjs,sql}` binaries (SUB-M3,
/// `plan/design-script-engine-subcommands.md` §3). One shared internal
/// marker (`INTERNAL_ENGINE_SUBCOMMAND`) carries the engine token as its
/// first forwarded argument, rather than minting four more
/// `__agenterm-internal-{rh,lua,qjs,sql}` markers — the design doc's own
/// preference (§3: "倾向...一个内部标记 + 第一个参数是入口选择器") and the
/// shape that composes with zero changes to `run_console_worker`, which
/// already treats its `args` parameter as an opaque, ordered Vec forwarded
/// after the marker.
const ENGINE_SUBCOMMANDS: &[&str] = &[
    "rh",
    #[cfg(feature = "script-lua")]
    "lua",
    "qjs",
    #[cfg(feature = "script-sql")]
    "sql",
];

fn main() -> std::process::ExitCode {
    // Cargo's `RUSTC_WRAPPER` contract accepts exactly one executable path —
    // no extra argv (cargo invokes `$RUSTC_WRAPPER rustc <rustc's own
    // argv>`) — so retiring the standalone `agenterm-rh.exe` requires this
    // main PE itself to be able to answer as the configured wrapper.
    // Detection is purely environment-based
    // (`AGENTERM_INTERNAL_RUSTC_WRAPPER` + `RUSTC_WRAPPER` process identity,
    // see `agenterm::incremental_wrapper::is_incremental_rustc_wrapper_process`),
    // never argv-shape-based, so it composes with zero risk of colliding
    // with `cli`/`tui`/`server`/the engine subcommands below: cargo's own
    // invocation passes rustc's argv (typically starting with a path to
    // `rustc`), never any of this dispatcher's tokens. This has to run
    // before every other branch — including before argv is even collected
    // as `String` (`env::args()` panics on the first non-UTF-8 argument,
    // which would fire before the wrapper probe ever got a chance to run;
    // `script_rh_cli_main::run_main`'s doc comment established this same
    // `_os`-first, convert-after-the-check ordering for the identical
    // reason).
    let os_args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if agenterm::incremental_wrapper::is_incremental_rustc_wrapper_process(&os_args) {
        agenterm::incremental_wrapper::run_incremental_rustc_wrapper(os_args);
    }
    // `--internal-incremental-finalize` is invoked directly (not through
    // cargo/`RUSTC_WRAPPER`) by the build script, using the same stable
    // wrapper executable — see `finalize_incremental_manifest`'s callers.
    // `is_incremental_rustc_wrapper_process` above deliberately excludes
    // this shape so the two paths stay disjoint.
    if os_args
        .first()
        .is_some_and(|argument| argument == "--internal-incremental-finalize")
    {
        let arguments = os_args[1..]
            .iter()
            .cloned()
            .map(|argument| {
                argument
                    .into_string()
                    .unwrap_or_else(|invalid| panic!("invalid utf-8 sequence in argument: {invalid:?}"))
            })
            .collect::<Vec<_>>();
        return match agenterm::incremental_wrapper::finalize_incremental_manifest(&arguments) {
            Ok(code) => std::process::ExitCode::from(code),
            Err(error) => {
                eprintln!("{error:#}");
                std::process::ExitCode::from(2)
            }
        };
    }

    let mut args = os_args
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .unwrap_or_else(|invalid| panic!("invalid utf-8 sequence in argument: {invalid:?}"))
        })
        .collect::<Vec<_>>();

    // The public GUI-subsystem process attaches to the caller's console, then
    // starts the same PE with explicitly duplicated stdio handles. The child
    // sees valid handles from process startup, before Rust initializes its
    // cached stdio objects, and therefore reuses the ordinary CLI entry.
    if args.first().map(String::as_str) == Some("cli") {
        args.remove(0);
        std::process::exit(run_cli_from_gui_subsystem(args));
    }
    if args.first().map(String::as_str) == Some("tui") {
        args.remove(0);
        std::process::exit(run_tui_from_gui_subsystem(args));
    }
    if let Some(engine) = args.first().map(String::as_str)
        && ENGINE_SUBCOMMANDS.contains(&engine)
    {
        let engine = args.remove(0);
        return run_engine_from_gui_subsystem(engine, args);
    }
    if args.first().map(String::as_str) == Some(INTERNAL_CLI_SUBCOMMAND) {
        args.remove(0);
        // The worker's std slots hold explicit duplicated handles, but a
        // console handle is only writable from a process with a console
        // connection (ConDrv). Attach to the forwarder's console — same one
        // as the caller's — keeping default Ctrl+C termination; the explicit
        // handles survive the attach untouched. Pure-pipe callers have no
        // console and the attach is a harmless no-op failure.
        let _console =
            agenterm_platform::process::ScopedConsole::attach_parent_with_default_interrupts();
        std::process::exit(agenterm::run_cli_entry_with_args(args));
    }
    if args.first().map(String::as_str) == Some(INTERNAL_TUI_SUBCOMMAND) {
        args.remove(0);
        let _console =
            agenterm_platform::process::ScopedConsole::attach_parent_with_default_interrupts();
        std::process::exit(agenterm::tui::run_entry_with_args(args));
    }
    if args.first().map(String::as_str) == Some(INTERNAL_ENGINE_SUBCOMMAND) {
        args.remove(0);
        let _console =
            agenterm_platform::process::ScopedConsole::attach_parent_with_default_interrupts();
        // Unlike the cli/tui workers above (both `i32`-returning entry
        // points, forwarded via `std::process::exit`), rh's entry point
        // returns `std::process::ExitCode`, which stable Rust offers no way
        // to convert back into a raw `i32` — the only sanctioned way to
        // apply it to the real process exit status is to return it out of
        // `main` itself. That's why `main` returns `ExitCode` (all the
        // `std::process::exit` calls elsewhere in this function still
        // typecheck fine against that return type: `exit`'s return type is
        // `!`, which unifies with any expected type).
        return dispatch_engine_from_args(args);
    }

    // G3: GUI PE must still print version/help when launched from a terminal.
    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    let server_mode = match args.first().map(String::as_str) {
        Some("server") => {
            args.remove(0);
            true
        }
        // Transitional alias from the short-lived `--server` flag leaf.
        _ => {
            if let Some(index) = args.iter().position(|argument| argument == "--server") {
                args.remove(index);
                true
            } else {
                false
            }
        }
    };
    if server_mode {
        std::process::exit(agenterm::run_server_entry_with_args(args));
    }
    std::process::exit(agenterm::run_gui_entry());
}

#[cfg(windows)]
fn run_cli_from_gui_subsystem(args: Vec<String>) -> i32 {
    run_console_worker(INTERNAL_CLI_SUBCOMMAND, "CLI", args)
}

#[cfg(windows)]
fn run_tui_from_gui_subsystem(args: Vec<String>) -> i32 {
    run_console_worker(INTERNAL_TUI_SUBCOMMAND, "TUI", args)
}

/// Windows: re-exec through the same duplicated-handle console worker as
/// `cli`/`tui` (§1), with `engine` spliced in as the first forwarded
/// argument after the internal marker. `run_console_worker` returns the
/// child's raw exit code as `i32`; folded into `ExitCode` here (not
/// `std::process::exit`, see the `dispatch_engine_from_args` call site's
/// comment) via `u8::try_from` so this function's signature matches its
/// non-Windows counterpart below and `main`'s single, platform-agnostic
/// `return run_engine_from_gui_subsystem(...)` call site needs no `#[cfg]`.
#[cfg(windows)]
fn run_engine_from_gui_subsystem(engine: String, args: Vec<String>) -> std::process::ExitCode {
    let mut forwarded = Vec::with_capacity(args.len() + 1);
    forwarded.push(engine);
    forwarded.extend(args);
    let status = run_console_worker(INTERNAL_ENGINE_SUBCOMMAND, "engine", forwarded);
    std::process::ExitCode::from(u8::try_from(status).unwrap_or(1))
}

#[cfg(windows)]
fn run_console_worker(internal_subcommand: &str, worker_name: &str, args: Vec<String>) -> i32 {
    use std::os::windows::io::OwnedHandle;
    use std::process::{Command, Stdio};

    // Attach to the caller's console (when launched from a terminal) so the
    // console-backed std slots become valid, then duplicate the real
    // stdin/stdout/stderr — console, pipe, or file — and wire the duplicates
    // into the child explicitly. `Stdio::inherit` or `println!` would trust
    // std state cached before the attach; only duplicated handles are safe,
    // and they stay valid however long the child outlives the attachment.
    let _console = agenterm_platform::process::ScopedConsole::attach_parent();
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                "agenterm cli: could not resolve agenterm.exe: {error}"
            ));
            return 1;
        }
    };
    let [stdin_handle, stdout_handle, stderr_handle] =
        agenterm_platform::process::duplicated_std_handles();
    fn stdio(handle: Option<OwnedHandle>) -> Stdio {
        handle.map_or_else(Stdio::null, Stdio::from)
    }
    match Command::new(executable)
        .arg(internal_subcommand)
        .args(args)
        .stdin(stdio(stdin_handle))
        .stdout(stdio(stdout_handle))
        .stderr(stdio(stderr_handle))
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                "agenterm: could not start the {worker_name} worker: {error}"
            ));
            1
        }
    }
}

#[cfg(not(windows))]
fn run_cli_from_gui_subsystem(args: Vec<String>) -> i32 {
    agenterm::run_cli_entry_with_args(args)
}

#[cfg(not(windows))]
fn run_tui_from_gui_subsystem(args: Vec<String>) -> i32 {
    agenterm::tui::run_entry_with_args(args)
}

/// Unix: no GUI-subsystem console problem to work around (§1/§3), so this
/// calls straight into the engine entry point in-process — no re-exec.
#[cfg(not(windows))]
fn run_engine_from_gui_subsystem(engine: String, args: Vec<String>) -> std::process::ExitCode {
    dispatch_engine(&engine, args)
}

/// Shared by both the Windows internal-marker branch (post re-exec) and the
/// Unix direct-call branch above: `args[0]` is the engine token, the rest is
/// that engine's own argv with the token (and program name) already
/// stripped.
fn dispatch_engine_from_args(mut args: Vec<String>) -> std::process::ExitCode {
    if args.is_empty() {
        eprintln!("agenterm: missing engine token (expected one of: rh, lua, qjs, sql)");
        return std::process::ExitCode::from(2);
    }
    let engine = args.remove(0);
    dispatch_engine(&engine, args)
}

/// Runs one engine's CLI entry point in-process. `rest` is that engine's own
/// argv with both the program name and the engine token already stripped —
/// exactly what a standalone `agenterm-<engine>` binary's `main()` collects
/// from `env::args().skip(1)`.
fn dispatch_engine(engine: &str, rest: Vec<String>) -> std::process::ExitCode {
    match engine {
        "rh" => {
            // rh's worker modes (`--worker`, `--framed-worker`,
            // `--internal-incremental-finalize`) are argv-shape-sensitive
            // (design §7: exact single-argument matches) and handled
            // entirely inside `run_main` — forward `rest` untouched, with no
            // normalization and no flag inspection here.
            let os_args: Vec<std::ffi::OsString> = rest.into_iter().map(Into::into).collect();
            agenterm::script_rh_cli_main::run_main(os_args)
        }
        #[cfg(feature = "script-lua")]
        "lua" => {
            // `agenterm_lua::cli::run` indexes into its `args` slice
            // starting at `[1]` — it expects argv INCLUDING a program-name
            // slot at index 0, mirroring `std::env::args()`'s shape (see
            // `crates/agenterm-lua/src/cli.rs`'s doc comment on `run`). The
            // other three engines' entry points all take argv with the
            // program name already stripped, so lua is the one asymmetric
            // case here: splice in a placeholder purely to satisfy that
            // indexing convention, since the real argv[0] was discarded
            // before this function was ever called.
            let mut full = Vec::with_capacity(rest.len() + 1);
            full.push("agenterm-lua".to_owned());
            full.extend(rest);
            std::process::ExitCode::from(agenterm_lua::cli::run(&full))
        }
        "qjs" => std::process::ExitCode::from(agenterm_qjs::cli::run(&rest)),
        #[cfg(feature = "script-sql")]
        "sql" => std::process::ExitCode::from(agenterm_sql::cli::run(&rest)),
        other => {
            eprintln!("agenterm: unknown engine subcommand `{other}`");
            std::process::ExitCode::from(2)
        }
    }
}

/// Flags that must not open a window. Returns Some(exit_code) when handled.
fn offline_cli_exit(args: &[String]) -> Option<i32> {
    let alone = args.len() == 1;
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if alone => {
            let line = format!("agenterm {}", env!("CARGO_PKG_VERSION"));
            let _ = agenterm_platform::process::write_parent_console_stdout(&line);
            Some(0)
        }
        Some("--help" | "-h") if alone => {
            let line = "\
Usage: agenterm [--no-activate] [--endpoint ENDPOINT | --address HOST:PORT | --instance NAME]
       agenterm server [server options]
       agenterm cli <command> [options]
       agenterm tui
       agenterm rh|lua|qjs|sql <args>  (aliases for the standalone agenterm-{rh,lua,qjs,sql} binaries)
       agenterm --version
       agenterm --help

GUI launcher. Use `agenterm cli` for control-plane commands (list-windows, mux, mcp, …),
or `agenterm tui` for the interactive terminal interface.
On Windows, the GUI-subsystem executable attaches to the parent console and runs this command through the same PE.";
            let _ = agenterm_platform::process::write_parent_console_stdout(line);
            Some(0)
        }
        Some("--version" | "-V" | "--help" | "-h") => {
            let _ = agenterm_platform::process::write_parent_console_stderr(
                "error: --version/--help must be used alone",
            );
            Some(2)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::offline_cli_exit;

    #[test]
    fn offline_version_and_help_are_solo_flags() {
        assert_eq!(offline_cli_exit(&["--version".to_owned()]), Some(0));
        assert_eq!(offline_cli_exit(&["-V".to_owned()]), Some(0));
        assert_eq!(offline_cli_exit(&["--help".to_owned()]), Some(0));
        assert_eq!(
            offline_cli_exit(&["--version".to_owned(), "extra".to_owned()]),
            Some(2)
        );
        assert_eq!(offline_cli_exit(&["server".to_owned()]), None);
    }
}
