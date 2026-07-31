#[cfg(windows)]
use windows_sys::Win32::{UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_HIDE};

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

use std::path::{Path, PathBuf};
use std::{
    cell::RefCell,
    env,
    io::{BufRead, IsTerminal, Read, Write},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context as _, Result};

use crate::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBrokerError, ScriptBrokerRequest,
    ScriptBrokerResponse, ScriptBudgets, ScriptExitClass, ScriptInvocation, ScriptOperation,
    ScriptProfile,
};
use crate::{
    build_identity::BuildIdentity,
    commands::{
        COMMAND_CATALOG, COMMAND_CATALOG_SCHEMA_VERSION, MUX_COMMANDS, MuxStatus,
        canonical_control_command, control_command_requests_help, control_command_usage,
        has_option, last_positional, mux_command, option_value, snapshot_modal_matches,
        supported_commands, validate_control_command,
    },
    control_contract::{
        ControlRequest, ErrorCategory, OperationId, PayloadFingerprint, RequestId, RequestIntent,
    },
    event_journal::{EVENT_CATALOG, EVENT_CATALOG_SCHEMA_VERSION},
    instances::{
        InstanceRecord, RegistrationOwnerState, cleanup_instance, discover_instances,
        instance_process_is_alive, registration_owner_state,
    },
    ipc_endpoint::{
        EndpointSelectorArgs, IpcEndpoint, ResolvedIpcEndpoint, default_workspace_path_for,
        resolve_ipc_endpoint,
    },
    ipc_transport::{IPC_RESPONSE_MAX_BYTES, IpcStream, read_bounded_ipc_line},
    operations::{
        OPERATION_CATALOG, OPERATION_CATALOG_SCHEMA_VERSION, OperationClass, operation_for_args,
        validate_operation_args,
    },
    protocol::{IpcRequest, IpcResponse},
    ui_bridge,
    upgrade_identity::UpgradeIdentity,
};

use crate::operations::operation_by_id;

use crate::script_api_view::{
    filter_script_api_catalog, parse_script_api_view, render_script_api_tree,
};
use crate::script_audit::{
    AuditBudgets, AuditInvocation, AuditOutcome, AuditSourceKind, ScriptAuditSink,
    source_fingerprint,
};
use crate::script_project::{
    ResolvedScriptTask, ScriptTaskCatalog, ScriptTaskStatus, discover_task_manifest,
    load_task_catalog, resolve_task,
};
use crate::script_repl::{
    ReplCellFailure, ReplCellResult, ReplInputState, ReplSession, ReplSessionConfig,
    canonical_project_root,
};
use crate::worker_supervisor::{SupervisorError, WorkerSupervisor};

const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const IPC_AUTOSTART_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_AUTOSTART_POLL: Duration = Duration::from_millis(100);
thread_local! {
    static IPC_SELECTOR_OVERRIDE: RefCell<EndpointSelectorArgs> =
        const { RefCell::new(EndpointSelectorArgs { endpoint: None, address: None, instance: None }) };
}

struct OwnedScriptTemp {
    root: PathBuf,
}

impl OwnedScriptTemp {
    fn create(invocation_id: &str) -> std::io::Result<Self> {
        let parent = env::temp_dir().join("AgenTerm").join("script-invocations");
        std::fs::create_dir_all(&parent)?;
        Self::prune_stale(&parent);
        let root = parent.join(invocation_id);
        std::fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn prune_stale(parent: &Path) {
        let Ok(entries) = std::fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(owner_pid) = name
                .split_once('-')
                .and_then(|(prefix, _)| prefix.parse::<u32>().ok())
            else {
                continue;
            };
            if !instance_process_is_alive(owner_pid) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    fn display(&self) -> String {
        self.root.display().to_string()
    }
}

impl Drop for OwnedScriptTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(crate) fn no_activate_from_environment() -> bool {
    no_activate_from_value(env::var_os("AGENTERM_NO_ACTIVATE").as_deref())
}

pub(crate) fn no_activate_from_value(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| {
        let value = value.to_string_lossy();
        !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
    })
}

#[derive(Default)]
struct CliControlOptions {
    request_id: Option<String>,
    deadline_ms: Option<u64>,
    receipt_json: bool,
}

pub fn run_cli_entry() -> i32 {
    let mut arguments: Vec<String> = env::args().skip(1).collect();
    let mut control_options = CliControlOptions::default();
    let mut selectors = EndpointSelectorArgs::default();
    loop {
        match arguments.first().map(String::as_str) {
            Some("--address") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --address requires HOST:PORT");
                    return 2;
                }
                arguments.remove(0);
                let address = arguments.remove(0);
                if let Err(error) = parse_loopback_ipc_address(&address) {
                    eprintln!("{error:#}");
                    return 2;
                }
                if selectors.address.replace(address).is_some() {
                    eprintln!("agenterm-cli --address may be specified only once");
                    return 2;
                }
            }
            Some("--endpoint") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --endpoint requires a typed endpoint");
                    return 2;
                }
                arguments.remove(0);
                if selectors.endpoint.replace(arguments.remove(0)).is_some() {
                    eprintln!("agenterm-cli --endpoint may be specified only once");
                    return 2;
                }
            }
            Some("--instance") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --instance requires a name");
                    return 2;
                }
                arguments.remove(0);
                if selectors.instance.replace(arguments.remove(0)).is_some() {
                    eprintln!("agenterm-cli --instance may be specified only once");
                    return 2;
                }
            }
            Some("--request-id") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --request-id requires an ID");
                    return 2;
                }
                arguments.remove(0);
                let value = arguments.remove(0);
                if let Err(error) = RequestId::new(value.clone()) {
                    eprintln!("{error}");
                    return 2;
                }
                control_options.request_id = Some(value);
            }
            Some("--deadline-ms") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --deadline-ms requires milliseconds");
                    return 2;
                }
                arguments.remove(0);
                let value = arguments.remove(0);
                match value.parse::<u64>() {
                    Ok(value) if (1..=60_000).contains(&value) => {
                        control_options.deadline_ms = Some(value);
                    }
                    _ => {
                        eprintln!("agenterm-cli --deadline-ms must be from 1 to 60000");
                        return 2;
                    }
                }
            }
            Some("--receipt-json") => {
                arguments.remove(0);
                control_options.receipt_json = true;
            }
            _ => break,
        }
    }
    if let Err(error) = set_ipc_selectors(selectors) {
        eprintln!("{error:#}");
        return 2;
    }
    if arguments
        .first()
        .is_some_and(|arg| arg == "-V" || arg == "--version")
    {
        println!("agenterm-cli {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if arguments.is_empty()
        || arguments
            .first()
            .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print_help();
        return 0;
    }
    if arguments
        .first()
        .is_some_and(|argument| argument.starts_with('-'))
    {
        eprintln!(
            "unknown global option '{}'. Use --endpoint ENDPOINT, --address HOST:PORT, or \
             --instance NAME before the command.",
            arguments[0]
        );
        return 2;
    }
    run_cli(arguments, control_options)
}

pub fn run_script_entry_with_args(mut arguments: Vec<String>) -> i32 {
    if arguments.is_empty()
        || arguments
            .first()
            .is_some_and(|argument| argument == "-h" || argument == "--help")
    {
        return write_script_stdout(&format!("{}\n", script_help_text()))
            .map_or_else(|code| code, |()| 0);
    }
    arguments.insert(0, "script".to_owned());
    if let Err(error) = validate_control_command(&arguments) {
        eprintln!("{error}");
        return 2;
    }
    run_script_command_direct(&arguments)
}

fn script_help_text() -> &'static str {
    "AgenTerm Script Runtime\n\
         Usage:\n\
           agenterm-script api [MODULE] [--status STATE] [--tree|--json]\n\
           agenterm-script check [OPTIONS] FILE.rhai|-\n\
           agenterm-script check-many --manifest FILE [OPTIONS]\n\
           agenterm-script eval [OPTIONS] EXPRESSION [--] [ARGS...]\n\
           agenterm-script repl [OPTIONS] [--] [ARGS...]\n\
           agenterm-script run [OPTIONS] FILE.rhai|- [--] [ARGS...]\n\
           agenterm-script task list [--manifest PATH] [--json]\n\
           agenterm-script task show TASK [--manifest PATH] [--json]\n\
           agenterm-script task check [TASK] [--manifest PATH] [--json]\n\
           agenterm-script task run TASK [--manifest PATH] [OPTIONS] [--] [ARGS...]\n\
         REPL commands: :help :quit :reset :history :vars :functions :limits \
         :api [MODULE] :load FILE :json on|off\n\
         Options: --timeout-ms N --max-operations N --max-collection-items N \
         --max-string-bytes N --max-output-bytes N --project-root DIR \
         --manifest FILE --json"
}

fn write_script_stdout(text: &str) -> std::result::Result<(), i32> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Err(0),
        Err(error) => {
            eprintln!("script_stdout: failed to write output: {error}");
            Err(1)
        }
    }
}

fn run_script_repl(arguments: &[String]) -> i32 {
    let budgets = match parse_repl_budgets(arguments) {
        Ok(budgets) => budgets,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    if let Err(error) = validate_repl_options(arguments) {
        eprintln!("{error}");
        return 2;
    }
    let project_root = match canonical_project_root(option_value(arguments, "--project-root")) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let delimiter = arguments.iter().position(|argument| argument == "--");
    let script_arguments = delimiter
        .and_then(|position| arguments.get(position + 1..))
        .unwrap_or_default()
        .to_vec();
    let session_id = format!(
        "repl-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let invocation_temp = match OwnedScriptTemp::create(&session_id) {
        Ok(owned) => owned,
        Err(error) => {
            eprintln!("host_temp_create: REPL temporary root is unavailable: {error}");
            return 1;
        }
    };
    let broker_budgets = budgets.clone();
    let fleet = Arc::new(move |operation: &str, arguments: serde_json::Value| {
        let response = handle_script_broker(
            &ScriptBrokerRequest {
                operation: operation.to_owned(),
                arguments,
            },
            ScriptProfile::Local,
            &broker_budgets,
            Duration::from_millis(broker_budgets.wait_time_ms),
        );
        if response.ok {
            Ok(response.value.unwrap_or(serde_json::Value::Null))
        } else {
            let error = response
                .error
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "broker_unknown_failure".to_owned());
            Err(error)
        }
    });
    let mut session = match ReplSession::new(ReplSessionConfig {
        budgets,
        arguments: script_arguments,
        project_root,
        invocation_temp_root: Some(invocation_temp.root.clone()),
        fleet: Some(fleet),
    }) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };

    let interactive = std::io::stdin().is_terminal();
    let mut json = has_option(arguments, "--json");
    let fail_fast = has_option(arguments, "--fail-fast");
    if interactive {
        eprintln!(
            "AgenTerm Rhai REPL {} — :help for commands, Ctrl-D or :quit to exit",
            env!("CARGO_PKG_VERSION")
        );
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    let mut buffer = String::new();
    let mut had_failure = false;

    loop {
        if interactive {
            eprint!(
                "{}",
                if buffer.is_empty() {
                    "rhai> "
                } else {
                    "....> "
                }
            );
            let _ = std::io::stderr().flush();
        }
        line.clear();
        let read = match input.read_line(&mut line) {
            Ok(read) => read,
            Err(error) => {
                eprintln!("repl_stdin: {error}");
                return 1;
            }
        };
        if read == 0 {
            if !buffer.trim().is_empty() {
                let result = incomplete_eof_result(&mut session, &buffer);
                had_failure |= !result.ok;
                if render_repl_result(&result, json).is_err() {
                    return 1;
                }
            }
            break;
        }
        let line_without_eol = line.trim_end_matches(['\r', '\n']);
        if buffer.is_empty() && line_without_eol.trim_start().starts_with(':') {
            match run_repl_meta(
                line_without_eol.trim(),
                &mut session,
                &mut json,
                interactive,
            ) {
                ReplMetaOutcome::Continue => {}
                ReplMetaOutcome::Quit => break,
                ReplMetaOutcome::Failure => {
                    had_failure = true;
                    if fail_fast {
                        break;
                    }
                }
            }
            continue;
        }
        if buffer.is_empty() && line_without_eol.trim().is_empty() {
            continue;
        }
        buffer.push_str(&line);
        match session.inspect_input(&buffer) {
            ReplInputState::Incomplete => continue,
            ReplInputState::Invalid(failure) => {
                let result = session.evaluate_cell(&buffer);
                debug_assert_eq!(result.failure.as_ref(), Some(&failure));
                had_failure = true;
                if render_repl_result(&result, json).is_err() {
                    return 1;
                }
                buffer.clear();
                if fail_fast {
                    break;
                }
            }
            ReplInputState::Complete => {
                let result = session.evaluate_cell(&buffer);
                had_failure |= !result.ok;
                if render_repl_result(&result, json).is_err() {
                    return 1;
                }
                buffer.clear();
                if !result.ok && fail_fast {
                    break;
                }
            }
        }
    }
    i32::from(had_failure)
}

enum ReplMetaOutcome {
    Continue,
    Quit,
    Failure,
}

fn run_repl_meta(
    command: &str,
    session: &mut ReplSession,
    json: &mut bool,
    interactive: bool,
) -> ReplMetaOutcome {
    let (name, argument) = command
        .split_once(char::is_whitespace)
        .map_or((command, ""), |(name, argument)| (name, argument.trim()));
    match name {
        ":quit" | ":exit" => ReplMetaOutcome::Quit,
        ":help" => {
            let help = ":help                 show this command list\n\
                 :quit | :exit          leave the session\n\
                 :reset                clear user variables and functions\n\
                 :history              list in-memory successful cells\n\
                 :vars                 list variable names and types\n\
                 :functions            list session-defined functions\n\
                 :limits               show per-cell and session robustness limits\n\
                 :api [MODULE]         inspect the runtime API catalog\n\
                 :load FILE            evaluate a Rhai file as one cell\n\
                 :json on|off          toggle NDJSON cell results";
            render_repl_meta("help", serde_json::Value::String(help.to_owned()), *json);
            ReplMetaOutcome::Continue
        }
        ":reset" => {
            session.reset();
            if *json {
                render_repl_meta("reset", serde_json::json!({"reset": true}), true);
            } else if interactive {
                eprintln!("session reset");
            }
            ReplMetaOutcome::Continue
        }
        ":history" => {
            if *json {
                render_repl_meta("history", serde_json::json!(session.history()), true);
            } else {
                for (index, source) in session.history().iter().enumerate() {
                    println!("{}  {}", index + 1, source.trim_end().replace('\n', "\\n"));
                }
            }
            ReplMetaOutcome::Continue
        }
        ":vars" => {
            let variables = session.variables();
            if *json {
                render_repl_meta("vars", serde_json::json!(variables), true);
            } else {
                for (name, type_name) in variables {
                    println!("{name}: {type_name}");
                }
            }
            ReplMetaOutcome::Continue
        }
        ":functions" => {
            let functions = session.functions();
            if *json {
                render_repl_meta("functions", serde_json::json!(functions), true);
            } else {
                for function in functions {
                    println!("{function}");
                }
            }
            ReplMetaOutcome::Continue
        }
        ":limits" => {
            let budgets = session.budgets();
            if *json {
                render_repl_meta(
                    "limits",
                    serde_json::json!({
                        "source_bytes": budgets.source_bytes,
                        "operations": budgets.operations,
                        "call_depth": budgets.call_depth,
                        "expression_depth": budgets.expression_depth,
                        "collection_items": budgets.collection_items,
                        "string_bytes": budgets.string_bytes,
                        "output_bytes": budgets.output_bytes,
                        "wall_time_ms": budgets.wall_time_ms,
                    }),
                    true,
                );
            } else {
                println!(
                    "source_bytes={} operations={} call_depth={} expression_depth={} \
                     collection_items={} string_bytes={} output_bytes={} wall_time_ms={}",
                    budgets.source_bytes,
                    budgets.operations,
                    budgets.call_depth,
                    budgets.expression_depth,
                    budgets.collection_items,
                    budgets.string_bytes,
                    budgets.output_bytes,
                    budgets.wall_time_ms
                );
            }
            ReplMetaOutcome::Continue
        }
        ":api" => {
            let mut view_arguments = vec!["script".to_owned(), "api".to_owned()];
            if !argument.is_empty() {
                view_arguments.push(argument.to_owned());
            }
            match parse_script_api_view(&view_arguments).and_then(|view| {
                let mut catalog = crate::script_catalog::catalog();
                filter_script_api_catalog(&mut catalog, &view)
                    .map_err(|error| error.to_string())?;
                render_script_api_tree(&catalog).map_err(|error| error.to_string())
            }) {
                Ok(tree) => {
                    if *json {
                        render_repl_meta("api", serde_json::Value::String(tree), true);
                    } else {
                        println!("{tree}");
                    }
                }
                Err(error) => {
                    eprintln!("repl_api: {error}");
                    return ReplMetaOutcome::Failure;
                }
            }
            ReplMetaOutcome::Continue
        }
        ":load" => {
            if argument.is_empty() {
                eprintln!("repl_load: :load requires a Rhai file path");
                return ReplMetaOutcome::Failure;
            }
            let source = match std::fs::read_to_string(argument) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("repl_load: {argument}: {error}");
                    return ReplMetaOutcome::Failure;
                }
            };
            let result = session.evaluate_cell(&source);
            if render_repl_result(&result, *json).is_err() || !result.ok {
                ReplMetaOutcome::Failure
            } else {
                ReplMetaOutcome::Continue
            }
        }
        ":json" => match argument {
            "on" => {
                *json = true;
                ReplMetaOutcome::Continue
            }
            "off" => {
                *json = false;
                ReplMetaOutcome::Continue
            }
            _ => {
                eprintln!("repl_json: expected :json on or :json off");
                ReplMetaOutcome::Failure
            }
        },
        _ => {
            eprintln!("repl_meta_unknown: {name}; use :help");
            ReplMetaOutcome::Failure
        }
    }
}

fn incomplete_eof_result(session: &mut ReplSession, source: &str) -> ReplCellResult {
    match session.inspect_input(source) {
        ReplInputState::Complete => session.evaluate_cell(source),
        ReplInputState::Incomplete => ReplCellResult {
            sequence: 0,
            ok: false,
            stdout: String::new(),
            value: None,
            failure: Some(ReplCellFailure {
                code: "script_incomplete".to_owned(),
                category: "script".to_owned(),
                message: "end of input reached while the REPL cell was incomplete".to_owned(),
            }),
            state_committed: false,
            duration_ms: 0,
        },
        ReplInputState::Invalid(failure) => ReplCellResult {
            sequence: 0,
            ok: false,
            stdout: String::new(),
            value: None,
            failure: Some(failure),
            state_committed: false,
            duration_ms: 0,
        },
    }
}

fn render_repl_meta(command: &str, value: serde_json::Value, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "kind": "meta",
                "command": command,
                "ok": true,
                "value": value,
            })
        );
    } else if let Some(text) = value.as_str() {
        println!("{text}");
    }
}

fn render_repl_result(result: &ReplCellResult, json: bool) -> std::io::Result<()> {
    if json {
        serde_json::to_writer(std::io::stdout().lock(), result)?;
        println!();
        return Ok(());
    }
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
        if !result.stdout.ends_with('\n') {
            println!();
        }
    }
    if result.ok {
        if let Some(value) = result.value.as_ref() {
            if let Some(serialized) = value.value.as_ref() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(serialized).unwrap_or_else(|_| "null".to_owned())
                );
            } else {
                println!("<{}>", value.type_name);
            }
        }
    } else if let Some(failure) = result.failure.as_ref() {
        eprintln!("{}: {}", failure.code, failure.message);
    }
    std::io::stdout().flush()
}

fn validate_repl_options(arguments: &[String]) -> Result<(), String> {
    let mut index = 2;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            return Ok(());
        }
        match argument.as_str() {
            "--json" | "--fail-fast" => index += 1,
            "--timeout-ms"
            | "--max-operations"
            | "--max-output-bytes"
            | "--max-collection-items"
            | "--max-string-bytes"
            | "--project-root" => {
                if arguments.get(index + 1).is_none() {
                    return Err(format!("script repl {argument} requires a value"));
                }
                index += 2;
            }
            other => return Err(format!("unknown script repl option: {other}")),
        }
    }
    Ok(())
}

fn parse_repl_budgets(arguments: &[String]) -> Result<ScriptBudgets, String> {
    let mut budgets = ScriptBudgets::default();
    let hard = ScriptBudgets::hard_limits();
    if let Some(value) = option_value(arguments, "--timeout-ms") {
        budgets.wall_time_ms = parse_repl_limit(value, 1, hard.wall_time_ms, "--timeout-ms")?;
    }
    if let Some(value) = option_value(arguments, "--max-operations") {
        budgets.operations = parse_repl_limit(value, 1, hard.operations, "--max-operations")?;
    }
    if let Some(value) = option_value(arguments, "--max-output-bytes") {
        budgets.output_bytes = parse_repl_limit(value, 1, hard.output_bytes, "--max-output-bytes")?;
    }
    if let Some(value) = option_value(arguments, "--max-collection-items") {
        budgets.collection_items =
            parse_repl_limit(value, 1, hard.collection_items, "--max-collection-items")?;
    }
    if let Some(value) = option_value(arguments, "--max-string-bytes") {
        budgets.string_bytes = parse_repl_limit(value, 1, hard.string_bytes, "--max-string-bytes")?;
    }
    Ok(budgets)
}

fn parse_repl_limit<T>(value: &str, minimum: T, maximum: T, option: &str) -> Result<T, String>
where
    T: std::str::FromStr + Ord + Copy + std::fmt::Display,
{
    value
        .parse::<T>()
        .ok()
        .filter(|value| *value >= minimum && *value <= maximum)
        .ok_or_else(|| format!("script repl {option} must be from {minimum} to {maximum}"))
}

pub fn run_mux_entry() -> i32 {
    let mut arguments: Vec<String> = env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|arg| arg == "-V" || arg == "--version")
    {
        println!(
            "agenterm-mux {} (AgenTerm compatibility frontend)",
            env!("CARGO_PKG_VERSION")
        );
        return 0;
    }
    if arguments.is_empty()
        || arguments
            .first()
            .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print_mux_help();
        return 0;
    }

    let mut selectors = EndpointSelectorArgs::default();
    let mut session = None;
    loop {
        match arguments.first().map(String::as_str) {
            Some("--address") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --address requires HOST:PORT");
                    return 2;
                }
                arguments.remove(0);
                let candidate = arguments.remove(0);
                if let Err(error) = parse_loopback_ipc_address(&candidate) {
                    eprintln!("{error:#}");
                    return 2;
                }
                if selectors.address.replace(candidate).is_some() {
                    eprintln!("agenterm-mux --address may be specified only once");
                    return 2;
                }
            }
            Some("--endpoint") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --endpoint requires a typed endpoint");
                    return 2;
                }
                arguments.remove(0);
                if selectors.endpoint.replace(arguments.remove(0)).is_some() {
                    eprintln!("agenterm-mux --endpoint may be specified only once");
                    return 2;
                }
            }
            Some("--instance") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --instance requires a name");
                    return 2;
                }
                arguments.remove(0);
                if selectors.instance.replace(arguments.remove(0)).is_some() {
                    eprintln!("agenterm-mux --instance may be specified only once");
                    return 2;
                }
            }
            Some("--session") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --session requires a session name");
                    return 2;
                }
                arguments.remove(0);
                session = Some(arguments.remove(0));
            }
            Some("-L" | "-S") => {
                eprintln!(
                    "agenterm-mux does not support tmux socket selection; use --address HOST:PORT"
                );
                return 2;
            }
            _ => break,
        }
    }
    let Some(command) = arguments.first().cloned() else {
        eprintln!("agenterm-mux requires a command");
        return 2;
    };

    if command == "compatibility" {
        print_mux_compatibility(arguments.iter().any(|argument| argument == "--json"));
        return 0;
    }
    if command == "agenterm" {
        arguments.remove(0);
        if arguments.is_empty() {
            eprintln!("agenterm-mux agenterm requires a native AgenTerm command");
            return 2;
        }
    } else {
        let Some(specification) = mux_command(&command) else {
            eprintln!(
                "{command} is not in the agenterm-mux compatibility surface; \
                 use `agenterm-mux agenterm {command} ...` for native AgenTerm extensions"
            );
            return 2;
        };
        if let MuxStatus::Unsupported(reason) = specification.status {
            eprintln!("{command} is unsupported: {reason}");
            return 1;
        }
        if matches!(command.as_str(), "list-commands" | "lscm") {
            print_mux_commands();
            return 0;
        }
    }

    if let Some(session) = session
        && matches!(
            arguments.first().map(String::as_str),
            Some("attach" | "attach-session" | "has" | "has-session" | "kill-session")
        )
        && !has_option(&arguments, "-t")
    {
        arguments.extend(["-t".to_owned(), session]);
    }
    if let Err(error) = set_ipc_selectors(selectors) {
        eprintln!("{error:#}");
        return 2;
    }
    run_cli(arguments, CliControlOptions::default())
}
pub(crate) fn resolved_ipc_endpoint() -> Result<ResolvedIpcEndpoint> {
    let selectors = IPC_SELECTOR_OVERRIDE.with(|value| value.borrow().clone());
    resolve_ipc_endpoint(&selectors).map_err(anyhow::Error::new)
}

pub(crate) fn ipc_endpoint() -> Result<IpcEndpoint> {
    let resolved = resolved_ipc_endpoint()?;
    apply_resolved_environment(&resolved);
    Ok(resolved.endpoint)
}

pub(crate) fn ipc_address() -> String {
    ipc_endpoint()
        .map(|endpoint| {
            endpoint
                .legacy_address()
                .unwrap_or_else(|| endpoint.to_string())
        })
        .unwrap_or_else(|_| "unresolved".to_owned())
}

pub(crate) fn parse_loopback_ipc_address(address: &str) -> Result<std::net::SocketAddr> {
    if address.contains('\0') {
        anyhow::bail!("invalid AgenTerm IPC address: NUL is not allowed");
    }
    let socket: std::net::SocketAddr = address
        .parse()
        .with_context(|| format!("invalid AgenTerm IPC address: {address}"))?;
    if !socket.ip().is_loopback() {
        anyhow::bail!(
            "AgenTerm IPC address must use a loopback IP (127.0.0.0/8 or ::1): {address}"
        );
    }
    Ok(socket)
}

pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn current_upgrade_identity() -> UpgradeIdentity {
    UpgradeIdentity::current(1)
}

pub(crate) fn control_request_identity(args: &[String]) -> Result<(OperationId, RequestIntent)> {
    if let Some(operation) = operation_for_args(args).map_err(anyhow::Error::msg)? {
        let intent = if operation.class == OperationClass::Observe {
            RequestIntent::Query
        } else {
            RequestIntent::Mutation
        };
        return Ok((
            OperationId::new(operation.id).map_err(anyhow::Error::msg)?,
            intent,
        ));
    }
    let command = args.first().map(String::as_str).unwrap_or("unknown");
    let intent = if matches!(
        canonical_control_command(command),
        "active-window"
            | "capture-pane"
            | "display-message"
            | "dump-cells"
            | "get-settings"
            | "has-session"
            | "inspect"
            | "list-panes"
            | "list-sessions"
            | "list-tab-tree"
            | "list-windows"
            | "read-events"
            | "show-composer"
            | "show-options"
            | "show-tab-note"
            | "show-tab-parent"
            | "workspace-info"
    ) {
        RequestIntent::Query
    } else {
        RequestIntent::Mutation
    };
    let identity = format!(
        "command.{}",
        canonical_control_command(command).replace('-', ".")
    );
    Ok((
        OperationId::new(identity).map_err(anyhow::Error::msg)?,
        intent,
    ))
}

pub(crate) fn control_payload_fingerprint(args: &[String]) -> Result<PayloadFingerprint> {
    Ok(PayloadFingerprint::from_bytes(&serde_json::to_vec(args)?))
}

pub(crate) fn error_category_from_wire(category: &str) -> ErrorCategory {
    match category {
        "validation" => ErrorCategory::Validation,
        "conflict" => ErrorCategory::Conflict,
        "not_found" => ErrorCategory::NotFound,
        "precondition" => ErrorCategory::Precondition,
        "availability" => ErrorCategory::Availability,
        "timeout" => ErrorCategory::Timeout,
        "policy" => ErrorCategory::Policy,
        "unsupported" => ErrorCategory::Unsupported,
        _ => ErrorCategory::Internal,
    }
}

fn build_control_request(
    args: &[String],
    request_id: Option<String>,
    deadline_ms: u64,
) -> Result<ControlRequest> {
    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request_id = request_id.unwrap_or_else(|| {
        format!(
            "client:{}:{}:{}",
            std::process::id(),
            unix_time_ms(),
            NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    });
    let (operation_id, intent) = control_request_identity(args)?;
    Ok(ControlRequest::new(
        RequestId::new(request_id).map_err(anyhow::Error::msg)?,
        operation_id,
        control_payload_fingerprint(args)?,
        intent,
        Some(unix_time_ms().saturating_add(deadline_ms)),
    ))
}

pub(crate) fn set_ipc_selectors(selectors: EndpointSelectorArgs) -> Result<()> {
    resolve_ipc_endpoint(&selectors).map_err(anyhow::Error::new)?;
    IPC_SELECTOR_OVERRIDE.with(|value| *value.borrow_mut() = selectors);
    configure_scoped_paths()
}

pub(crate) fn configure_scoped_paths() -> Result<()> {
    let resolved = resolved_ipc_endpoint()?;
    apply_resolved_environment(&resolved);
    Ok(())
}

fn apply_resolved_environment(resolved: &ResolvedIpcEndpoint) {
    const DERIVED_SETTINGS_MARKER: &str = "AGENTERM_SETTINGS_PATH_DERIVED";
    if env::var_os("AGENTERM_WORKSPACE_PATH").is_none() {
        unsafe {
            env::set_var(
                "AGENTERM_WORKSPACE_PATH",
                default_workspace_path_for(resolved),
            );
        }
    }
    let settings_path_is_derived =
        env::var_os(DERIVED_SETTINGS_MARKER).as_deref() == Some(std::ffi::OsStr::new("1"));
    if env::var_os("AGENTERM_SETTINGS_PATH").is_none() || settings_path_is_derived {
        unsafe {
            if resolved.logical_instance == crate::ipc_endpoint::LogicalInstance::Main {
                if settings_path_is_derived {
                    env::remove_var("AGENTERM_SETTINGS_PATH");
                    env::remove_var(DERIVED_SETTINGS_MARKER);
                }
            } else {
                env::set_var(
                    "AGENTERM_SETTINGS_PATH",
                    crate::settings::default_settings_path_for(resolved),
                );
                env::set_var(DERIVED_SETTINGS_MARKER, "1");
            }
        }
    }
    // Do not manufacture an environment selector for implicit compatibility
    // main. A later resolver call would reinterpret our own
    // AGENTERM_INSTANCE=main as an explicit native opt-in.
    if resolved.explicit {
        unsafe {
            env::set_var("AGENTERM_INSTANCE", resolved.logical_instance.to_string());
        }
    }
}

pub(crate) fn send_ipc_request(args: Vec<String>) -> Result<IpcResponse> {
    let control = build_control_request(&args, None, IPC_TIMEOUT.as_millis() as u64)?;
    send_ipc_request_to_endpoint_with_timeout(&ipc_endpoint()?, args, Some(control), IPC_TIMEOUT)
}

fn send_control_request(args: Vec<String>, control: ControlRequest) -> Result<IpcResponse> {
    send_ipc_request_to_endpoint_with_timeout(&ipc_endpoint()?, args, Some(control), IPC_TIMEOUT)
}

fn await_ui_client_relay(response: IpcResponse, timeout: Duration) -> Result<IpcResponse> {
    await_ui_client_relay_to(response, &ipc_address(), timeout)
}

fn await_ui_client_relay_to(
    response: IpcResponse,
    address: &str,
    timeout: Duration,
) -> Result<IpcResponse> {
    if !response.ok {
        return Ok(response);
    }
    let Ok(queued) = serde_json::from_str::<serde_json::Value>(&response.output) else {
        return Ok(response);
    };
    if queued["relay"].as_str() != Some("ui_client") || queued["queued"].as_bool() != Some(true) {
        return Ok(response);
    }
    let command_id = queued["command_id"]
        .as_str()
        .context("UI client relay omitted command_id")?
        .to_owned();
    let relay_receipt = response.receipt;
    let deadline = Instant::now() + timeout;
    // Give the 100 ms native UI timer one uncontested turn before opening a
    // result connection. This avoids phase-locking the waiter with the exact
    // UI owner on transports that schedule short-lived connections unfairly.
    thread::sleep(Duration::from_millis(125).min(timeout));
    let mut last_state = "queued".to_owned();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("UI client command {command_id} timed out in relay state {last_state}");
        }
        let result = send_ipc_request_to_timeout(
            address,
            vec![
                "ui-client-command".to_owned(),
                "result".to_owned(),
                "--command-id".to_owned(),
                command_id.clone(),
            ],
            remaining,
        )?;
        if !result.ok {
            anyhow::bail!(
                "{} [{}:{}]",
                result.error,
                result.error_category,
                result.error_code
            );
        }
        let value: serde_json::Value =
            serde_json::from_str(&result.output).context("invalid UI client relay result")?;
        last_state = value["state"].as_str().unwrap_or("invalid").to_owned();
        if value["state"].as_str() == Some("complete") {
            let mut completed: IpcResponse = serde_json::from_value(value["response"].clone())
                .context("invalid completed UI client response")?;
            if completed.receipt.is_none()
                && let Some(mut receipt) = relay_receipt.clone()
            {
                if let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&completed.output)
                    && let Ok(position) = serde_json::from_value(snapshot["event_position"].clone())
                {
                    receipt.after_position = Some(position);
                }
                receipt.result = completed
                    .ok
                    .then(|| serde_json::json!({ "output": completed.output }));
                completed.receipt = Some(receipt);
            }
            return Ok(completed);
        }
        // Polling faster than the native UI timer cannot reduce completion
        // latency and creates enough short-lived Windows IPC connections to
        // contend with the UI owner's apply/complete requests.
        thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

pub(crate) fn send_ipc_request_to_timeout(
    address: &str,
    args: Vec<String>,
    timeout: Duration,
) -> Result<IpcResponse> {
    let control = build_control_request(&args, None, timeout.as_millis() as u64)?;
    send_ipc_request_to_with_timeout(address, args, Some(control), timeout)
}

fn send_ipc_request_to_with_timeout(
    address: &str,
    args: Vec<String>,
    control: Option<ControlRequest>,
    timeout: Duration,
) -> Result<IpcResponse> {
    let endpoint = address
        .parse::<IpcEndpoint>()
        .or_else(|_| IpcEndpoint::from_legacy_address(address))
        .map_err(anyhow::Error::new)?;
    endpoint.validate_local().map_err(anyhow::Error::new)?;
    send_ipc_request_to_endpoint_with_timeout(&endpoint, args, control, timeout)
}

fn send_ipc_request_to_endpoint_with_timeout(
    endpoint: &IpcEndpoint,
    args: Vec<String>,
    control: Option<ControlRequest>,
    timeout: Duration,
) -> Result<IpcResponse> {
    let deadline = Instant::now() + timeout;
    let remaining = || {
        let duration = deadline.saturating_duration_since(Instant::now());
        if duration.is_zero() {
            anyhow::bail!("AgenTerm IPC request timed out");
        }
        Ok(duration)
    };
    let connect_timeout = timeout
        .min(Duration::from_millis(100))
        .max(Duration::from_millis(1));
    let mut connection = IpcStream::connect(endpoint, connect_timeout)
        .map_err(anyhow::Error::new)
        .context("AgenTerm server is not running")?;
    connection
        .set_io_timeout(remaining()?)
        .map_err(anyhow::Error::new)?;
    connection.write_all(serde_json::to_string(&IpcRequest { args, control })?.as_bytes())?;
    connection.write_all(b"\n")?;
    connection.flush()?;
    let mut reader = std::io::BufReader::new(connection);
    let response =
        read_bounded_ipc_line(&mut reader, IPC_RESPONSE_MAX_BYTES, "AgenTerm IPC response")?;
    serde_json::from_str(&response).context("invalid response from AgenTerm server")
}

fn probe_registered_instance(
    record: &InstanceRecord,
    endpoint: &IpcEndpoint,
) -> (
    &'static str,
    String,
    Option<serde_json::Value>,
    Option<serde_json::Value>,
) {
    let protocol_response = match send_ipc_request_to_timeout(
        &endpoint.to_string(),
        vec!["protocol-info".to_owned()],
        IPC_DISCOVERY_TIMEOUT,
    ) {
        Ok(response) if response.ok => response,
        Ok(response) => {
            return (
                "incompatible",
                format!("protocol_probe_rejected:{}", response.error),
                None,
                None,
            );
        }
        Err(error) => {
            return (
                "unreachable",
                format!("protocol_probe_failed:{error}"),
                None,
                None,
            );
        }
    };
    let protocol = match serde_json::from_str::<serde_json::Value>(&protocol_response.output) {
        Ok(protocol) => protocol,
        Err(error) => {
            return (
                "incompatible",
                format!("protocol_facts_unparseable:{error}"),
                None,
                None,
            );
        }
    };
    let expected_scope = record.server_scope_id.as_ref().map(|scope| scope.as_str());
    let expected_endpoint = endpoint.to_string();
    if protocol["pid"].as_u64() != Some(u64::from(record.pid))
        || protocol["endpoint"].as_str() != Some(expected_endpoint.as_str())
        || protocol["server_scope_id"].as_str() != expected_scope
    {
        return (
            "incompatible",
            "protocol_identity_mismatch".to_owned(),
            Some(protocol),
            None,
        );
    }

    let snapshot_response = match send_ipc_request_to_timeout(
        &endpoint.to_string(),
        vec!["ui-snapshot".to_owned()],
        IPC_DISCOVERY_TIMEOUT,
    ) {
        Ok(response) if response.ok => response,
        Ok(response) => {
            return (
                "incompatible",
                format!("snapshot_probe_rejected:{}", response.error),
                Some(protocol),
                None,
            );
        }
        Err(error) => {
            return (
                "unreachable",
                format!("snapshot_probe_failed:{error}"),
                Some(protocol),
                None,
            );
        }
    };
    let snapshot = match serde_json::from_str::<serde_json::Value>(&snapshot_response.output) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                "incompatible",
                format!("snapshot_facts_unparseable:{error}"),
                Some(protocol),
                None,
            );
        }
    };
    if snapshot["event_position"]["epoch"].as_str() != record.server_epoch.as_deref() {
        return (
            "incompatible",
            "server_epoch_mismatch".to_owned(),
            Some(protocol),
            Some(snapshot),
        );
    }
    (
        "live",
        "registration_process_protocol_and_epoch_matched".to_owned(),
        Some(protocol),
        Some(snapshot),
    )
}

fn run_list_instances(arguments: &[String]) -> i32 {
    let json = arguments.iter().any(|argument| argument == "--json");
    let prune = arguments.iter().any(|argument| argument == "--prune");
    let instances = match discover_instances() {
        Ok(instances) => instances,
        Err(error) => {
            eprintln!("failed to discover AgenTerm instances: {error:#}");
            return 1;
        }
    };
    let mut views = Vec::new();
    let mut cleanup_failed = false;
    let staged_identity = current_upgrade_identity();
    for instance in instances {
        let endpoint = instance
            .record
            .resolved_endpoint()
            .expect("discovery filters records without a usable endpoint");
        let owner = registration_owner_state(&instance.record);
        let (classification, status_reason, protocol, snapshot) = match &owner {
            RegistrationOwnerState::Dead { .. } | RegistrationOwnerState::PidReused { .. } => (
                if instance.record.test_fixture {
                    "stale-test-fixture"
                } else {
                    "stale"
                },
                owner.reason().to_owned(),
                None,
                None,
            ),
            RegistrationOwnerState::OwnerUnknown { .. } => {
                ("owner-unknown", owner.reason().to_owned(), None, None)
            }
            RegistrationOwnerState::ConfirmedLive { .. }
                if instance.record.lease_nonce.is_none()
                    || instance.record.server_epoch.is_none() =>
            {
                (
                    "owner-unknown",
                    "registration_missing_lease_nonce_or_server_epoch".to_owned(),
                    None,
                    None,
                )
            }
            RegistrationOwnerState::ConfirmedLive { .. } => {
                probe_registered_instance(&instance.record, &endpoint)
            }
        };
        let status = if classification == "live" {
            "running"
        } else {
            classification
        };
        let cleanup_receipt = prune.then(|| cleanup_instance(&instance));
        cleanup_failed |= cleanup_receipt
            .as_ref()
            .is_some_and(|receipt| receipt.failed());
        let tab_count = snapshot
            .as_ref()
            .and_then(|value| value["tabs"].as_array())
            .map_or(0, Vec::len);
        let active_tab = snapshot
            .as_ref()
            .and_then(|value| value["focus"]["window_id"].as_str())
            .unwrap_or("-");
        let window_title = snapshot
            .as_ref()
            .and_then(|value| value["window"]["title"].as_str())
            .unwrap_or("");
        let window_visible = snapshot
            .as_ref()
            .and_then(|value| value["window"]["visible"].as_bool());
        let window_detached = snapshot
            .as_ref()
            .and_then(|value| value["window"]["detached"].as_bool());
        let window_state = snapshot
            .as_ref()
            .and_then(|value| value["window"]["state"].as_str());
        let modal_kind = snapshot
            .as_ref()
            .and_then(|value| value["modal"]["kind"].as_str());
        let event_epoch = snapshot
            .as_ref()
            .and_then(|value| value["event_position"]["epoch"].as_str());
        let event_sequence = snapshot
            .as_ref()
            .and_then(|value| value["event_position"]["sequence"].as_u64());
        let running_identity = instance.record.upgrade_identity.clone().unwrap_or_default();
        let upgrade = running_identity.compare_staged(&staged_identity);
        let upgrade_explanation = upgrade.explanation();
        let legacy_client_compatibility = instance.record.legacy_client_compatibility();
        let username = env::var("USERNAME")
            .or_else(|_| env::var("USER"))
            .unwrap_or_else(|_| "user".to_owned());
        let logical_instance = instance.record.resolved_logical_instance();
        views.push(serde_json::json!({
            "schema_version": 2,
            "pid": instance.record.pid,
            "address": instance.record.address,
            "endpoint": endpoint.to_string(),
            "transport": endpoint.transport_name(),
            "instance": logical_instance.to_string(),
            "instance_label": logical_instance.display_label(&username),
            "server_scope_id": instance.record.server_scope_id.as_ref().map(|scope| scope.as_str()),
            "registration_schema_version": instance.record.schema_version,
            "process_start_identity": instance.record.process_start_identity.clone(),
            "lease_nonce": instance.record.lease_nonce.clone(),
            "server_epoch": instance.record.server_epoch.clone(),
            "test_fixture": instance.record.test_fixture,
            "legacy_client_compatibility": legacy_client_compatibility,
            "version": instance.record.version,
            "status": status,
            "classification": classification,
            "status_reason": status_reason,
            "owner_state": owner.name(),
            "observed_process_start_identity": owner.observed_start_identity(),
            "cleanup_eligible": owner.is_stale(),
            "cleanup_receipt": cleanup_receipt,
            "observed_protocol_pid": protocol.as_ref().and_then(|value| value["pid"].as_u64()),
            "observed_protocol_endpoint": protocol.as_ref().and_then(|value| value["endpoint"].as_str()),
            "observed_protocol_server_scope_id": protocol.as_ref().and_then(|value| value["server_scope_id"].as_str()),
            "tab_count": tab_count,
            "active_tab": active_tab,
            "session": instance.record.session,
            "workspace_path": instance.record.workspace_path,
            "started_at_unix_ms": instance.record.started_at_unix_ms,
            "window_title": window_title,
            "window_visible": window_visible,
            "window_detached": window_detached,
            "window_state": window_state,
            "modal_kind": modal_kind,
            "event_epoch": event_epoch,
            "event_sequence": event_sequence,
            "running_identity": running_identity,
            "staged_identity": staged_identity,
            "upgrade": upgrade,
            "upgrade_explanation": upgrade_explanation,
        }));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&views).unwrap_or_else(|_| "[]".to_owned())
        );
    } else if views.is_empty() {
        println!("No registered AgenTerm instances.");
    } else {
        println!(
            "PID\tINSTANCE\tTRANSPORT\tENDPOINT\tV1-CLIENT\tVERSION\tSTATUS\tUPGRADE\tWINDOW\tTABS\tACTIVE\tSESSION\tWORKSPACE"
        );
        for view in views {
            let window = match (
                view["window_visible"].as_bool(),
                view["window_detached"].as_bool(),
                view["window_state"].as_str(),
            ) {
                (Some(false), Some(true), _) => "detached",
                (Some(true), _, Some(state)) => state,
                (Some(false), _, _) => "hidden",
                _ => "-",
            };
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                view["pid"].as_u64().unwrap_or_default(),
                view["instance_label"].as_str().unwrap_or("main"),
                view["transport"].as_str().unwrap_or_default(),
                view["endpoint"].as_str().unwrap_or_default(),
                view["legacy_client_compatibility"]
                    .as_str()
                    .unwrap_or("unknown"),
                view["version"].as_str().unwrap_or_default(),
                view["status"].as_str().unwrap_or_default(),
                view["upgrade"]["status"].as_str().unwrap_or("unknown"),
                window,
                view["tab_count"].as_u64().unwrap_or_default(),
                view["active_tab"].as_str().unwrap_or("-"),
                view["session"].as_str().unwrap_or_default(),
                view["workspace_path"].as_str().unwrap_or_default(),
            );
        }
    }
    if cleanup_failed { 1 } else { 0 }
}

#[cfg_attr(not(windows), allow(unused_variables, unused_mut))]
fn run_cli(arguments: Vec<String>, control_options: CliControlOptions) -> i32 {
    let mut arguments = arguments;
    if control_command_requests_help(&arguments) {
        let command = arguments.first().map(String::as_str).unwrap_or_default();
        if let Some(usage) = control_command_usage(command) {
            println!("Usage: {usage}");
            return 0;
        }
    }
    if let Err(error) = validate_control_command(&arguments) {
        eprintln!("{error}");
        return 2;
    }
    if arguments
        .first()
        .is_some_and(|command| command == "set-composer")
    {
        let content = if let Some(position) = arguments
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .position(|argument| argument == "--stdin")
        {
            arguments.remove(position);
            let mut content = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut content) {
                eprintln!("failed to read composer content from stdin: {error}");
                return 1;
            }
            Some(content)
        } else if let Some(position) = arguments
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .position(|argument| argument == "--file")
        {
            if position + 1 >= arguments.len() {
                eprintln!("set-composer --file requires a path");
                return 1;
            }
            let path = arguments.remove(position + 1);
            arguments.remove(position);
            match std::fs::read_to_string(&path) {
                Ok(content) => Some(content),
                Err(error) => {
                    eprintln!("failed to read composer file {path}: {error}");
                    return 1;
                }
            }
        } else {
            None
        };
        if let Some(content) = content {
            arguments.push("--".to_owned());
            arguments.push(content);
        }
    }
    if let Some(command) = arguments.first_mut() {
        let canonical = canonical_control_command(command);
        if canonical != command {
            *command = canonical.to_owned();
        }
    }
    if let Err(error) = validate_operation_args(&arguments) {
        eprintln!("{error}");
        return 2;
    }
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    if matches!(command, "list-commands" | "lscm") {
        print!("{}", supported_commands());
        return 0;
    }
    if command == "protocol-info" && !has_option(&arguments, "--running") {
        println!("{}", protocol_info_json("client_binary"));
        return 0;
    }
    if matches!(command, "list-instances" | "server-list") {
        return run_list_instances(&arguments);
    }
    if command == "script" {
        return run_script_command_hosted(&arguments);
    }
    if command == "control-center" {
        return crate::control_center::run_control_center_cli(&arguments, &ipc_address());
    }
    if command == "wait-ui" {
        return run_wait_ui(&arguments);
    }
    if matches!(command, "wait-pane" | "expect-pane") {
        return run_wait_pane(&arguments);
    }
    if command == "wait-events" {
        return run_wait_events(&arguments);
    }
    let may_start_server = command_may_start_server(command);
    let relay_timeout = Duration::from_millis(
        control_options
            .deadline_ms
            .unwrap_or(5_000)
            .clamp(1, 60_000),
    );
    let control = match build_control_request(
        &arguments,
        control_options.request_id,
        control_options.deadline_ms.unwrap_or(5_000),
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("{error:#}");
            return 2;
        }
    };
    let mut response = send_control_request(arguments.clone(), control.clone());
    if response.is_err() && may_start_server {
        #[cfg(windows)]
        if let Err(error) = start_server_process() {
            eprintln!("{error:#}");
        }
    }
    if response.is_err() && may_start_server {
        let deadline = Instant::now() + IPC_AUTOSTART_TIMEOUT;
        while Instant::now() < deadline {
            thread::sleep(
                IPC_AUTOSTART_POLL.min(deadline.saturating_duration_since(Instant::now())),
            );
            response = send_control_request(arguments.clone(), control.clone());
            if response.is_ok() {
                break;
            }
        }
    }
    response = response.and_then(|response| await_ui_client_relay(response, relay_timeout));
    match response {
        Ok(response) if response.ok => {
            if control_options.receipt_json {
                match response.receipt {
                    Some(receipt) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&receipt).unwrap_or_default()
                        );
                        return 0;
                    }
                    None => {
                        eprintln!("AgenTerm server did not return a control receipt");
                        return 1;
                    }
                }
            }
            if !response.output.is_empty() {
                print!("{}", response.output);
                if !response.output.ends_with('\n') {
                    println!();
                }
            }
            0
        }
        Ok(response) => {
            if control_options.receipt_json {
                if let Some(receipt) = response.receipt {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&receipt).unwrap_or_default()
                    );
                } else {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "error": response.error,
                            "error_code": response.error_code,
                            "error_category": response.error_category,
                            "retryable": response.retryable,
                        })
                    );
                }
            } else {
                eprintln!("{}", response.error);
            }
            1
        }
        Err(error) => {
            eprintln!("{error:#}");
            1
        }
    }
}

fn command_may_start_server(command: &str) -> bool {
    matches!(
        command,
        "new-session"
            | "new"
            | "new-agent"
            | "new-window"
            | "neww"
            | "attach-session"
            | "attach"
            | "start-server"
    )
}

#[cfg(not(windows))]
fn run_script_command_hosted(_arguments: &[String]) -> i32 {
    eprintln!(
        "agenterm-cli script hosting is not yet available on this platform; \
         invoke agenterm-script directly"
    );
    2
}

#[cfg(windows)]
fn run_script_command_hosted(arguments: &[String]) -> i32 {
    if !arguments
        .get(1)
        .is_some_and(|value| value == "repl" || value == "check-many")
    {
        if arguments.get(1).is_some_and(|value| value == "task") {
            return run_script_task_command(arguments);
        }
        return run_script_command_with_context(arguments, None);
    }
    let worker = match script_worker_executable() {
        Ok(worker) => worker,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    match std::process::Command::new(worker)
        .args(arguments.iter().skip(1))
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("host_worker_spawn: could not start agenterm-script: {error}");
            1
        }
    }
}

fn run_script_command_direct(arguments: &[String]) -> i32 {
    if arguments.get(1).is_some_and(|value| value == "task") {
        return run_script_task_command(arguments);
    }
    if arguments.get(1).is_some_and(|value| value == "repl") {
        return run_script_repl(arguments);
    }
    run_script_command_with_context(arguments, None)
}

#[derive(Clone, Debug)]
struct ScriptExecutionContext {
    project_root: PathBuf,
    working_directory: PathBuf,
}

fn script_worker_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|error| {
        format!("host_worker_missing: could not locate current executable: {error}")
    })?;
    let parent = current.parent().ok_or_else(|| {
        "host_worker_missing: agenterm-script is not installed next to the invoking executable"
            .to_owned()
    })?;
    #[cfg(windows)]
    let candidates = ["agenterm-script.exe"];
    #[cfg(not(windows))]
    let candidates = ["agenterm-script", "agenterm-script.exe"];
    for name in candidates {
        let path = parent.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "host_worker_missing: agenterm-script is not installed next to {}; \
         scripting is an optional component",
        current.display()
    ))
}

fn run_script_command_with_context(
    arguments: &[String],
    context: Option<ScriptExecutionContext>,
) -> i32 {
    let Some(operation_name) = arguments.get(1).map(String::as_str) else {
        eprintln!("script requires api, check, eval, run, or task");
        return 2;
    };
    let operation = match operation_name {
        "api" => ScriptOperation::Api,
        "check" => ScriptOperation::Check,
        "eval" => ScriptOperation::Eval,
        "run" => ScriptOperation::Run,
        other => {
            eprintln!("unknown script operation: {other}");
            return 2;
        }
    };
    let api_view = if operation == ScriptOperation::Api {
        match parse_script_api_view(arguments) {
            Ok(view) => Some(view),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    } else {
        if has_option(arguments, "--status") {
            eprintln!("script --status is available only for script api");
            return 2;
        }
        None
    };
    let requested_profile = option_value(arguments, "--profile").unwrap_or("local");
    let profile = match requested_profile {
        "pure" | "observe" | "local" => ScriptProfile::Local,
        other => {
            eprintln!("unknown script profile: {other}");
            return 2;
        }
    };
    let mut budgets = ScriptBudgets::default();
    let hard_limits = ScriptBudgets::hard_limits();
    if let Some(value) = option_value(arguments, "--timeout-ms") {
        match value.parse::<u64>() {
            Ok(value) if (1..=hard_limits.wall_time_ms).contains(&value) => {
                budgets.wall_time_ms = value;
            }
            _ => {
                eprintln!("script --timeout-ms must be from 1 to 3600000");
                return 2;
            }
        }
    }
    if let Some(value) = option_value(arguments, "--max-operations") {
        match value.parse::<u64>() {
            Ok(value) if (1..=hard_limits.operations).contains(&value) => {
                budgets.operations = value;
            }
            _ => {
                eprintln!("script --max-operations must be from 1 to 100000000");
                return 2;
            }
        }
    }
    if let Some(value) = option_value(arguments, "--max-output-bytes") {
        match value.parse::<usize>() {
            Ok(value) if (1..=hard_limits.output_bytes).contains(&value) => {
                budgets.output_bytes = value;
            }
            _ => {
                eprintln!("script --max-output-bytes must be from 1 to 1048576");
                return 2;
            }
        }
    }
    if let Some(value) = option_value(arguments, "--max-collection-items") {
        match value.parse::<usize>() {
            Ok(value) if (1..=hard_limits.collection_items).contains(&value) => {
                budgets.collection_items = value;
            }
            _ => {
                eprintln!("script --max-collection-items must be from 1 to 100000");
                return 2;
            }
        }
    }
    if let Some(value) = option_value(arguments, "--max-string-bytes") {
        match value.parse::<usize>() {
            Ok(value) if (1..=hard_limits.string_bytes).contains(&value) => {
                budgets.string_bytes = value;
            }
            _ => {
                eprintln!("script --max-string-bytes must be from 1 to 8388608");
                return 2;
            }
        }
    }

    let operand = script_operand(arguments);
    let (source_label, source) = match operation {
        ScriptOperation::Api => ("api".to_owned(), String::new()),
        ScriptOperation::Eval => {
            let Some(expression) = operand else {
                eprintln!("script eval requires an expression");
                return 2;
            };
            ("eval".to_owned(), expression.to_owned())
        }
        ScriptOperation::Check | ScriptOperation::Run => {
            let Some(path) = operand else {
                eprintln!("script {operation_name} requires a file path or -");
                return 2;
            };
            if path == "-" {
                match read_script_source(std::io::stdin().lock(), budgets.source_bytes) {
                    Ok(source) => ("stdin".to_owned(), source),
                    Err((code, error)) => {
                        eprintln!("failed to read script from stdin: {error}");
                        return code;
                    }
                }
            } else {
                let canonical = match std::fs::canonicalize(path) {
                    Ok(path) => path,
                    Err(error) => {
                        eprintln!("failed to resolve script {path}: {error}");
                        return 1;
                    }
                };
                if std::fs::metadata(&canonical)
                    .is_ok_and(|metadata| metadata.len() > budgets.source_bytes as u64)
                {
                    eprintln!(
                        "script source exceeds the {} byte limit",
                        budgets.source_bytes
                    );
                    return 3;
                }
                let file = match std::fs::File::open(&canonical) {
                    Ok(file) => file,
                    Err(error) => {
                        eprintln!("failed to read script {path}: {error}");
                        return 1;
                    }
                };
                match read_script_source(file, budgets.source_bytes) {
                    Ok(source) => (canonical.display().to_string(), source),
                    Err((code, error)) => {
                        eprintln!("failed to read script {path}: {error}");
                        return code;
                    }
                }
            }
        }
    };
    if source.len() > budgets.source_bytes {
        eprintln!(
            "script source exceeds the {} byte limit",
            budgets.source_bytes
        );
        return 3;
    }
    let context = match context {
        Some(context) => context,
        None => match direct_script_context(arguments, &source_label) {
            Ok(context) => context,
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        },
    };

    let observation = None;
    let invocation_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let delimiter = arguments.iter().position(|argument| argument == "--");
    let script_arguments = delimiter
        .and_then(|position| arguments.get(position + 1..))
        .unwrap_or_default()
        .to_vec();
    let audit_source_kind = match operation {
        ScriptOperation::Api => AuditSourceKind::Api,
        ScriptOperation::Eval => AuditSourceKind::Eval,
        ScriptOperation::Check | ScriptOperation::Run if source_label == "stdin" => {
            AuditSourceKind::Stdin
        }
        ScriptOperation::Check | ScriptOperation::Run => AuditSourceKind::File,
    };
    let capabilities = vec!["unrestricted_local".to_owned()];
    let mut audit_invocation = AuditInvocation {
        invocation_id: invocation_id.clone(),
        source_fingerprint: source_fingerprint(&source),
        source_kind: audit_source_kind,
        api_version: SCRIPT_API_VERSION,
        operation: operation_name.to_owned(),
        requested_profile: requested_profile.to_owned(),
        effective_profile: "unrestricted".to_owned(),
        requested_capabilities: capabilities.clone(),
        effective_capabilities: capabilities,
        requested_budgets: audit_budgets(&budgets),
        effective_budgets: audit_budgets(&budgets),
        broker_operation_ids: Vec::new(),
    };
    let audit_sink = match ScriptAuditSink::discover() {
        Ok(sink) => sink,
        Err(error) => return report_audit_error(error),
    };
    let audit_started = Instant::now();
    let invocation_temp = match OwnedScriptTemp::create(&invocation_id) {
        Ok(owned) => Some(owned),
        Err(error) => {
            let outcome = AuditOutcome {
                duration_ms: audit_duration_ms(audit_started),
                result_class: "host".to_owned(),
                failure_code: Some("host_temp_create".to_owned()),
                denied: false,
                cancelled: false,
                timed_out: false,
                crashed: false,
            };
            if let Err(audit_error) = audit_sink.append(&audit_invocation, &outcome) {
                return report_audit_error(audit_error);
            }
            eprintln!("host_temp_create: invocation temporary root is unavailable: {error}");
            return 1;
        }
    };
    let invocation = ScriptInvocation {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id,
        api_version: SCRIPT_API_VERSION,
        operation,
        profile,
        source_label,
        source,
        project_root: Some(context.project_root.display().to_string()),
        invocation_temp_root: invocation_temp.as_ref().map(OwnedScriptTemp::display),
        arguments: script_arguments,
        budgets,
        observation,
    };
    let executable = match script_worker_executable() {
        Ok(path) => path,
        Err(message) => {
            let outcome = AuditOutcome {
                duration_ms: audit_duration_ms(audit_started),
                result_class: "configuration".to_owned(),
                failure_code: Some("host_worker_missing".to_owned()),
                denied: true,
                cancelled: false,
                timed_out: false,
                crashed: false,
            };
            if let Err(error) = audit_sink.append(&audit_invocation, &outcome) {
                return report_audit_error(error);
            }
            eprintln!("{message}");
            return 2;
        }
    };
    let expected_invocation_id = invocation.invocation_id.clone();
    let deadline = Duration::from_millis(invocation.budgets.wall_time_ms);
    let broker_budgets = invocation.budgets.clone();
    let broker_profile = invocation.profile;
    let (mut result, cancel_requested) = match WorkerSupervisor::invoke(
        &executable,
        Some(&context.working_directory),
        invocation,
        deadline,
        Duration::from_millis(150),
        |request, remaining| {
            handle_script_broker(request, broker_profile, &broker_budgets, remaining)
        },
    ) {
        Ok(outcome) => {
            let _worker_pid = outcome.worker_pid;
            audit_invocation.broker_operation_ids = outcome.broker_operation_ids;
            (outcome.result, outcome.cancel_requested)
        }
        Err(error) => {
            let audit_outcome = audit_outcome_for_supervisor_error(&error, audit_started);
            if let Err(audit_error) = audit_sink.append(&audit_invocation, &audit_outcome) {
                return report_audit_error(audit_error);
            }
            return report_supervisor_error(error);
        }
    };
    if result.envelope_version != SCRIPT_ENVELOPE_VERSION
        || result.api_version != SCRIPT_API_VERSION
        || result.invocation_id != expected_invocation_id
        || result.operation != Some(operation)
        || result.profile != Some(profile)
    {
        let audit_outcome = AuditOutcome {
            duration_ms: audit_duration_ms(audit_started),
            result_class: "host".to_owned(),
            failure_code: Some("host_worker_protocol".to_owned()),
            denied: true,
            cancelled: cancel_requested,
            timed_out: false,
            crashed: false,
        };
        if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
            return report_audit_error(error);
        }
        eprintln!(
            "agenterm-script returned a mismatched protocol result \
             (envelope/API/invocation/operation/profile identity)"
        );
        return 1;
    }
    let result_consistent = if result.ok {
        result.exit_class == ScriptExitClass::Success && result.failure.is_none()
    } else {
        result.exit_class != ScriptExitClass::Success && result.failure.is_some()
    };
    if !result_consistent {
        let audit_outcome = AuditOutcome {
            duration_ms: audit_duration_ms(audit_started),
            result_class: "host".to_owned(),
            failure_code: Some("host_worker_protocol".to_owned()),
            denied: true,
            cancelled: cancel_requested,
            timed_out: false,
            crashed: false,
        };
        if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
            return report_audit_error(error);
        }
        eprintln!("agenterm-script returned an inconsistent result envelope");
        return 1;
    }
    if let Some(view) = api_view.as_ref() {
        let Some(catalog) = result.value.as_mut() else {
            let audit_outcome = AuditOutcome {
                duration_ms: audit_duration_ms(audit_started),
                result_class: "host".to_owned(),
                failure_code: Some("host_worker_protocol".to_owned()),
                denied: true,
                cancelled: cancel_requested,
                timed_out: false,
                crashed: false,
            };
            if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
                return report_audit_error(error);
            }
            eprintln!("agenterm-script returned an API result without a catalog");
            return 1;
        };
        if let Err(error) = filter_script_api_catalog(catalog, view) {
            let audit_outcome = AuditOutcome {
                duration_ms: audit_duration_ms(audit_started),
                result_class: "host".to_owned(),
                failure_code: Some("host_worker_protocol".to_owned()),
                denied: true,
                cancelled: cancel_requested,
                timed_out: false,
                crashed: false,
            };
            if let Err(audit_error) = audit_sink.append(&audit_invocation, &audit_outcome) {
                return report_audit_error(audit_error);
            }
            eprintln!("agenterm-script returned an invalid API catalog: {error}");
            return 1;
        }
    }
    let audit_outcome = audit_outcome_for_result(&result, audit_started, cancel_requested);
    if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
        return report_audit_error(error);
    }
    let rendered_stdout = if has_option(arguments, "--json") {
        Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_owned())
        ))
    } else if result.ok {
        let mut output = String::new();
        if !result.stdout.is_empty() {
            output.push_str(&result.stdout);
            if !result.stdout.ends_with('\n') {
                output.push('\n');
            }
        }
        if let Some(value) = result.value {
            if operation == ScriptOperation::Api {
                match render_script_api_tree(&value) {
                    Ok(tree) => {
                        output.push_str(&tree);
                        output.push('\n');
                    }
                    Err(error) => {
                        eprintln!("agenterm-script returned an invalid API catalog: {error}");
                        return 1;
                    }
                }
            } else if let Some(value) = value.as_str() {
                output.push_str(value);
                output.push('\n');
            } else {
                output.push_str(
                    &serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
                );
                output.push('\n');
            }
        } else if operation == ScriptOperation::Check {
            output.push_str("OK\n");
        }
        Some(output)
    } else if let Some(failure) = &result.failure {
        eprintln!(
            "{}",
            serde_json::json!({
                "code": failure.code,
                "message": failure.message,
                "invocation_id": result.invocation_id,
                "exit_class": result.exit_class,
            })
        );
        None
    } else {
        None
    };
    if let Some(output) = rendered_stdout
        && let Err(code) = write_script_stdout(&output)
    {
        return code;
    }
    if result.ok {
        0
    } else {
        match result.exit_class.as_str() {
            "configuration" => 2,
            "limit" => 3,
            "child" => 4,
            "cancelled" => 5,
            "fleet" => 6,
            _ => 1,
        }
    }
}

fn direct_script_context(
    arguments: &[String],
    source_label: &str,
) -> Result<ScriptExecutionContext, String> {
    let current =
        std::env::current_dir().map_err(|error| format!("script_current_dir: {error}"))?;
    let working_directory = match option_value(arguments, "--cwd") {
        Some(path) => canonical_script_directory(&current, path, "script_cwd")?,
        None => current.clone(),
    };
    let project_root = match option_value(arguments, "--project-root") {
        Some(path) => canonical_script_directory(&current, path, "script_project_root")?,
        None => {
            let source = Path::new(source_label);
            if source.is_file() {
                source
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| current.clone())
            } else {
                current
            }
        }
    };
    Ok(ScriptExecutionContext {
        project_root,
        working_directory,
    })
}

fn canonical_script_directory(base: &Path, value: &str, code: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        base.join(value)
    };
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|error| format!("{code}: {}: {error}", candidate.display()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "{code}: {} is not a directory",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn run_script_task_command(arguments: &[String]) -> i32 {
    let Some(action) = arguments.get(2).map(String::as_str) else {
        eprintln!("script task requires list, show, check, or run");
        return 2;
    };
    let manifest = match script_task_manifest(arguments) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let catalog = match load_task_catalog(&manifest) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    match action {
        "list" => print_task_catalog(&catalog, has_option(arguments, "--json"))
            .map_or_else(|code| code, |()| 0),
        "show" => {
            let Some(id) = arguments.get(3).filter(|value| !value.starts_with('-')) else {
                eprintln!("script task show requires a task ID");
                return 2;
            };
            let matches = catalog
                .tasks
                .iter()
                .filter(|task| task.id == *id)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                eprintln!("task_not_found: {id}");
                return 2;
            }
            if has_option(arguments, "--json") {
                let output = format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": catalog.schema_version,
                        "runtime_version": catalog.runtime_version,
                        "script_api_version": catalog.script_api_version,
                        "script_catalog_schema_version": catalog.script_catalog_schema_version,
                        "manifest_path": catalog.manifest_path,
                        "project_root": catalog.project_root,
                        "project_id": catalog.project_id,
                        "project_version": catalog.project_version,
                        "requirements": catalog.requirements,
                        "origin": catalog.origin,
                        "provenance": catalog.provenance,
                        "compatible": catalog.compatible,
                        "compatibility_reason": catalog.compatibility_reason,
                        "tasks": matches,
                    }))
                    .unwrap_or_else(|_| "{}".to_owned())
                );
                if let Err(code) = write_script_stdout(&output) {
                    return code;
                }
            } else {
                let mut output = String::new();
                for task in matches {
                    output.push_str(&task_entry_text(task));
                    output.push('\n');
                }
                if let Err(code) = write_script_stdout(&output) {
                    return code;
                }
            }
            0
        }
        "check" => {
            let id = arguments.get(3).filter(|value| !value.starts_with('-'));
            let result = match id {
                Some(id) => resolve_task(&catalog, id).map(|_| ()),
                None if !catalog.compatible => Err(format!(
                    "task_project_incompatible: {}",
                    catalog
                        .compatibility_reason
                        .as_deref()
                        .unwrap_or("project requirements are not satisfied")
                )),
                None => catalog
                    .tasks
                    .iter()
                    .find(|task| task.status == ScriptTaskStatus::Degraded)
                    .map_or(Ok(()), |task| {
                        Err(format!(
                            "task_degraded: {}: {}",
                            task.id,
                            task.degraded_reason.as_deref().unwrap_or("invalid task")
                        ))
                    }),
            };
            if let Err(error) = result {
                eprintln!("{error}");
                return 2;
            }
            if has_option(arguments, "--json") {
                if let Err(code) = print_task_catalog(&catalog, true) {
                    return code;
                }
            } else {
                if let Err(code) = write_script_stdout("OK\n") {
                    return code;
                }
            }
            0
        }
        "run" => {
            let Some(id) = arguments.get(3).filter(|value| !value.starts_with('-')) else {
                eprintln!("script task run requires a task ID");
                return 2;
            };
            let task = match resolve_task(&catalog, id) {
                Ok(task) => task,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            run_resolved_script_task(arguments, task)
        }
        other => {
            eprintln!("unknown script task operation: {other}");
            2
        }
    }
}

fn script_task_manifest(arguments: &[String]) -> Result<PathBuf, String> {
    if let Some(path) = option_value(arguments, "--manifest") {
        return Ok(PathBuf::from(path));
    }
    let current =
        std::env::current_dir().map_err(|error| format!("task_manifest_current_dir: {error}"))?;
    discover_task_manifest(&current)
}

fn print_task_catalog(catalog: &ScriptTaskCatalog, json: bool) -> std::result::Result<(), i32> {
    if json {
        return write_script_stdout(&format!(
            "{}\n",
            serde_json::to_string_pretty(catalog).unwrap_or_else(|_| "{}".to_owned())
        ));
    }
    let mut output = String::new();
    for task in &catalog.tasks {
        output.push_str(&task_entry_text(task));
        output.push('\n');
    }
    write_script_stdout(&output)
}

fn task_entry_text(task: &crate::script_project::ScriptTaskEntry) -> String {
    let status = match task.status {
        ScriptTaskStatus::Ready => "ready",
        ScriptTaskStatus::Degraded => "degraded",
    };
    let detail = task
        .degraded_reason
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(task.description.as_str());
    let platforms = task
        .platforms
        .iter()
        .map(|platform| match platform {
            crate::script_project::ScriptTaskPlatform::Windows => "windows",
            crate::script_project::ScriptTaskPlatform::Linux => "linux",
            crate::script_project::ScriptTaskPlatform::Macos => "macos",
        })
        .collect::<Vec<_>>()
        .join(",");
    let dependencies = if task.dependencies.is_empty() {
        "-".to_owned()
    } else {
        task.dependencies.join(",")
    };
    let side_effects = if task.side_effects.is_empty() {
        "none".to_owned()
    } else {
        task.side_effects
            .iter()
            .map(|effect| match effect {
                crate::script_project::ScriptTaskSideEffect::RepositoryWrite => "repository_write",
                crate::script_project::ScriptTaskSideEffect::ArtifactWrite => "artifact_write",
                crate::script_project::ScriptTaskSideEffect::ProcessSpawn => "process_spawn",
                crate::script_project::ScriptTaskSideEffect::GuiSpawn => "gui_spawn",
                crate::script_project::ScriptTaskSideEffect::NetworkLoopback => "network_loopback",
                crate::script_project::ScriptTaskSideEffect::GitMutation => "git_mutation",
                crate::script_project::ScriptTaskSideEffect::RemotePublish => "remote_publish",
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let contract = task.contract.as_ref();
    let inputs = contract
        .map(|value| value.inputs.join(","))
        .unwrap_or_else(|| "-".to_owned());
    let outputs = contract
        .map(|value| value.outputs.join(","))
        .unwrap_or_else(|| "-".to_owned());
    let evidence = contract
        .map(|value| value.evidence.join(","))
        .unwrap_or_else(|| "-".to_owned());
    let network = contract
        .map(|value| {
            if value.network.is_empty() {
                return "none".to_owned();
            }
            value
                .network
                .iter()
                .map(|policy| match policy {
                    crate::script_project::ScriptTaskNetwork::DependencyFetch => "dependency_fetch",
                    crate::script_project::ScriptTaskNetwork::Loopback => "loopback",
                    crate::script_project::ScriptTaskNetwork::RemotePublish => "remote_publish",
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| "-".to_owned());
    let budget = contract
        .map(|value| {
            format!(
                "{}/{}/{}",
                value.budget.timeout_ms, value.budget.max_operations, value.budget.max_output_bytes
            )
        })
        .unwrap_or_else(|| "-".to_owned());
    format!(
        "{}\t{}\tplatforms={}\tdependencies={}\tinputs={}\toutputs={}\t\
         side_effects={}\tnetwork={}\tbudget={}\tevidence={}\t{}",
        task.id,
        status,
        platforms,
        dependencies,
        inputs,
        outputs,
        side_effects,
        network,
        budget,
        evidence,
        detail
    )
}

fn run_resolved_script_task(arguments: &[String], task: ResolvedScriptTask) -> i32 {
    let missing = task
        .env
        .iter()
        .filter(|name| std::env::var_os(name).is_none())
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        eprintln!(
            "task_environment_missing: {} requires {}",
            task.id,
            missing.join(", ")
        );
        return 2;
    }
    let mut translated = vec![
        "script".to_owned(),
        "run".to_owned(),
        task.entry.display().to_string(),
        "--profile".to_owned(),
        task.profile,
    ];
    if let Some(budget) = task.budget {
        let mut declared_options = vec![
            ("--timeout-ms", budget.timeout_ms),
            ("--max-operations", budget.max_operations),
            ("--max-output-bytes", budget.max_output_bytes),
        ];
        if let Some(value) = budget.max_collection_items {
            declared_options.push(("--max-collection-items", value));
        }
        if let Some(value) = budget.max_string_bytes {
            declared_options.push(("--max-string-bytes", value));
        }
        for (option, declared) in declared_options {
            let selected = match option_value(arguments, option) {
                Some(value) => match value.parse::<u64>() {
                    Ok(value) if value > 0 && value <= declared => value,
                    Ok(value) => {
                        eprintln!(
                            "task_budget_exceeded: {} {} requested {}, declared {}",
                            task.id, option, value, declared
                        );
                        return 2;
                    }
                    Err(error) => {
                        eprintln!("task_budget_invalid: {} {}: {}", task.id, option, error);
                        return 2;
                    }
                },
                None => declared,
            };
            translated.push(option.to_owned());
            translated.push(selected.to_string());
        }
        for option in ["--max-collection-items", "--max-string-bytes"] {
            let declared = match option {
                "--max-collection-items" => budget.max_collection_items,
                "--max-string-bytes" => budget.max_string_bytes,
                _ => unreachable!(),
            };
            if declared.is_none()
                && let Some(value) = option_value(arguments, option)
            {
                translated.push(option.to_owned());
                translated.push(value.to_owned());
            }
        }
    } else {
        for option in [
            "--timeout-ms",
            "--max-operations",
            "--max-output-bytes",
            "--max-collection-items",
            "--max-string-bytes",
        ] {
            if let Some(value) = option_value(arguments, option) {
                translated.push(option.to_owned());
                translated.push(value.to_owned());
            }
        }
    }
    if has_option(arguments, "--json") {
        translated.push("--json".to_owned());
    }
    translated.push("--".to_owned());
    translated.extend(task.args);
    if let Some(delimiter) = arguments.iter().position(|value| value == "--") {
        translated.extend_from_slice(&arguments[delimiter + 1..]);
    }
    run_script_command_with_context(
        &translated,
        Some(ScriptExecutionContext {
            project_root: task.project_root,
            working_directory: task.cwd,
        }),
    )
}

fn report_supervisor_error(error: SupervisorError) -> i32 {
    let (code, message, exit_class, exit_code) = match error {
        SupervisorError::ConcurrencyLimit => (
            "host_concurrency_limit",
            "script worker concurrency limit reached".to_owned(),
            "configuration",
            2,
        ),
        SupervisorError::HardTimeout { worker_pid } => (
            "host_hard_timeout",
            format!("script worker {worker_pid} exceeded the host deadline and was terminated"),
            "limit",
            3,
        ),
        SupervisorError::WorkerCrash {
            worker_pid,
            exit_code: worker_exit,
        } => (
            "host_worker_crash",
            format!("script worker {worker_pid} exited before a valid result ({worker_exit:?})"),
            "host",
            1,
        ),
        SupervisorError::Spawn(message) => ("host_worker_spawn", message, "host", 1),
        SupervisorError::Transport(message) => ("host_worker_transport", message, "host", 1),
        SupervisorError::Protocol(message) => ("host_worker_protocol", message, "host", 1),
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "code": code,
            "message": message,
            "exit_class": exit_class,
        })
    );
    exit_code
}

fn script_broker_error(code: &str, message: impl Into<String>) -> ScriptBrokerResponse {
    ScriptBrokerResponse {
        ok: false,
        value: None,
        error: Some(ScriptBrokerError {
            code: code.to_owned(),
            message: message.into(),
            details: None,
        }),
    }
}

fn script_broker_ipc(
    arguments: Vec<String>,
    timeout: Duration,
) -> Result<String, ScriptBrokerResponse> {
    match send_ipc_request_to_timeout(&ipc_address(), arguments, timeout) {
        Ok(response) if response.ok => Ok(response.output),
        Ok(response) => {
            let code = match response.error_code.as_str() {
                "server_restart" | "journal_gap" | "future_sequence" => {
                    response.error_code.as_str()
                }
                _ => "broker_host_error",
            };
            Err(script_broker_error(code, response.error))
        }
        Err(error) => Err(script_broker_error(
            "broker_transport",
            format!("{error:#}"),
        )),
    }
}

fn handle_script_broker(
    request: &ScriptBrokerRequest,
    profile: ScriptProfile,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> ScriptBrokerResponse {
    if request.operation == "fleet.call" {
        return match script_fleet_call(&request.arguments, profile, budgets, remaining) {
            Ok(value) => ScriptBrokerResponse {
                ok: true,
                value: Some(value),
                error: None,
            },
            Err(response) => response,
        };
    }
    let started = Instant::now();
    let parse_json = |output: String| {
        serde_json::from_str::<serde_json::Value>(&output)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))
    };
    let value = match request.operation.as_str() {
        "protocol.info" => {
            script_broker_ipc(vec!["protocol-info".to_owned()], remaining).and_then(parse_json)
        }
        "workspace.info" => script_broker_ipc(vec!["workspace-info".to_owned()], remaining)
            .and_then(parse_json)
            .and_then(|mut workspace| {
                let request_remaining = remaining.saturating_sub(started.elapsed());
                if request_remaining.is_zero() {
                    return Err(script_broker_error(
                        "broker_transport",
                        "workspace observation exceeded the host deadline",
                    ));
                }
                let snapshot = script_broker_ipc(vec!["ui-snapshot".to_owned()], request_remaining)
                    .and_then(parse_json)?;
                workspace["event_position"] = snapshot
                    .get("event_position")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                Ok(workspace)
            }),
        "ui.snapshot" | "tabs.list" | "tabs.active" => {
            script_broker_ipc(vec!["ui-snapshot".to_owned()], remaining)
                .and_then(parse_json)
                .map(|snapshot| match request.operation.as_str() {
                    "tabs.list" => snapshot
                        .get("tabs")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                    "tabs.active" => snapshot
                        .get("tabs")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|tabs| {
                            tabs.iter().find(|tab| {
                                tab.get("active").and_then(serde_json::Value::as_bool) == Some(true)
                            })
                        })
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    _ => snapshot,
                })
        }
        "pane.capture" => {
            let tab = request
                .arguments
                .get("tab")
                .and_then(serde_json::Value::as_str);
            let requested = request
                .arguments
                .get("max_bytes")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            match (tab, requested) {
                (Some(tab), Some(requested))
                    if tab.starts_with('@') && (1..=budgets.capture_bytes).contains(&requested) =>
                {
                    script_broker_ipc(
                        vec![
                            "capture-pane".to_owned(),
                            "-p".to_owned(),
                            "-t".to_owned(),
                            tab.to_owned(),
                            "--max-bytes".to_owned(),
                            requested.to_string(),
                            "--json".to_owned(),
                        ],
                        remaining,
                    )
                    .and_then(parse_json)
                }
                _ => Err(script_broker_error(
                    "broker_invalid_arguments",
                    "pane.capture requires stable tab @ID and max_bytes within the capture budget",
                )),
            }
        }
        "events.read" => {
            let epoch = request
                .arguments
                .get("epoch")
                .and_then(serde_json::Value::as_str);
            let after = request
                .arguments
                .get("after")
                .and_then(serde_json::Value::as_u64);
            let limit = request
                .arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64);
            match (epoch, after, limit) {
                (Some(epoch), Some(after), Some(limit))
                    if limit > 0 && limit <= budgets.event_items as u64 =>
                {
                    script_broker_ipc(
                        vec![
                            "read-events".to_owned(),
                            "--epoch".to_owned(),
                            epoch.to_owned(),
                            "--after".to_owned(),
                            after.to_string(),
                            "--limit".to_owned(),
                            limit.to_string(),
                        ],
                        remaining,
                    )
                    .and_then(parse_json)
                }
                _ => Err(script_broker_error(
                    "broker_invalid_arguments",
                    "events.read requires epoch, nonnegative after, and a bounded positive limit",
                )),
            }
        }
        "events.wait" => script_broker_wait(&request.arguments, budgets, remaining),
        _ => Err(script_broker_error(
            "broker_operation_unknown",
            format!("unknown broker operation {}", request.operation),
        )),
    };
    match value {
        Ok(value) => ScriptBrokerResponse {
            ok: true,
            value: Some(value),
            error: None,
        },
        Err(response) => response,
    }
}

fn script_fleet_call(
    arguments: &serde_json::Value,
    profile: ScriptProfile,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> Result<serde_json::Value, ScriptBrokerResponse> {
    let operation_id = arguments
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            script_broker_error(
                "broker_invalid_arguments",
                "fleet.call requires operation_id",
            )
        })?;
    let parameters = arguments
        .get("parameters")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            script_broker_error(
                "broker_invalid_arguments",
                "fleet.call requires an object parameters value",
            )
        })?;
    let operation = operation_by_id(operation_id).ok_or_else(|| {
        script_broker_error(
            "broker_operation_unknown",
            format!("unknown Fleet operation {operation_id}"),
        )
    })?;
    if !operation.available {
        return Err(script_broker_error(
            "broker_operation_degraded",
            format!("Fleet operation {operation_id} is not available"),
        ));
    }
    validate_fleet_parameters(operation, &parameters, budgets)?;

    if operation.class == OperationClass::Observe {
        let nested = ScriptBrokerRequest {
            operation: operation_id.to_owned(),
            arguments: parameters,
        };
        let response = handle_script_broker(&nested, profile, budgets, remaining);
        return if response.ok {
            Ok(response.value.unwrap_or(serde_json::Value::Null))
        } else {
            Err(response)
        };
    }

    let command = fleet_mutation_command(operation_id, &parameters)?;
    script_fleet_mutation(operation, &parameters, command, budgets, remaining)
}

fn validate_fleet_parameters(
    operation: &crate::operations::OperationSpec,
    parameters: &serde_json::Value,
    budgets: &ScriptBudgets,
) -> Result<(), ScriptBrokerResponse> {
    let object = parameters
        .as_object()
        .expect("fleet.call parameters were checked as an object");
    if let Some(unknown) = object
        .keys()
        .find(|name| !operation.parameters.iter().any(|spec| spec.name == *name))
    {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            format!("{} does not accept parameter {unknown}", operation.id),
        ));
    }
    for spec in operation.parameters {
        let Some(value) = object.get(spec.name) else {
            if spec.required {
                return Err(script_broker_error(
                    "broker_invalid_arguments",
                    format!("{} requires parameter {}", operation.id, spec.name),
                ));
            }
            continue;
        };
        let valid_type = match spec.value_type {
            "string" | "session_name" => value.as_str().is_some(),
            "stable_tab_id" => value.as_str().is_some_and(|tab| {
                tab.len() <= 32
                    && tab.strip_prefix('@').is_some_and(|id| {
                        !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
                    })
            }),
            "uint32" | "uint64" => value.as_u64().is_some(),
            "integer" => value.as_i64().is_some(),
            _ => false,
        };
        if !valid_type {
            return Err(script_broker_error(
                "broker_invalid_arguments",
                format!(
                    "{} parameter {} must be {}",
                    operation.id, spec.name, spec.value_type
                ),
            ));
        }
        if spec.value_type == "string"
            && value.as_str().is_some_and(|text| {
                spec.minimum
                    .is_some_and(|minimum| text.len() < minimum.max(0) as usize)
                    || spec
                        .maximum
                        .is_some_and(|maximum| text.len() > maximum.max(0) as usize)
            })
        {
            return Err(script_broker_error(
                "broker_invalid_arguments",
                format!(
                    "{} parameter {} is outside its UTF-8 byte bounds",
                    operation.id, spec.name
                ),
            ));
        }
        let integer = value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()));
        if integer.is_some_and(|value| {
            spec.minimum.is_some_and(|minimum| value < minimum)
                || spec.maximum.is_some_and(|maximum| value > maximum)
        }) {
            return Err(script_broker_error(
                "broker_invalid_arguments",
                format!(
                    "{} parameter {} is outside its bounds",
                    operation.id, spec.name
                ),
            ));
        }
    }
    if operation.id == "pane.capture"
        && parameters["max_bytes"]
            .as_u64()
            .is_none_or(|value| value == 0 || value > budgets.capture_bytes as u64)
    {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            "pane.capture max_bytes exceeds the invocation capture budget",
        ));
    }
    if operation.id == "events.read"
        && parameters["limit"]
            .as_u64()
            .is_none_or(|value| value == 0 || value > budgets.event_items as u64)
    {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            "events.read limit exceeds the invocation event budget",
        ));
    }
    if operation.id == "events.wait"
        && parameters["timeout_ms"]
            .as_u64()
            .is_none_or(|value| value == 0 || value > budgets.wait_time_ms)
    {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            "events.wait timeout_ms exceeds the invocation wait budget",
        ));
    }
    Ok(())
}

fn fleet_mutation_command(
    operation_id: &str,
    parameters: &serde_json::Value,
) -> Result<Vec<String>, ScriptBrokerResponse> {
    let arguments = match operation_id {
        "ui.tabs.show" => vec!["ui-action".to_owned(), "tabs-show".to_owned()],
        "ui.tabs.hide" => vec!["ui-action".to_owned(), "tabs-hide".to_owned()],
        "ui.tabs.toggle" => vec!["ui-action".to_owned(), "tabs-toggle".to_owned()],
        "ui.tabs.set-width" => vec![
            "ui-action".to_owned(),
            "tabs-set-width".to_owned(),
            "--width".to_owned(),
            parameters["width"]
                .as_i64()
                .expect("validated Fleet width")
                .to_string(),
        ],
        "tabs.set-note" => vec![
            "set-tab-note".to_owned(),
            "-t".to_owned(),
            parameters["tab"]
                .as_str()
                .expect("validated Fleet tab target")
                .to_owned(),
            parameters["note"]
                .as_str()
                .expect("validated Fleet tab note")
                .to_owned(),
        ],
        "server.kill" => {
            let mut arguments = vec!["kill-server".to_owned()];
            if let Some(target) = parameters.get("target").and_then(serde_json::Value::as_str) {
                arguments.extend(["-t".to_owned(), target.to_owned()]);
            }
            arguments
        }
        "workspace.shutdown" => vec!["shutdown".to_owned()],
        _ => {
            return Err(script_broker_error(
                "broker_operation_unknown",
                format!("no Fleet adapter exists for {operation_id}"),
            ));
        }
    };
    Ok(arguments)
}

fn script_fleet_mutation(
    operation: &crate::operations::OperationSpec,
    parameters: &serde_json::Value,
    command: Vec<String>,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> Result<serde_json::Value, ScriptBrokerResponse> {
    let started = Instant::now();
    let address = ipc_address();
    let response = send_ipc_request_to_timeout(&address, command, remaining)
        .map_err(|error| script_broker_error("broker_transport", format!("{error:#}")))?;
    let response = await_ui_client_relay_to(
        response,
        &address,
        remaining.saturating_sub(started.elapsed()),
    )
    .map_err(|error| script_broker_error("broker_transport", format!("{error:#}")))?;
    let receipt = response.receipt.as_ref().ok_or_else(|| {
        script_broker_error(
            "broker_receipt_missing",
            format!("{} did not return a native control receipt", operation.id),
        )
    })?;
    let receipt_json = serde_json::to_value(receipt)
        .map_err(|error| script_broker_error("broker_invalid_receipt", error.to_string()))?;
    if !response.ok {
        let mut error = script_broker_error(
            if response.error_code.is_empty() {
                "broker_host_error"
            } else {
                &response.error_code
            },
            response.error,
        );
        if let Some(details) = error.error.as_mut() {
            details.details = Some(serde_json::json!({"receipt": receipt_json}));
        }
        return Err(error);
    }
    let mut output = if response.output.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&response.output)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))?
    };
    if operation.id == "tabs.set-note" {
        let request_remaining = remaining.saturating_sub(started.elapsed());
        let snapshot_text = script_broker_ipc(vec!["ui-snapshot".to_owned()], request_remaining)?;
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_text)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))?;
        let target = parameters["tab"]
            .as_str()
            .expect("validated Fleet tab target");
        output = snapshot
            .get("tabs")
            .and_then(serde_json::Value::as_array)
            .and_then(|tabs| {
                tabs.iter()
                    .find(|tab| tab.get("id").and_then(serde_json::Value::as_str) == Some(target))
            })
            .cloned()
            .ok_or_else(|| {
                script_broker_error(
                    "broker_post_state_missing",
                    "mutated tab was absent from the post-state snapshot",
                )
            })?;
    }
    let events = collect_fleet_receipt_events(
        operation,
        &receipt_json,
        budgets,
        remaining.saturating_sub(started.elapsed()),
    );
    let (verified, reason) =
        verify_fleet_post_state(operation.id, parameters, &output, &events, &receipt_json);
    Ok(serde_json::json!({
        "receipt": receipt_json,
        "events": events,
        "post_state": {
            "verified": verified,
            "reason": reason,
            "value": output,
        }
    }))
}

fn collect_fleet_receipt_events(
    operation: &crate::operations::OperationSpec,
    receipt: &serde_json::Value,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> Vec<serde_json::Value> {
    let Some(before) = receipt.get("before_position") else {
        return Vec::new();
    };
    let Some(epoch) = before.get("epoch").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    let Some(after) = before.get("sequence").and_then(serde_json::Value::as_u64) else {
        return Vec::new();
    };
    let upper = receipt
        .get("after_position")
        .and_then(|position| position.get("sequence"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(after);
    if upper <= after || remaining.is_zero() {
        return Vec::new();
    }
    let output = match script_broker_ipc(
        vec![
            "read-events".to_owned(),
            "--epoch".to_owned(),
            epoch.to_owned(),
            "--after".to_owned(),
            after.to_string(),
            "--limit".to_owned(),
            budgets.event_items.min(1024).to_string(),
        ],
        remaining,
    ) {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };
    let batch: serde_json::Value = match serde_json::from_str(&output) {
        Ok(batch) => batch,
        Err(_) => return Vec::new(),
    };
    let request_id = receipt
        .get("request_id")
        .and_then(serde_json::Value::as_str);
    let resolved_tab = receipt
        .pointer("/resolved/tab_id")
        .and_then(serde_json::Value::as_u64);
    batch
        .get("events")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|event| {
            event
                .get("sequence")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|sequence| sequence <= upper)
                && operation.events.iter().any(|kind| {
                    event.get("kind").and_then(serde_json::Value::as_str) == Some(*kind)
                })
                && (event.get("request_id").and_then(serde_json::Value::as_str) == request_id
                    || event
                        .get("operation_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(operation.id)
                    || event
                        .pointer("/payload/operation_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(operation.id)
                    || (operation.id == "tabs.set-note"
                        && resolved_tab.is_some()
                        && event.get("tab_id").and_then(serde_json::Value::as_u64) == resolved_tab))
        })
        .cloned()
        .collect()
}

fn verify_fleet_post_state(
    operation_id: &str,
    parameters: &serde_json::Value,
    value: &serde_json::Value,
    events: &[serde_json::Value],
    receipt: &serde_json::Value,
) -> (bool, &'static str) {
    let final_receipt = matches!(
        receipt.get("outcome").and_then(serde_json::Value::as_str),
        Some("committed" | "no_op")
    );
    match operation_id {
        "ui.tabs.show" => (
            final_receipt
                && value
                    .pointer("/layout/sidebar/visible")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true),
            "ui_snapshot",
        ),
        "ui.tabs.hide" => (
            final_receipt
                && value
                    .pointer("/layout/sidebar/visible")
                    .and_then(serde_json::Value::as_bool)
                    == Some(false),
            "ui_snapshot",
        ),
        "ui.tabs.toggle" => (
            final_receipt
                && value
                    .pointer("/layout/sidebar/visible")
                    .and_then(serde_json::Value::as_bool)
                    .is_some()
                && (!events.is_empty()
                    || receipt.get("outcome").and_then(serde_json::Value::as_str) == Some("no_op")),
            "ui_snapshot_and_event",
        ),
        "ui.tabs.set-width" => (
            final_receipt
                && value
                    .pointer("/layout/sidebar/configured_width")
                    .and_then(serde_json::Value::as_i64)
                    == parameters.get("width").and_then(serde_json::Value::as_i64),
            "ui_snapshot",
        ),
        "tabs.set-note" => (
            final_receipt
                && value.get("id").and_then(serde_json::Value::as_str)
                    == parameters.get("tab").and_then(serde_json::Value::as_str)
                && value.get("note").and_then(serde_json::Value::as_str)
                    == parameters.get("note").and_then(serde_json::Value::as_str)
                && (!events.is_empty()
                    || receipt.get("outcome").and_then(serde_json::Value::as_str) == Some("no_op")),
            "tab_snapshot_and_event",
        ),
        _ => (false, "destructive_post_state_unavailable"),
    }
}

fn script_broker_wait(
    arguments: &serde_json::Value,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> Result<serde_json::Value, ScriptBrokerResponse> {
    let epoch = arguments
        .get("epoch")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "epoch is required"))?;
    let mut after = arguments
        .get("after")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "after is required"))?;
    let kind = arguments
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "kind is required"))?;
    let tab = arguments.get("tab").and_then(serde_json::Value::as_str);
    let timeout_ms = arguments
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "timeout_ms is required"))?;
    if timeout_ms == 0 || timeout_ms > budgets.wait_time_ms {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            "timeout_ms exceeds the wait budget",
        ));
    }
    let deadline = Instant::now()
        + Duration::from_millis(timeout_ms)
            .min(remaining.saturating_sub(Duration::from_millis(10)));
    loop {
        let ipc_remaining = deadline.saturating_duration_since(Instant::now());
        if ipc_remaining.is_zero() {
            return Err(script_broker_error(
                "event_wait_timeout",
                format!("no {kind} event arrived within {timeout_ms} ms"),
            ));
        }
        let output = match script_broker_ipc(
            vec![
                "read-events".to_owned(),
                "--epoch".to_owned(),
                epoch.to_owned(),
                "--after".to_owned(),
                after.to_string(),
                "--limit".to_owned(),
                budgets.event_items.min(256).to_string(),
            ],
            ipc_remaining,
        ) {
            Ok(output) => output,
            Err(response)
                if response
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "broker_transport")
                    && Instant::now() >= deadline =>
            {
                return Err(script_broker_error(
                    "event_wait_timeout",
                    format!("no {kind} event arrived within {timeout_ms} ms"),
                ));
            }
            Err(response) => return Err(response),
        };
        let batch: serde_json::Value = serde_json::from_str(&output)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))?;
        if let Some(events) = batch.get("events").and_then(serde_json::Value::as_array) {
            for event in events {
                if let Some(sequence) = event.get("sequence").and_then(serde_json::Value::as_u64) {
                    after = after.max(sequence);
                }
                if event.get("kind").and_then(serde_json::Value::as_str) == Some(kind)
                    && tab.is_none_or(|tab| {
                        event.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab)
                    })
                {
                    return Ok(event.clone());
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(script_broker_error(
                "event_wait_timeout",
                format!("no {kind} event arrived within {timeout_ms} ms"),
            ));
        }
        thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn audit_budgets(budgets: &ScriptBudgets) -> AuditBudgets {
    AuditBudgets {
        source_bytes: budgets.source_bytes,
        operations: budgets.operations,
        call_depth: budgets.call_depth,
        expression_depth: budgets.expression_depth,
        collection_items: budgets.collection_items,
        string_bytes: budgets.string_bytes,
        output_bytes: budgets.output_bytes,
        wall_time_ms: budgets.wall_time_ms,
        broker_requests: budgets.broker_requests,
        broker_return_bytes: budgets.broker_return_bytes,
        capture_bytes: budgets.capture_bytes,
        event_items: budgets.event_items,
        wait_time_ms: budgets.wait_time_ms,
    }
}

fn audit_duration_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn audit_outcome_for_result(
    result: &crate::script_protocol::ScriptResult,
    started: Instant,
    cancel_requested: bool,
) -> AuditOutcome {
    let failure_code = result.failure.as_ref().map(|failure| failure.code.clone());
    let denied = result.exit_class == ScriptExitClass::Configuration
        || failure_code.as_deref().is_some_and(|code| {
            matches!(
                code,
                "script_api_unknown"
                    | "script_api_unavailable"
                    | "configuration_observation"
                    | "protocol_broker_unavailable"
            )
        });
    let timed_out = failure_code.as_deref() == Some("limit_wall_time");
    AuditOutcome {
        duration_ms: audit_duration_ms(started),
        result_class: result.exit_class.as_str().to_owned(),
        failure_code,
        denied,
        cancelled: cancel_requested || result.exit_class == ScriptExitClass::Cancelled,
        timed_out,
        crashed: false,
    }
}

fn audit_outcome_for_supervisor_error(error: &SupervisorError, started: Instant) -> AuditOutcome {
    let (result_class, failure_code, denied, cancelled, timed_out, crashed) = match error {
        SupervisorError::ConcurrencyLimit => (
            "configuration",
            "host_concurrency_limit",
            true,
            false,
            false,
            false,
        ),
        SupervisorError::HardTimeout { .. } => {
            ("limit", "host_hard_timeout", false, true, true, false)
        }
        SupervisorError::WorkerCrash { .. } => {
            ("host", "host_worker_crash", false, false, false, true)
        }
        SupervisorError::Spawn(_) => ("host", "host_worker_spawn", true, false, false, false),
        SupervisorError::Transport(_) => {
            ("host", "host_worker_transport", false, false, false, false)
        }
        SupervisorError::Protocol(_) => ("host", "host_worker_protocol", true, false, false, false),
    };
    AuditOutcome {
        duration_ms: audit_duration_ms(started),
        result_class: result_class.to_owned(),
        failure_code: Some(failure_code.to_owned()),
        denied,
        cancelled,
        timed_out,
        crashed,
    }
}

fn report_audit_error(message: String) -> i32 {
    eprintln!(
        "{}",
        serde_json::json!({
            "code": "host_audit_write",
            "message": message,
            "exit_class": "host",
        })
    );
    1
}

fn read_script_source(
    reader: impl Read,
    limit: usize,
) -> std::result::Result<String, (i32, String)> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024).saturating_add(1));
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| (1, error.to_string()))?;
    if bytes.len() > limit {
        return Err((3, format!("script source exceeds the {limit} byte limit")));
    }
    String::from_utf8(bytes).map_err(|error| (1, format!("script source is not UTF-8: {error}")))
}

fn script_operand(arguments: &[String]) -> Option<&str> {
    let mut position = 2;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--" => return None,
            "--profile"
            | "--timeout-ms"
            | "--max-operations"
            | "--max-collection-items"
            | "--max-string-bytes"
            | "--max-output-bytes"
            | "--cwd"
            | "--project-root"
            | "--manifest" => position += 2,
            "--json" => position += 1,
            value => return Some(value),
        }
    }
    None
}

fn run_wait_events(arguments: &[String]) -> i32 {
    let Some(epoch) = option_value(arguments, "--epoch") else {
        eprintln!("wait-events requires --epoch");
        return 2;
    };
    let Some(mut after) =
        option_value(arguments, "--after").and_then(|value| value.parse::<u64>().ok())
    else {
        eprintln!("wait-events requires a numeric --after sequence");
        return 2;
    };
    let Some(kind) = option_value(arguments, "--kind") else {
        eprintln!("wait-events requires --kind");
        return 2;
    };
    let tab_id = option_value(arguments, "--tab")
        .map(|value| value.trim_start_matches('@'))
        .and_then(|value| value.parse::<u64>().ok());
    if option_value(arguments, "--tab").is_some() && tab_id.is_none() {
        eprintln!("wait-events --tab must be a stable @ID");
        return 2;
    }
    let timeout_ms = option_value(arguments, "--timeout-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000)
        .min(60_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let response = send_ipc_request(vec![
            "read-events".to_owned(),
            "--epoch".to_owned(),
            epoch.to_owned(),
            "--after".to_owned(),
            after.to_string(),
            "--limit".to_owned(),
            "256".to_owned(),
        ]);
        let response = match response {
            Ok(response) if response.ok => response,
            Ok(response) => {
                eprintln!("{}", response.error);
                return 1;
            }
            Err(error) => {
                eprintln!("{error:#}");
                return 1;
            }
        };
        let Ok(batch) = serde_json::from_str::<serde_json::Value>(&response.output) else {
            eprintln!("wait-events received an invalid event batch");
            return 1;
        };
        if let Some(events) = batch["events"].as_array() {
            for event in events {
                if let Some(sequence) = event["sequence"].as_u64() {
                    after = after.max(sequence);
                }
                let kind_matches = event["kind"].as_str() == Some(kind);
                let tab_matches =
                    tab_id.is_none_or(|requested| event["tab_id"].as_u64() == Some(requested));
                if kind_matches && tab_matches {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(event).unwrap_or_else(|_| event.to_string())
                    );
                    return 0;
                }
            }
        }
        if Instant::now() >= deadline {
            eprintln!(
                "{}",
                serde_json::json!({
                    "code": "event_wait_timeout",
                    "epoch": epoch,
                    "after": after,
                    "kind": kind,
                    "tab_id": tab_id,
                    "timeout_ms": timeout_ms,
                })
            );
            return 1;
        }
        thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn resolve_stable_wait_target(selector: &str) -> Result<String> {
    let response = send_ipc_request(vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        selector.to_owned(),
        "#{window_id}".to_owned(),
    ])?;
    if !response.ok || !response.output.trim().starts_with('@') {
        anyhow::bail!(
            "wait target could not be resolved to a stable tab ID: {}",
            response.error
        );
    }
    Ok(response.output.trim().to_owned())
}

fn fetch_ui_wait_snapshot() -> Result<(String, serde_json::Value)> {
    let response = send_ipc_request(vec!["ui-snapshot".to_owned()])?;
    if !response.ok {
        anyhow::bail!("{}", response.error);
    }
    let snapshot = serde_json::from_str::<serde_json::Value>(&response.output)
        .context("wait-ui received an invalid UI snapshot")?;
    Ok((response.output, snapshot))
}

fn parse_terminal_grid(value: &str) -> Option<(u64, u64)> {
    let (rows, columns) = value.split_once(['x', 'X'])?;
    let rows = rows.parse::<u64>().ok().filter(|value| *value > 0)?;
    let columns = columns.parse::<u64>().ok().filter(|value| *value > 0)?;
    Some((rows, columns))
}

pub(crate) fn run_wait_ui(arguments: &[String]) -> i32 {
    let timeout_ms = match option_value(arguments, "--timeout-ms") {
        Some(value) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("wait-ui --timeout-ms must be a non-negative integer");
                return 2;
            }
        },
        None => 10_000,
    };
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    let requested_active = option_value(arguments, "--active");
    let expected_focus = option_value(arguments, "--focus");
    let expected_state = option_value(arguments, "--tab-state");
    let expected_proxy_state = option_value(arguments, "--proxy-state");
    let expected_window_state = option_value(arguments, "--window-state");
    let expected_modal_kind = option_value(arguments, "--modal-kind");
    let requested_modal_target = option_value(arguments, "--modal-target");
    let expected_tab_editor_state = option_value(arguments, "--tab-editor-state");
    let expected_client_width =
        option_value(arguments, "--client-width").and_then(|value| value.parse::<i64>().ok());
    let expected_client_height =
        option_value(arguments, "--client-height").and_then(|value| value.parse::<i64>().ok());
    let terminal_grid_changed_from =
        match option_value(arguments, "--terminal-grid-changed-from").map(parse_terminal_grid) {
            Some(Some(value)) => Some(value),
            Some(None) => {
                eprintln!(
                    "wait-ui --terminal-grid-changed-from requires positive ROWSxCOLS dimensions"
                );
                return 2;
            }
            None => None,
        };
    let requested_target = option_value(arguments, "-t");
    if expected_proxy_state.is_some() && requested_target.is_none() {
        eprintln!("wait-ui --proxy-state requires -t target");
        return 2;
    }
    if expected_tab_editor_state.is_some() && requested_target.is_none() {
        eprintln!("wait-ui --tab-editor-state requires -t target");
        return 2;
    }
    if expected_tab_editor_state.is_some_and(|state| !matches!(state, "open" | "closed")) {
        eprintln!("wait-ui --tab-editor-state must be open or closed");
        return 2;
    }
    if expected_modal_kind.is_some_and(|kind| matches!(kind, "none" | "closed"))
        && requested_modal_target.is_some()
    {
        eprintln!("wait-ui --modal-target cannot be combined with --modal-kind none or closed");
        return 2;
    }
    if requested_active.is_none()
        && expected_focus.is_none()
        && expected_state.is_none()
        && expected_proxy_state.is_none()
        && expected_window_state.is_none()
        && expected_client_width.is_none()
        && expected_client_height.is_none()
        && terminal_grid_changed_from.is_none()
        && expected_modal_kind.is_none()
        && requested_modal_target.is_none()
        && expected_tab_editor_state.is_none()
    {
        eprintln!(
            "wait-ui requires an active, focus, tab-state, proxy-state, window-state, client-size, \
             terminal-grid change, modal-kind, modal-target, or tab-editor-state condition"
        );
        return 1;
    }
    let expected_active = match requested_active.map(resolve_stable_wait_target).transpose() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let target = match requested_target.map(resolve_stable_wait_target).transpose() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let expected_modal_target = match requested_modal_target
        .map(resolve_stable_wait_target)
        .transpose()
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let (baseline_output, baseline_snapshot) = match fetch_ui_wait_snapshot() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let baseline_position = baseline_snapshot["event_position"].clone();
    let baseline_epoch = baseline_snapshot["event_position"]["epoch"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if baseline_epoch.is_empty() {
        eprintln!("wait-ui baseline did not contain a server epoch");
        return 1;
    }
    let mut pending_snapshot = Some((baseline_output, baseline_snapshot));
    loop {
        match pending_snapshot
            .take()
            .map(Ok)
            .unwrap_or_else(fetch_ui_wait_snapshot)
        {
            Ok((output, snapshot)) => {
                let current_epoch = snapshot["event_position"]["epoch"]
                    .as_str()
                    .unwrap_or_default();
                if current_epoch != baseline_epoch {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "server_restart",
                            "expected_epoch": baseline_epoch,
                            "current_epoch": current_epoch,
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
                let active_matches = expected_active.as_deref().is_none_or(|expected| {
                    snapshot["focus"]["window_id"].as_str() == Some(expected)
                });
                let focus_matches = expected_focus
                    .is_none_or(|expected| snapshot["focus"]["surface"].as_str() == Some(expected));
                let state_matches = expected_state.is_none_or(|expected| {
                    snapshot["tabs"].as_array().is_some_and(|tabs| {
                        tabs.iter().any(|tab| {
                            let target_matches = target
                                .as_deref()
                                .is_none_or(|selector| tab["id"].as_str() == Some(selector));
                            target_matches && tab["state"].as_str() == Some(expected)
                        })
                    })
                });
                let proxy_state_matches = expected_proxy_state.is_none_or(|expected| {
                    snapshot["tabs"].as_array().is_some_and(|tabs| {
                        tabs.iter().any(|tab| {
                            let target_matches = target
                                .as_deref()
                                .is_some_and(|selector| tab["id"].as_str() == Some(selector));
                            target_matches
                                && tab["working_context"]["proxy"]["application_state"].as_str()
                                    == Some(expected)
                        })
                    })
                });
                let window_state_matches = expected_window_state
                    .is_none_or(|expected| snapshot["window"]["state"].as_str() == Some(expected));
                let width_matches = expected_client_width.is_none_or(|expected| {
                    snapshot["window"]["client_width"].as_i64() == Some(expected)
                });
                let height_matches = expected_client_height.is_none_or(|expected| {
                    snapshot["window"]["client_height"].as_i64() == Some(expected)
                });
                let terminal_grid_matches =
                    terminal_grid_changed_from.is_none_or(|(previous_rows, previous_columns)| {
                        let rows = snapshot["layout"]["terminal"]["rows"].as_u64();
                        let columns = snapshot["layout"]["terminal"]["cols"].as_u64();
                        rows.zip(columns)
                            .is_some_and(|current| current != (previous_rows, previous_columns))
                    });
                let modal_matches = snapshot_modal_matches(
                    &snapshot,
                    expected_modal_kind,
                    expected_modal_target.as_deref(),
                );
                let tab_editor_matches = expected_tab_editor_state.is_none_or(|state| {
                    let editor = &snapshot["tab_editor"];
                    let replaceable_projection =
                        snapshot["projection"].as_str() == Some("replaceable_ui_client");
                    let target_present = target.as_deref().is_some_and(|expected| {
                        snapshot["tabs"].as_array().is_some_and(|tabs| {
                            tabs.iter().any(|tab| tab["id"].as_str() == Some(expected))
                        })
                    });
                    replaceable_projection
                        && target_present
                        && match state {
                            "open" => {
                                editor.is_object() && editor["target"].as_str() == target.as_deref()
                            }
                            "closed" => {
                                editor.is_null() || editor["target"].as_str() != target.as_deref()
                            }
                            _ => false,
                        }
                });
                if active_matches
                    && focus_matches
                    && state_matches
                    && proxy_state_matches
                    && window_state_matches
                    && width_matches
                    && height_matches
                    && terminal_grid_matches
                    && modal_matches
                    && tab_editor_matches
                {
                    println!("{output}");
                    return 0;
                }
                let resolved_target_closed = target.as_deref().is_some_and(|resolved| {
                    !snapshot["tabs"].as_array().is_some_and(|tabs| {
                        tabs.iter().any(|tab| tab["id"].as_str() == Some(resolved))
                    })
                });
                if resolved_target_closed {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "ui_wait_target_closed",
                            "target": target,
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "ui_wait_timeout",
                            "timeout_ms": timeout_ms,
                            "expected": {
                                "active": expected_active,
                                "focus": expected_focus,
                                "tab_state": expected_state,
                                "proxy_state": expected_proxy_state,
                                "target": target,
                                "window_state": expected_window_state,
                                "client_width": expected_client_width,
                                "client_height": expected_client_height,
                                "modal_kind": expected_modal_kind,
                                "modal_target": expected_modal_target,
                                "tab_editor_state": expected_tab_editor_state,
                            },
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
            }
            Err(error) => {
                let last_transport_error = format!("{error:#}");
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "ui_wait_transport_timeout",
                            "timeout_ms": timeout_ms,
                            "baseline_position": baseline_position,
                            "last_transport_error": last_transport_error,
                        })
                    );
                    return 1;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_wait_pane(arguments: &[String]) -> i32 {
    let requested_target = option_value(arguments, "-t").map(str::to_owned);
    let timeout_ms = option_value(arguments, "--timeout-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000);
    let contains = option_value(arguments, "--contains")
        .map(str::to_owned)
        .or_else(|| {
            (arguments
                .first()
                .is_some_and(|value| value == "expect-pane"))
            .then(|| last_positional(arguments, &["-t", "--timeout-ms"]))
            .flatten()
            .map(str::to_owned)
        });
    let wait_dead = arguments.iter().any(|argument| argument == "--dead");
    let wait_submit = arguments
        .iter()
        .any(|argument| argument == "--submit-complete");
    let wait_finalized = arguments.iter().any(|argument| argument == "--finalized");
    if contains.is_none() && !wait_dead && !wait_submit && !wait_finalized {
        eprintln!(
            "usage: wait-pane [-t target] \
             (--contains text | --dead | --submit-complete | --finalized) \
             [--timeout-ms ms]"
        );
        return 2;
    }
    let mut resolve_request = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "#{window_id}".to_owned(),
    ];
    if let Some(target) = &requested_target {
        resolve_request.extend(["-t".to_owned(), target.clone()]);
    }
    let target = match send_ipc_request(resolve_request) {
        Ok(response) if response.ok && response.output.trim().starts_with('@') => {
            response.output.trim().to_owned()
        }
        Ok(response) => {
            eprintln!("{}", response.error);
            return 1;
        }
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let mut matched = true;
        if wait_dead {
            let mut request = vec![
                "display-message".to_owned(),
                "-p".to_owned(),
                "#{pane_dead}".to_owned(),
            ];
            request.extend(["-t".to_owned(), target.clone()]);
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.trim() == "1");
        }
        if let Some(needle) = &contains {
            let mut request = vec!["capture-pane".to_owned(), "-p".to_owned()];
            request.extend(["-t".to_owned(), target.clone()]);
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.contains(needle));
        }
        if wait_submit || wait_finalized {
            let request = vec!["inspect".to_owned(), "-t".to_owned(), target.clone()];
            matched &= send_ipc_request(request).is_ok_and(|response| {
                response.ok
                    && serde_json::from_str::<serde_json::Value>(&response.output)
                        .ok()
                        .and_then(|snapshot| snapshot["windows"].as_array().cloned())
                        .is_some_and(|windows| {
                            !windows.is_empty()
                                && windows.iter().all(|window| {
                                    (!wait_submit
                                        || !window["submit_pending"].as_bool().unwrap_or(true))
                                        && (!wait_finalized
                                            || window["finalized"].as_bool().unwrap_or(false))
                                })
                        })
            });
        }
        if matched {
            return 0;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!(
                "{}",
                serde_json::json!({
                    "code": "pane_wait_timeout",
                    "timeout_ms": timeout_ms,
                    "target": target,
                    "conditions": {
                        "contains": contains,
                        "dead": wait_dead,
                        "submit_complete": wait_submit,
                        "finalized": wait_finalized,
                    }
                })
            );
            return 1;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
fn start_server_process() -> Result<()> {
    let current =
        env::current_exe().context("could not locate the running agenterm-cli executable")?;
    let server = current.with_file_name("agenterm-server.exe");
    if !server.is_file() {
        anyhow::bail!(
            "AgenTerm server executable was not found beside agenterm-cli: {}",
            server.display()
        );
    }
    let server = wide(&server.to_string_lossy());
    let operation = wide("open");
    let endpoint = ipc_endpoint()?;
    let parameters = if let Some(address) = endpoint.legacy_address() {
        wide(&format!("--address {address}"))
    } else {
        wide(&format!("--endpoint {}", endpoint))
    };
    let launched = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            server.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_HIDE,
        )
    } as isize;
    if launched <= 32 {
        anyhow::bail!("failed to launch AgenTerm server through Windows Shell ({launched})");
    }
    Ok(())
}

fn print_help() {
    println!(
        "\
AgenTerm CLI - control the native tabbed terminal

Usage:
  agenterm-cli [--endpoint ENDPOINT|--address HOST:PORT|--instance NAME] [--request-id ID] [--deadline-ms MS]
                [--receipt-json] command [args...]
  agenterm-cli list-instances [--json] [--prune]
  agenterm-cli server-list [--json] [--prune]
  agenterm-cli control-center open|status|snapshot|close [--no-activate]
  agenterm-cli new-session [-s name]
  agenterm-cli new-window [-d] [-n name] [--parent target] [-F format] [-e NAME=VALUE] [command [args...]]
  agenterm-cli new-agent [-d] [-n name] [--parent target] [--proxy URL] [--yolo] [-- [codex args...]]
  agenterm-cli list-windows [-F format]
  agenterm-cli list-tab-tree [-F format]
  agenterm-cli select-window -t target
  agenterm-cli rename-window [-t target] name
  agenterm-cli kill-window -t target
  agenterm-cli send-keys [-t target] key...
  agenterm-cli scroll-pane [-t target] up|down|page-up|page-down|top|bottom [rows]
  agenterm-cli read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]
  agenterm-cli wait-events --epoch EPOCH --after SEQUENCE --kind KIND [--tab @ID] [--timeout-ms MS]
  agenterm-cli capture-pane -p [-t target]
  agenterm-cli capture-pane --raw-escaped [-t target]
  agenterm-cli dump-cells [-t target] [-r row]
  agenterm-cli active-window [-F format]
  agenterm-cli inspect [-t target]
  agenterm-cli screenshot [-o file.png]
  agenterm-cli screenshot-pane [-t target] [-o file.png]
  agenterm-cli show-composer [-t target]
  agenterm-cli set-composer [-t target] text
  agenterm-cli set-composer [-t target] --stdin|--file path
  agenterm-cli send-composer [-t target]
  agenterm-cli set-tab-note [-t target] text
  agenterm-cli show-tab-note [-t target]
  agenterm-cli set-tab-parent -t child --parent parent|root
  agenterm-cli show-tab-parent [-t target]
  agenterm-cli save-workspace
  agenterm-cli workspace-info
  agenterm-cli shutdown
  agenterm-cli get-settings
  agenterm-cli set-setting terminal.font-family FAMILY
  agenterm-cli set-setting terminal.font-size 8..36
  agenterm-cli script api [MODULE] [--status shipped|planned|all] [--tree|--json]
  agenterm-cli script check|run FILE|- [--project-root DIR] [--cwd DIR]
  agenterm-cli script eval EXPRESSION [--cwd DIR]
  agenterm-cli script task list|show|check|run [TASK] [--manifest FILE] [--json] [-- ARGS...]
  agenterm-cli send-mouse [-t target] -x col -y row [--button left] [--action press]
  agenterm-cli ui-snapshot
  agenterm-cli ui-hello --minimum VERSION --maximum VERSION [--client-id ID]
  agenterm-cli ui-bootstrap
  agenterm-cli ui-deltas --epoch EPOCH --after SEQUENCE [--limit 1..64]
  agenterm-cli ui-action new-tab|new-child|edit-tab|toggle-tree|tabs-show|tabs-hide|tabs-toggle|toggle-tabs|tabs-set-width|select-tab|close-tab|close-window|keep-server-running|stop-server-and-exit|confirm|cancel|composer-send|copy-selection|open-settings|open-control-center|settings-theme-dark|settings-theme-light|settings-apply|open-cwd-editor|cwd-prepare|cwd-prepare-append|cwd-prepare-replace|cwd-send-now
  agenterm-cli ui-action tabs-set-width --width 180..480
  agenterm-cli focus terminal|composer|tabs [-t target]
  agenterm-cli wait-pane [-t target] (--contains text|--dead|--submit-complete) [--timeout-ms ms]
  agenterm-cli wait-ui [--active @id] [--focus surface] [-t target --tab-state state|--proxy-state state|--tab-editor-state open|closed] [--client-width PX --client-height PX] [--terminal-grid-changed-from ROWSxCOLS] [--modal-kind KIND|none|closed] [--modal-target target]
  agenterm-cli protocol-info
  agenterm-cli list-panes [-F format]
  agenterm-cli list-sessions | has-session | kill-server | server-kill"
    );
}

fn print_mux_help() {
    println!(
        "\
agenterm-mux - tmux/RMUX compatibility frontend for AgenTerm

Usage:
  agenterm-mux [--endpoint ENDPOINT|--address HOST:PORT|--instance NAME] [--session NAME] COMMAND [ARGS...]
  agenterm-mux compatibility [--json]
  agenterm-mux agenterm COMMAND [ARGS...]

The GUI remains the only server and PTY owner. One AgenTerm tab maps to one
window and one pane. Unsupported tmux operations fail explicitly. Native
AgenTerm commands are available only through the `agenterm` namespace."
    );
}

fn print_mux_commands() {
    for command in MUX_COMMANDS {
        match command.status {
            MuxStatus::Supported => println!("{}", command.name),
            MuxStatus::Unsupported(reason) => {
                println!("{} (unsupported: {reason})", command.name);
            }
        }
    }
}

pub(crate) fn protocol_info_json(identity_scope: &str) -> String {
    protocol_info_json_with_ui_bridge(identity_scope, ui_bridge::headless_server_facts())
}

pub(crate) fn protocol_info_json_with_ui_bridge(
    identity_scope: &str,
    ui_bridge_facts: ui_bridge::UiBridgeFacts,
) -> String {
    let build_identity = BuildIdentity::current();
    let resolved = resolved_ipc_endpoint().ok();
    serde_json::to_string_pretty(&serde_json::json!({
        "protocol_version": 1,
        "agenterm_version": env!("CARGO_PKG_VERSION"),
        "identity_scope": identity_scope,
        "pid": std::process::id(),
        "address": ipc_address(),
        "endpoint": resolved.as_ref().map(|value| value.endpoint.to_string()),
        "transport": resolved.as_ref().map(|value| value.endpoint.transport_name()),
        "instance": resolved.as_ref().map(|value| value.logical_instance.to_string()),
        "server_scope_id": resolved.as_ref().map(|value| value.server_scope_id.to_string()),
        "settings_path": crate::settings::config_path().display().to_string(),
        "build_identity": build_identity,
        "build_identity_complete": build_identity.is_complete(),
        "upgrade_identity": current_upgrade_identity(),
        "platform": crate::platform::platform_info_json(),
        "ui_bridge": ui_bridge_facts,
        "control_contract": {
            "schema_version": crate::control_contract::CONTROL_CONTRACT_SCHEMA_VERSION,
            "request_dedupe": true,
            "deadline_guard": true,
            "receipt_outcomes": ["committed", "accepted", "no_op", "outcome_unknown"],
        },
        "command_catalog": {
            "schema_version": COMMAND_CATALOG_SCHEMA_VERSION,
            "commands": COMMAND_CATALOG,
        },
        "operation_catalog": {
            "schema_version": OPERATION_CATALOG_SCHEMA_VERSION,
            "classification_only": true,
            "authorization_policy": false,
            "operations": OPERATION_CATALOG,
        },
        "event_catalog": {
            "schema_version": EVENT_CATALOG_SCHEMA_VERSION,
            "events": EVENT_CATALOG,
        },
        "compatibility": {
            "tmux_rmux": [
                "new-session", "list-sessions", "has-session",
                "new-window", "list-windows", "select-window",
                "next-window", "previous-window", "rename-window",
                "kill-window", "list-panes", "capture-pane",
                "send-keys", "display-message", "show-options"
            ],
            "partial": ["kill-session", "kill-server"],
            "planned": ["split-window", "layouts"]
        },
        "extensions": [
            "ui-snapshot", "ui-action", "focus", "protocol-info", "control-center",
            "inspect", "screenshot", "screenshot-pane", "dump-cells",
            "wait-pane", "send-mouse", "show-composer",
            "set-composer", "send-composer", "get-settings",
            "set-setting", "set-tab-note", "show-tab-note",
            "list-tab-tree", "set-tab-parent", "show-tab-parent",
            "save-workspace", "workspace-info", "shutdown",
            "new-agent", "list-instances", "server-list", "server-kill", "scroll-pane", "read-events",
            "wait-events"
        ],
        "features": {
            "remain_on_exit": true,
            "live_close_confirmation": true,
            "semantic_ui_automation": true,
            "hierarchical_tabs": true,
            "persistent_workspace": true,
            "tab_environment": true,
            "codex_launcher": true,
            "mux_frontend": true,
            "instance_discovery": true,
            "typed_operations": true,
            "typed_events": true
        }
    }))
    .unwrap_or_default()
}

fn print_mux_compatibility(json: bool) {
    let supported = MUX_COMMANDS
        .iter()
        .filter(|command| command.status == MuxStatus::Supported)
        .map(|command| command.name)
        .collect::<Vec<_>>();
    let unsupported = MUX_COMMANDS
        .iter()
        .filter_map(|command| match command.status {
            MuxStatus::Supported => None,
            MuxStatus::Unsupported(reason) => Some(serde_json::json!({
                "name": command.name,
                "reason": reason,
            })),
        })
        .collect::<Vec<_>>();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "frontend": "agenterm-mux",
                "agenterm_version": env!("CARGO_PKG_VERSION"),
                "model": {
                    "server": "agenterm.exe",
                    "session": "workspace",
                    "window": "tab",
                    "pane": "single-pane-tab"
                },
                "differences": {
                    "workspace_persistence": "normal GUI shutdown saves tabs and restarts their commands on restore",
                    "process_ownership": "agenterm.exe owns every ConPTY child",
                    "live_close": "GUI close actions require confirmation; explicit CLI kill commands are authoritative",
                    "server_lifetime": "kill-server intentionally clears the saved workspace",
                    "split_panes": false
                },
                "supported": supported,
                "explicitly_unsupported": unsupported,
                "native_namespace": "agenterm",
            }))
            .unwrap_or_default()
        );
    } else {
        println!("agenterm-mux {}", env!("CARGO_PKG_VERSION"));
        println!("sessions=workspaces windows=tabs panes=single-pane-tabs");
        println!("supported: {}", supported.join(", "));
        for entry in unsupported {
            println!(
                "unsupported: {} ({})",
                entry["name"].as_str().unwrap_or_default(),
                entry["reason"].as_str().unwrap_or_default()
            );
        }
        println!("native AgenTerm extensions: agenterm-mux agenterm COMMAND ...");
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_loopback_ipc_address, parse_terminal_grid, run_wait_ui};

    #[test]
    fn accepts_only_loopback_ipc_addresses() {
        assert!(parse_loopback_ipc_address("127.0.0.1:42000").is_ok());
        assert!(parse_loopback_ipc_address("[::1]:42000").is_ok());
        assert!(parse_loopback_ipc_address("0.0.0.0:42000").is_err());
        assert!(parse_loopback_ipc_address("192.0.2.1:42000").is_err());
        assert!(parse_loopback_ipc_address("127.0.0.1:42\0").is_err());
    }

    #[test]
    fn wait_ui_rejects_closed_modal_with_a_target_without_polling() {
        let arguments = vec![
            "wait-ui".to_owned(),
            "--modal-kind".to_owned(),
            "closed".to_owned(),
            "--modal-target".to_owned(),
            "@1".to_owned(),
        ];
        assert_eq!(run_wait_ui(&arguments), 2);
    }

    #[test]
    fn terminal_grid_wait_dimension_is_positive_and_exact() {
        assert_eq!(parse_terminal_grid("24x80"), Some((24, 80)));
        assert_eq!(parse_terminal_grid("24X80"), Some((24, 80)));
        assert_eq!(parse_terminal_grid("0x80"), None);
        assert_eq!(parse_terminal_grid("24x"), None);
        assert_eq!(parse_terminal_grid("24x80x2"), None);
    }
}
