use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use rhai::{Array, Engine, EvalAltResult, Module};

use crate::{
    script_error::runtime_error,
    script_stdlib::{ScriptBytes, ScriptPath},
    script_stream::{
        CapturedStream, ScriptStream, cancel as cancel_stream, capture_after_close,
        discard_buffered, from_process_stderr, from_process_stdout, from_reader,
        mark_process_exited,
    },
};

const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(1);
const DEFAULT_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_STDIN_BYTES: usize = 4 * 1024 * 1024;

// Script Runtime owns policy and receipts; the facade owns native process
// tree mechanics (Job Objects / process groups).
use crate::platform::process as child_process_tree;

#[derive(Clone, Copy, Debug)]
pub struct ScriptDuration(pub(crate) Duration);

#[derive(Clone, Debug)]
pub struct ScriptCommand {
    program: String,
    arguments: Vec<String>,
    current_dir: Option<PathBuf>,
    environment: BTreeMap<String, Option<String>>,
    clear_environment: bool,
    stdin: Vec<u8>,
    stdout_file: Option<PathBuf>,
    stderr_file: Option<PathBuf>,
    timeout: Duration,
    capture_bytes: usize,
}

#[derive(Clone)]
pub struct ScriptChild(Arc<Mutex<ChildState>>);

#[derive(Clone, Debug)]
pub struct ScriptProcessPlatformFacts {
    top_level_window_supported: bool,
    top_level_window_present: bool,
    top_level_window_id: i64,
    top_level_window_title: String,
    foreground_window_id: i64,
    top_level_window_is_foreground: bool,
}

#[derive(Clone, Debug)]
pub struct ScriptWindowRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

#[derive(Clone, Debug)]
pub struct ScriptWindowControl {
    child: ScriptChild,
    id: i32,
}

#[derive(Clone, Debug)]
pub struct ScriptProcessInfo {
    id: u32,
    parent_id: u32,
    executable_name: String,
}

impl std::fmt::Debug for ScriptChild {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Child").finish_non_exhaustive()
    }
}

struct ChildState {
    id: u32,
    child: Option<Child>,
    process_tree: child_process_tree::ProcessTreeGuard,
    stdout: ScriptStream,
    stderr: ScriptStream,
    deadline: Instant,
    completed: Option<ScriptOutput>,
}

impl Drop for ChildState {
    fn drop(&mut self) {
        let _ = self.process_tree.terminate();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        cancel_stream(&self.stdout);
        cancel_stream(&self.stderr);
    }
}

#[derive(Clone, Debug)]
pub struct ScriptOutput {
    success: bool,
    exit_code: i64,
    stdout: ScriptBytes,
    stderr: ScriptBytes,
    complete: bool,
    truncated: bool,
}

pub(crate) fn register(engine: &mut Engine, std_module: &mut Module, time: &mut Module) {
    register_types(engine);
    std_module.set_sub_module("env", env_module());
    std_module.set_sub_module("process", process_module());
    time.set_sub_module("Duration", duration_module());
}

fn register_types(engine: &mut Engine) {
    engine.register_type_with_name::<ScriptDuration>("Duration");
    engine.register_get("millis", duration_millis);

    engine.register_type_with_name::<ScriptCommand>("Command");
    engine.register_fn("arg", command_arg);
    engine.register_fn("args", command_args);
    engine.register_fn("current_dir", command_current_dir_text);
    engine.register_fn("current_dir", command_current_dir_path);
    engine.register_fn("env", command_env);
    engine.register_fn("env_remove", command_env_remove);
    engine.register_fn("env_clear", |command: &mut ScriptCommand| {
        command.clear_environment = true;
    });
    engine.register_fn("stdin_text", command_stdin_text);
    engine.register_fn("stdin_bytes", command_stdin_bytes);
    engine.register_fn("stdout_file", command_stdout_file);
    engine.register_fn("stderr_file", command_stderr_file);
    engine.register_fn(
        "timeout",
        |command: &mut ScriptCommand, value: ScriptDuration| {
            command.timeout = value.0;
        },
    );
    engine.register_fn("capture_limit", command_capture_limit);
    engine.register_fn("output", command_output);
    engine.register_fn("start", command_start);

    engine.register_type_with_name::<ScriptChild>("Child");
    engine.register_get("id", child_id);
    engine.register_get("state", child_state);
    engine.register_get("platform_facts", child_platform_facts);
    engine.register_get("stdout", child_stdout);
    engine.register_get("stderr", child_stderr);
    engine.register_fn("kill", child_kill);
    engine.register_fn("kill_tree", child_kill_tree);
    engine.register_fn("window_key", child_window_key);
    engine.register_fn("window_pointer", child_window_pointer);
    engine.register_fn(
        "window_pointer_coordinate_scale",
        child_window_pointer_coordinate_scale,
    );
    engine.register_fn("window_message", child_window_message);
    engine.register_fn("window_rect", child_window_rect);
    engine.register_fn("window_client_rect", child_window_client_rect);
    engine.register_fn("window_resize", child_window_resize);
    engine.register_fn("window_control", child_window_control);
    engine.register_fn("wait_with_output", child_wait_with_output);
    engine.register_fn("wait_with_output", child_wait_with_output_for);

    engine.register_type_with_name::<ScriptProcessPlatformFacts>("ProcessPlatformFacts");
    engine.register_get(
        "top_level_window_supported",
        |facts: &mut ScriptProcessPlatformFacts| facts.top_level_window_supported,
    );
    engine.register_get(
        "top_level_window_present",
        |facts: &mut ScriptProcessPlatformFacts| facts.top_level_window_present,
    );
    engine.register_get(
        "top_level_window_id",
        |facts: &mut ScriptProcessPlatformFacts| facts.top_level_window_id,
    );
    engine.register_get(
        "top_level_window_title",
        |facts: &mut ScriptProcessPlatformFacts| facts.top_level_window_title.clone(),
    );
    engine.register_get(
        "foreground_window_id",
        |facts: &mut ScriptProcessPlatformFacts| facts.foreground_window_id,
    );
    engine.register_get(
        "top_level_window_is_foreground",
        |facts: &mut ScriptProcessPlatformFacts| facts.top_level_window_is_foreground,
    );

    engine.register_type_with_name::<ScriptWindowRect>("WindowRect");
    engine.register_get("left", |rect: &mut ScriptWindowRect| rect.left);
    engine.register_get("top", |rect: &mut ScriptWindowRect| rect.top);
    engine.register_get("right", |rect: &mut ScriptWindowRect| rect.right);
    engine.register_get("bottom", |rect: &mut ScriptWindowRect| rect.bottom);
    engine.register_get("width", |rect: &mut ScriptWindowRect| {
        rect.right - rect.left
    });
    engine.register_get("height", |rect: &mut ScriptWindowRect| {
        rect.bottom - rect.top
    });

    engine.register_type_with_name::<ScriptWindowControl>("WindowControl");
    engine.register_get("id", |control: &mut ScriptWindowControl| {
        rhai::INT::from(control.id)
    });
    engine.register_get("visible", window_control_visible);
    engine.register_get("text", window_control_text);
    engine.register_fn("set_text", window_control_set_text);
    engine.register_fn("click", window_control_click);

    engine.register_type_with_name::<ScriptProcessInfo>("ProcessInfo");
    engine.register_get("id", |process: &mut ScriptProcessInfo| {
        rhai::INT::from(process.id)
    });
    engine.register_get("parent_id", |process: &mut ScriptProcessInfo| {
        rhai::INT::from(process.parent_id)
    });
    engine.register_get("executable_name", |process: &mut ScriptProcessInfo| {
        process.executable_name.clone()
    });

    engine.register_type_with_name::<ScriptOutput>("Output");
    engine.register_get("success", |output: &mut ScriptOutput| output.success);
    engine.register_get("exit_code", |output: &mut ScriptOutput| output.exit_code);
    engine.register_get("stdout", |output: &mut ScriptOutput| output.stdout.clone());
    engine.register_get("stderr", |output: &mut ScriptOutput| output.stderr.clone());
    engine.register_get("complete", |output: &mut ScriptOutput| output.complete);
    engine.register_get("truncated", |output: &mut ScriptOutput| output.truncated);
    engine.register_fn("stdout_text", |output: &mut ScriptOutput| {
        output_text(&output.stdout, "process_stdout_not_utf8")
    });
    engine.register_fn("stderr_text", |output: &mut ScriptOutput| {
        output_text(&output.stderr, "process_stderr_not_utf8")
    });
    engine.register_fn("combined_text", |output: &mut ScriptOutput| {
        let stdout = output_text(&output.stdout, "process_stdout_not_utf8")?;
        let stderr = output_text(&output.stderr, "process_stderr_not_utf8")?;
        Ok::<String, Box<EvalAltResult>>(stdout + stderr.as_str())
    });
    engine.register_fn("error", output_error);
    engine.register_fn("require_success", output_require_success);
}

fn env_module() -> Module {
    let mut module = Module::new();
    module.set_native_fn("get", env_get);
    module.set_native_fn("has", env_has);
    module.set_native_fn("names", env_names);
    module.set_native_fn("current_dir", env_current_dir);
    module
}

fn process_module() -> Module {
    let mut module = Module::new();
    module.set_native_fn("id", process_id);
    module.set_native_fn("list", process_list);
    module.set_native_fn("kill", process_kill);
    module.set_native_fn("command", process_command);
    module
}

fn process_id() -> Result<rhai::INT, Box<EvalAltResult>> {
    Ok(std::process::id().into())
}

fn process_list() -> Result<Array, Box<EvalAltResult>> {
    let mut processes = platform_process_list()?;
    processes.sort_by_key(|process| process.id);
    Ok(processes.into_iter().map(rhai::Dynamic::from).collect())
}

fn process_kill(id: rhai::INT) -> Result<(), Box<EvalAltResult>> {
    let id = u32::try_from(id).map_err(|_| {
        runtime_error(
            "configuration",
            "process_id_invalid",
            "std.process.kill",
            "process ID must be in the unsigned 32-bit range",
            false,
            "process",
            false,
            Some("integer_range"),
        )
    })?;
    if id == 0 {
        return Err(runtime_error(
            "configuration",
            "process_id_invalid",
            "std.process.kill",
            "process ID must be greater than zero",
            false,
            "process",
            false,
            Some("integer_range"),
        ));
    }
    platform_process_kill(id)
}

fn platform_process_kill(id: u32) -> Result<(), Box<EvalAltResult>> {
    use crate::platform::process::ProcessErrorKind;

    crate::platform::process::kill(id).map_err(|error| {
        let (category, code, message, retryable, remediation) = match error.kind() {
            ProcessErrorKind::IdOutOfRange => (
                "configuration",
                "process_id_invalid",
                "process ID exceeds the platform range",
                false,
                Some("integer_range"),
            ),
            ProcessErrorKind::KillOpen => (
                "host",
                "process_kill_open",
                "unable to open the selected operating-system process",
                false,
                Some("os"),
            ),
            ProcessErrorKind::Unsupported => (
                "host",
                "process_kill_unsupported",
                "operating-system process termination is not supported on this platform",
                false,
                Some("platform"),
            ),
            _ => (
                "host",
                "process_kill",
                "unable to terminate the selected operating-system process",
                false,
                Some("os"),
            ),
        };
        runtime_error(
            category,
            code,
            "std.process.kill",
            message,
            retryable,
            "process",
            false,
            remediation,
        )
    })
}

fn platform_process_list() -> Result<Vec<ScriptProcessInfo>, Box<EvalAltResult>> {
    use crate::platform::process::ProcessErrorKind;

    crate::platform::process::list()
        .map(|processes| {
            processes
                .into_iter()
                .map(|process| ScriptProcessInfo {
                    id: process.id,
                    parent_id: process.parent_id,
                    executable_name: process.executable_name,
                })
                .collect()
        })
        .map_err(|error| {
            let (category, code, message, retryable, remediation) = match error.kind() {
                ProcessErrorKind::InventoryTooLarge => (
                    "limit",
                    "process_list_too_large",
                    "operating-system process inventory exceeds the supported size",
                    false,
                    Some("integer_conversion"),
                ),
                ProcessErrorKind::Unsupported => (
                    "host",
                    "process_list_unsupported",
                    "operating-system process inventory is not supported on this platform",
                    false,
                    Some("platform"),
                ),
                _ => (
                    "host",
                    "process_list_failed",
                    "unable to capture the operating-system process inventory",
                    true,
                    Some("os"),
                ),
            };
            runtime_error(
                category,
                code,
                "std.process.list",
                message,
                retryable,
                "process_inventory",
                false,
                remediation,
            )
        })
}

fn duration_module() -> Module {
    let mut module = Module::new();
    module.set_native_fn("from_millis", duration_from_millis);
    module.set_native_fn("from_secs", duration_from_secs);
    module
}

fn duration_from_millis(value: rhai::INT) -> Result<ScriptDuration, Box<EvalAltResult>> {
    Ok(ScriptDuration(Duration::from_millis(bounded(
        value,
        "duration_millis",
        MAX_TIMEOUT_MS,
    )?)))
}

fn duration_from_secs(value: rhai::INT) -> Result<ScriptDuration, Box<EvalAltResult>> {
    Ok(ScriptDuration(Duration::from_secs(bounded(
        value,
        "duration_seconds",
        MAX_TIMEOUT_MS / 1_000,
    )?)))
}

fn duration_millis(value: &mut ScriptDuration) -> Result<rhai::INT, Box<EvalAltResult>> {
    rhai::INT::try_from(value.0.as_millis())
        .map_err(|_| "duration_overflow: milliseconds exceed Rhai integer".into())
}

fn env_get(name: &str) -> Result<String, Box<EvalAltResult>> {
    validate_env_name(name)?;
    std::env::var(name).map_err(|error| match error {
        std::env::VarError::NotPresent => format!("environment_missing: {name}").into(),
        std::env::VarError::NotUnicode(_) => format!("environment_not_unicode: {name}").into(),
    })
}

fn env_has(name: &str) -> Result<bool, Box<EvalAltResult>> {
    validate_env_name(name)?;
    Ok(std::env::var_os(name).is_some())
}

fn env_names() -> Result<Array, Box<EvalAltResult>> {
    let mut names = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_uppercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    Ok(names.into_iter().map(Into::into).collect())
}

fn env_current_dir() -> Result<ScriptPath, Box<EvalAltResult>> {
    std::env::current_dir()
        .map(ScriptPath)
        .map_err(|error| format!("environment_current_dir: {error}").into())
}

fn process_command(program: &str) -> Result<ScriptCommand, Box<EvalAltResult>> {
    validate_text(program, "process_program")?;
    if program.is_empty() {
        return Err("process_program_empty: program must not be empty".into());
    }
    Ok(ScriptCommand {
        program: program.to_owned(),
        arguments: Vec::new(),
        current_dir: None,
        environment: BTreeMap::new(),
        clear_environment: false,
        stdin: Vec::new(),
        stdout_file: None,
        stderr_file: None,
        timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        capture_bytes: DEFAULT_CAPTURE_BYTES,
    })
}

fn command_arg(command: &mut ScriptCommand, value: &str) -> Result<(), Box<EvalAltResult>> {
    validate_text(value, "process_argument")?;
    command.arguments.push(value.to_owned());
    Ok(())
}

fn command_args(command: &mut ScriptCommand, values: Array) -> Result<(), Box<EvalAltResult>> {
    let mut arguments = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .into_string()
            .map_err(|_| "process_arguments_type: every argument must be a string")?;
        validate_text(&value, "process_argument")?;
        arguments.push(value);
    }
    command.arguments.extend(arguments);
    Ok(())
}

fn command_current_dir_text(
    command: &mut ScriptCommand,
    path: &str,
) -> Result<(), Box<EvalAltResult>> {
    validate_text(path, "process_current_dir")?;
    command.current_dir = Some(PathBuf::from(path));
    Ok(())
}

fn command_current_dir_path(
    command: &mut ScriptCommand,
    path: ScriptPath,
) -> Result<(), Box<EvalAltResult>> {
    command.current_dir = Some(path.0);
    Ok(())
}

fn command_env(
    command: &mut ScriptCommand,
    name: &str,
    value: &str,
) -> Result<(), Box<EvalAltResult>> {
    validate_env_name(name)?;
    validate_text(value, "process_environment_value")?;
    command
        .environment
        .insert(name.to_owned(), Some(value.to_owned()));
    Ok(())
}

fn command_env_remove(command: &mut ScriptCommand, name: &str) -> Result<(), Box<EvalAltResult>> {
    validate_env_name(name)?;
    command.environment.insert(name.to_owned(), None);
    Ok(())
}

fn command_stdin_text(command: &mut ScriptCommand, value: &str) -> Result<(), Box<EvalAltResult>> {
    if value.len() > MAX_STDIN_BYTES {
        return Err(format!("process_stdin_too_large: maximum is {MAX_STDIN_BYTES} bytes").into());
    }
    command.stdin = value.as_bytes().to_vec();
    Ok(())
}

fn command_stdin_bytes(
    command: &mut ScriptCommand,
    value: ScriptBytes,
) -> Result<(), Box<EvalAltResult>> {
    if value.0.len() > MAX_STDIN_BYTES {
        return Err(format!("process_stdin_too_large: maximum is {MAX_STDIN_BYTES} bytes").into());
    }
    command.stdin = value.0;
    Ok(())
}

fn command_stdout_file(command: &mut ScriptCommand, path: &str) -> Result<(), Box<EvalAltResult>> {
    if path.is_empty() {
        return Err("process_stdout_file_empty".into());
    }
    command.stdout_file = Some(PathBuf::from(path));
    Ok(())
}

fn command_stderr_file(command: &mut ScriptCommand, path: &str) -> Result<(), Box<EvalAltResult>> {
    if path.is_empty() {
        return Err("process_stderr_file_empty".into());
    }
    command.stderr_file = Some(PathBuf::from(path));
    Ok(())
}

fn command_capture_limit(
    command: &mut ScriptCommand,
    bytes: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    command.capture_bytes = usize::try_from(bounded(
        bytes,
        "process_capture_bytes",
        MAX_CAPTURE_BYTES as u64,
    )?)
    .map_err(|_| "process_capture_overflow")?;
    Ok(())
}

fn command_output(command: &mut ScriptCommand) -> Result<ScriptOutput, Box<EvalAltResult>> {
    wait_for_child(&spawn_owned(command)?, None)
}

fn command_start(command: &mut ScriptCommand) -> Result<ScriptChild, Box<EvalAltResult>> {
    spawn_owned(command)
}

fn spawn_owned(command: &ScriptCommand) -> Result<ScriptChild, Box<EvalAltResult>> {
    let mut process = Command::new(&command.program);
    process.args(&command.arguments).stdin(Stdio::piped());
    child_process_tree::configure_command(&mut process)
        .map_err(|error| format!("process_tree_configure: {error}"))?;
    if let Some(path) = command.stdout_file.as_ref() {
        let file =
            std::fs::File::create(path).map_err(|error| format!("process_stdout_file: {error}"))?;
        process.stdout(Stdio::from(file));
    } else {
        process.stdout(Stdio::piped());
    }
    if let Some(path) = command.stderr_file.as_ref() {
        let file =
            std::fs::File::create(path).map_err(|error| format!("process_stderr_file: {error}"))?;
        process.stderr(Stdio::from(file));
    } else {
        process.stderr(Stdio::piped());
    }
    if let Some(current_dir) = command.current_dir.as_ref() {
        process.current_dir(current_dir);
    }
    if command.clear_environment {
        process.env_clear();
    }
    for (name, value) in &command.environment {
        if let Some(value) = value {
            process.env(name, value);
        } else {
            process.env_remove(name);
        }
    }
    let mut child = process
        .spawn()
        .map_err(|error| format!("process_spawn: {error}"))?;
    let process_tree = match child_process_tree::ProcessTreeGuard::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("process_tree_attach: {error}").into());
        }
    };
    let stdout = if command.stdout_file.is_some() {
        from_reader(std::io::empty(), "bytes", 0)
    } else {
        from_process_stdout(
            child.stdout.take().ok_or("process_stdout_unavailable")?,
            "bytes",
            command.capture_bytes,
        )
    };
    let stderr = if command.stderr_file.is_some() {
        from_reader(std::io::empty(), "bytes", 0)
    } else {
        from_process_stderr(
            child.stderr.take().ok_or("process_stderr_unavailable")?,
            "bytes",
            command.capture_bytes,
        )
    };
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&command.stdin)
            .map_err(|error| format!("process_stdin_write: {error}"))?;
    }
    let id = child.id();
    Ok(ScriptChild(Arc::new(Mutex::new(ChildState {
        id,
        child: Some(child),
        process_tree,
        stdout,
        stderr,
        deadline: Instant::now() + command.timeout,
        completed: None,
    }))))
}

fn child_id(child: &mut ScriptChild) -> Result<rhai::INT, Box<EvalAltResult>> {
    let state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
    Ok(i64::from(state.id))
}

fn child_platform_facts(
    child: &mut ScriptChild,
) -> Result<ScriptProcessPlatformFacts, Box<EvalAltResult>> {
    let id = child
        .0
        .lock()
        .map_err(|_| "process_child_state_poisoned")?
        .id;
    Ok(process_platform_facts(id))
}

fn process_platform_facts(id: u32) -> ScriptProcessPlatformFacts {
    let facts = crate::platform::services::script_window::facts(id);
    ScriptProcessPlatformFacts {
        top_level_window_supported: facts.supported,
        top_level_window_present: facts.present,
        top_level_window_id: facts.window_id,
        top_level_window_title: facts.title,
        foreground_window_id: facts.foreground_window_id,
        top_level_window_is_foreground: facts.is_foreground,
    }
}

fn child_stdout(child: &mut ScriptChild) -> Result<ScriptStream, Box<EvalAltResult>> {
    Ok(child
        .0
        .lock()
        .map_err(|_| "process_child_state_poisoned")?
        .stdout
        .clone())
}

fn child_stderr(child: &mut ScriptChild) -> Result<ScriptStream, Box<EvalAltResult>> {
    Ok(child
        .0
        .lock()
        .map_err(|_| "process_child_state_poisoned")?
        .stderr
        .clone())
}

fn child_state(child: &mut ScriptChild) -> Result<String, Box<EvalAltResult>> {
    let mut state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
    if state.completed.is_some() {
        return Ok("exited".to_owned());
    }
    let exited = state
        .child
        .as_mut()
        .ok_or("process_child_missing")?
        .try_wait()
        .map_err(|error| format!("process_try_wait: {error}"))?
        .is_some();
    Ok(if exited { "exited" } else { "running" }.to_owned())
}

fn child_kill(child: &mut ScriptChild) -> Result<(), Box<EvalAltResult>> {
    let mut state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
    if let Some(process) = state.child.as_mut() {
        process
            .kill()
            .map_err(|error| format!("process_kill: {error}"))?;
    }
    Ok(())
}

fn child_kill_tree(child: &mut ScriptChild) -> Result<(), Box<EvalAltResult>> {
    let mut state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
    terminate_child_tree(&mut state).map_err(|error| {
        runtime_error(
            "host",
            "process_kill_tree",
            "std.process.Child.kill_tree",
            &error,
            false,
            "child_process_tree",
            false,
            Some("os"),
        )
    })
}

fn terminate_child_tree(state: &mut ChildState) -> Result<(), String> {
    let tree_result = state.process_tree.terminate();
    if let Some(process) = state.child.as_mut() {
        let _ = process.kill();
    }
    tree_result
}

fn child_process_id(child: &ScriptChild) -> Result<u32, Box<EvalAltResult>> {
    Ok(child
        .0
        .lock()
        .map_err(|_| "process_child_state_poisoned")?
        .id)
}

fn child_window_key(child: &mut ScriptChild, key: &str) -> Result<(), Box<EvalAltResult>> {
    use crate::platform::contract::script_window::ScriptWindowKey;
    let key = match key {
        "Backspace" => ScriptWindowKey::Backspace,
        "Delete" => ScriptWindowKey::Delete,
        "Down" => ScriptWindowKey::Down,
        "End" => ScriptWindowKey::End,
        "Enter" => ScriptWindowKey::Enter,
        "Escape" => ScriptWindowKey::Escape,
        "F2" => ScriptWindowKey::F2,
        "Home" => ScriptWindowKey::Home,
        "Left" => ScriptWindowKey::Left,
        "Right" => ScriptWindowKey::Right,
        "Tab" => ScriptWindowKey::Tab,
        "Up" => ScriptWindowKey::Up,
        _ => {
            return Err(process_window_error(
                "process_window_key_invalid",
                "Child.window_key",
                "window key must be Backspace, Delete, Down, End, Enter, Escape, F2, Home, Left, Right, Tab, or Up",
                Some("invalid_input"),
            ));
        }
    };
    window_result(
        "Child.window_key",
        crate::platform::services::script_window::key(child_process_id(child)?, key),
    )
}

fn child_window_pointer(
    child: &mut ScriptChild,
    action: &str,
    x: rhai::INT,
    y: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    use crate::platform::contract::script_window::ScriptWindowPointerAction;

    let x = i32::try_from(x).map_err(|_| {
        process_window_error(
            "process_window_coordinate_invalid",
            "Child.window_pointer",
            "window pointer coordinates must fit signed 32-bit integers",
            Some("invalid_input"),
        )
    })?;
    let y = i32::try_from(y).map_err(|_| {
        process_window_error(
            "process_window_coordinate_invalid",
            "Child.window_pointer",
            "window pointer coordinates must fit signed 32-bit integers",
            Some("invalid_input"),
        )
    })?;
    let action = match action {
        "click" => ScriptWindowPointerAction::Click,
        "down" => ScriptWindowPointerAction::Down,
        "move" => ScriptWindowPointerAction::Move,
        "move-held" => ScriptWindowPointerAction::MoveHeld,
        "up" => ScriptWindowPointerAction::Up,
        "capture-changed" => ScriptWindowPointerAction::CaptureChanged,
        _ => {
            return Err(process_window_error(
                "process_window_pointer_action_invalid",
                "Child.window_pointer",
                "window pointer action must be click, down, move, move-held, up, or capture-changed",
                Some("invalid_input"),
            ));
        }
    };
    window_result(
        "Child.window_pointer",
        crate::platform::services::script_window::pointer(child_process_id(child)?, action, x, y),
    )
}

fn child_window_pointer_coordinate_scale(
    child: &mut ScriptChild,
) -> Result<rhai::FLOAT, Box<EvalAltResult>> {
    window_result(
        "Child.window_pointer_coordinate_scale",
        crate::platform::services::script_window::pointer_coordinate_scale(child_process_id(
            child,
        )?),
    )
}

fn child_window_message(
    child: &mut ScriptChild,
    message: rhai::INT,
    wparam: rhai::INT,
    lparam: rhai::INT,
) -> Result<rhai::INT, Box<EvalAltResult>> {
    use crate::platform::contract::script_window::ScriptWindowMessage;

    let message = u32::try_from(message).map_err(|_| {
        process_window_error(
            "process_window_message_invalid",
            "Child.window_message",
            "window message must fit an unsigned 32-bit integer",
            Some("invalid_input"),
        )
    })?;
    let wparam = usize::try_from(wparam).map_err(|_| {
        process_window_error(
            "process_window_message_parameter_invalid",
            "Child.window_message",
            "window wparam must fit a native unsigned integer",
            Some("invalid_input"),
        )
    })?;
    let lparam = isize::try_from(lparam).map_err(|_| {
        process_window_error(
            "process_window_message_parameter_invalid",
            "Child.window_message",
            "window lparam must fit a native signed integer",
            Some("invalid_input"),
        )
    })?;
    window_result(
        "Child.window_message",
        crate::platform::services::script_window::message(
            child_process_id(child)?,
            ScriptWindowMessage {
                message,
                wparam,
                lparam,
            },
        ),
    )
    .map(|value| value as rhai::INT)
}

fn child_window_rect(child: &mut ScriptChild) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    window_rect_result(
        "Child.window_rect",
        crate::platform::services::script_window::rect(child_process_id(child)?, false),
    )
}

fn child_window_client_rect(
    child: &mut ScriptChild,
) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    window_rect_result(
        "Child.window_client_rect",
        crate::platform::services::script_window::rect(child_process_id(child)?, true),
    )
}

fn child_window_resize(
    child: &mut ScriptChild,
    width: rhai::INT,
    height: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    let width = i32::try_from(width)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            process_window_error(
                "process_window_size_invalid",
                "Child.window_resize",
                "window width must be a positive signed 32-bit integer",
                Some("invalid_input"),
            )
        })?;
    let height = i32::try_from(height)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            process_window_error(
                "process_window_size_invalid",
                "Child.window_resize",
                "window height must be a positive signed 32-bit integer",
                Some("invalid_input"),
            )
        })?;
    window_result(
        "Child.window_resize",
        crate::platform::services::script_window::resize(child_process_id(child)?, width, height),
    )
}

fn child_window_control(
    child: &mut ScriptChild,
    id: rhai::INT,
) -> Result<ScriptWindowControl, Box<EvalAltResult>> {
    let id = i32::try_from(id).map_err(|_| {
        process_window_error(
            "process_window_control_id_invalid",
            "Child.window_control",
            "window control ID must fit a signed 32-bit integer",
            Some("invalid_input"),
        )
    })?;
    let control = ScriptWindowControl {
        child: child.clone(),
        id,
    };
    window_result(
        "Child.window_control",
        crate::platform::services::script_window::control_exists(child_process_id(child)?, id),
    )?;
    Ok(control)
}

fn window_control_visible(control: &mut ScriptWindowControl) -> Result<bool, Box<EvalAltResult>> {
    window_result(
        "WindowControl.visible",
        crate::platform::services::script_window::control_visible(
            child_process_id(&control.child)?,
            control.id,
        ),
    )
}

fn window_control_text(control: &mut ScriptWindowControl) -> Result<String, Box<EvalAltResult>> {
    window_result(
        "WindowControl.text",
        crate::platform::services::script_window::control_text(
            child_process_id(&control.child)?,
            control.id,
        ),
    )
}

fn window_control_set_text(
    control: &mut ScriptWindowControl,
    text: &str,
) -> Result<(), Box<EvalAltResult>> {
    window_result(
        "WindowControl.set_text",
        crate::platform::services::script_window::control_set_text(
            child_process_id(&control.child)?,
            control.id,
            text,
        ),
    )
}

fn window_control_click(control: &mut ScriptWindowControl) -> Result<(), Box<EvalAltResult>> {
    window_result(
        "WindowControl.click",
        crate::platform::services::script_window::control_click(
            child_process_id(&control.child)?,
            control.id,
        ),
    )
}

fn window_result<T>(
    operation: &'static str,
    result: Result<T, crate::platform::contract::script_window::ScriptWindowError>,
) -> Result<T, Box<EvalAltResult>> {
    result.map_err(|error| process_window_error(error.code, operation, error.message, error.cause))
}

fn window_rect_result(
    operation: &'static str,
    result: Result<
        crate::platform::contract::script_window::ScriptWindowRect,
        crate::platform::contract::script_window::ScriptWindowError,
    >,
) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    window_result(operation, result).map(|rect| ScriptWindowRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    })
}

fn process_window_error(
    code: &'static str,
    operation: &'static str,
    message: &'static str,
    cause: Option<&'static str>,
) -> Box<EvalAltResult> {
    runtime_error(
        "process",
        code,
        operation,
        message,
        false,
        "child_window",
        false,
        cause,
    )
}

fn child_wait_with_output(child: &mut ScriptChild) -> Result<ScriptOutput, Box<EvalAltResult>> {
    wait_for_child(child, None)
}

fn child_wait_with_output_for(
    child: &mut ScriptChild,
    timeout: ScriptDuration,
) -> Result<ScriptOutput, Box<EvalAltResult>> {
    wait_for_child(child, Some(timeout.0))
}

fn wait_for_child(
    child: &ScriptChild,
    timeout: Option<Duration>,
) -> Result<ScriptOutput, Box<EvalAltResult>> {
    let requested_deadline = timeout.map(|value| Instant::now() + value);
    loop {
        let mut state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
        if let Some(output) = state.completed.clone() {
            return Ok(output);
        }
        discard_buffered(&state.stdout)?;
        discard_buffered(&state.stderr)?;
        let deadline = requested_deadline
            .map(|value| value.min(state.deadline))
            .unwrap_or(state.deadline);
        let status = state
            .child
            .as_mut()
            .ok_or("process_child_missing")?
            .try_wait()
            .map_err(|error| format!("process_try_wait: {error}"))?;
        if let Some(status) = status {
            mark_process_exited(&state.stdout);
            mark_process_exited(&state.stderr);
            let output = finish_output(&mut state, status, deadline)?;
            state.completed = Some(output.clone());
            return Ok(output);
        }
        if Instant::now() >= deadline {
            let tree_error = terminate_child_tree(&mut state).err();
            if let Some(process) = state.child.as_mut() {
                let _ = process.wait();
            }
            cancel_stream(&state.stdout);
            cancel_stream(&state.stderr);
            state.child.take();
            let detail = tree_error
                .map(|error| format!("; process_tree_kill: {error}"))
                .unwrap_or_default();
            return Err(format!("process_timeout: child exceeded its deadline{detail}").into());
        }
        drop(state);
        thread::sleep(Duration::from_millis(5));
    }
}

fn finish_output(
    state: &mut ChildState,
    status: ExitStatus,
    deadline: Instant,
) -> Result<ScriptOutput, Box<EvalAltResult>> {
    let tree_result = state.process_tree.terminate();
    let (stdout, stderr) = finish_capture(state, output_drain_deadline(deadline))?;
    state.child.take();
    tree_result.map_err(|error| format!("process_tree_kill: {error}"))?;
    Ok(ScriptOutput {
        success: status.success(),
        exit_code: i64::from(status.code().unwrap_or(-1)),
        truncated: stdout.truncated || stderr.truncated,
        stdout: ScriptBytes(stdout.bytes),
        stderr: ScriptBytes(stderr.bytes),
        complete: !stdout.truncated && !stderr.truncated,
    })
}

fn output_drain_deadline(process_deadline: Instant) -> Instant {
    // The child runtime budget ends once its process exits. Pump threads still
    // need a bounded scheduling window to publish the final bytes and EOF,
    // especially on loaded Windows runners.
    process_deadline.max(Instant::now() + OUTPUT_DRAIN_GRACE)
}

fn finish_capture(
    state: &mut ChildState,
    deadline: Instant,
) -> Result<(CapturedStream, CapturedStream), Box<EvalAltResult>> {
    let stdout = capture_after_close(&state.stdout, deadline)?;
    let stderr = capture_after_close(&state.stderr, deadline)?;
    Ok((stdout, stderr))
}

fn output_error(output: &mut ScriptOutput, code: &str) -> Result<String, Box<EvalAltResult>> {
    validate_text(code, "process_error_code")?;
    Ok(format!(
        "{code}: process exited with code {}{}",
        output.exit_code,
        if output.truncated {
            " (captured output truncated)"
        } else {
            ""
        }
    ))
}

fn output_require_success(output: &mut ScriptOutput, code: &str) -> Result<(), Box<EvalAltResult>> {
    validate_error_code(code)?;
    if output.success {
        return Ok(());
    }
    Err(runtime_error(
        "child",
        "child_nonzero",
        "std.process.Output.require_success",
        format!(
            "{code}: required child process exited with code {}{}",
            output.exit_code,
            if output.truncated {
                " (captured output truncated)"
            } else {
                ""
            }
        ),
        false,
        "child_process",
        output.truncated,
        Some("exit_status"),
    ))
}

fn output_text(bytes: &ScriptBytes, code: &str) -> Result<String, Box<EvalAltResult>> {
    String::from_utf8(bytes.0.clone()).map_err(|_| format!("{code}: output is not UTF-8").into())
}

fn validate_env_name(name: &str) -> Result<(), Box<EvalAltResult>> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return Err(
            "environment_name_invalid: name must be nonempty and exclude '=' and NUL".into(),
        );
    }
    Ok(())
}

fn validate_text(value: &str, code: &str) -> Result<(), Box<EvalAltResult>> {
    if value.contains('\0') {
        return Err(format!("{code}: value contains NUL").into());
    }
    Ok(())
}

fn validate_error_code(code: &str) -> Result<(), Box<EvalAltResult>> {
    if code.is_empty()
        || code.len() > 64
        || !code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
    {
        return Err(
            "process_error_code: code must be 1..64 lowercase ASCII letters, digits, '.', '_', or '-'"
                .into(),
        );
    }
    Ok(())
}

fn bounded(value: rhai::INT, code: &str, maximum: u64) -> Result<u64, Box<EvalAltResult>> {
    let value = u64::try_from(value).map_err(|_| format!("{code}: value must be nonnegative"))?;
    if value > maximum {
        return Err(format!("{code}: maximum is {maximum}").into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::working_context::shell_command_for_child;

    struct KillOnDrop(Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn engine() -> Engine {
        let mut engine = Engine::new();
        crate::script_stdlib::register_local(&mut engine);
        engine
    }

    #[cfg(any(windows, unix))]
    fn wrapped_shell_command(
        shell_command: &str,
        arguments: &[&str],
    ) -> (String, Vec<String>) {
        let shell = crate::platform::runtime::default_terminal_shell();
        let arguments = arguments
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        let mut wrapped = shell_command_for_child(&shell, shell_command, &arguments)
            .expect("shell command should be valid");
        let program = wrapped
            .drain(0..1)
            .next()
            .expect("wrapped shell command must include a program");
        (program, wrapped)
    }

    #[cfg(any(windows, unix))]
    fn long_running_shell_command() -> (&'static str, &'static [&'static str]) {
        long_running_process_command_fixture()
    }

    #[cfg(any(windows, unix))]
    fn long_running_process_command_fixture() -> (&'static str, &'static [&'static str]) {
        if crate::platform::is_windows_host() {
            ("ping.exe", &["-n", "30", "127.0.0.1", ">nul"])
        } else {
            ("sleep", &["30"])
        }
    }

    #[cfg(any(windows, unix))]
    fn long_running_process_command_timeout() -> (&'static str, &'static [&'static str]) {
        if crate::platform::is_windows_host() {
            ("ping", &["-n", "6", "127.0.0.1"])
        } else {
            ("sleep", &["5"])
        }
    }

    #[cfg(any(windows, unix))]
    fn shell_wrapped_process_command(
        windows_command: &str,
        unix_command: &str,
        arguments: &[&str],
    ) -> ScriptCommand {
        let shell_command = if crate::platform::is_windows_host() {
            windows_command
        } else {
            unix_command
        };
        let (program, command_arguments) = wrapped_shell_command(shell_command, arguments);
        let mut process = process_command(&program).expect("shell command should be runnable");
        process.arguments = command_arguments;
        process
    }

    #[cfg(any(windows, unix))]
    fn spawn_owned_tree_fixture() -> ScriptChild {
        let (program, arguments) = wrapped_shell_command(
            long_running_shell_command().0,
            long_running_shell_command().1,
        );
        let mut command = process_command(&program).expect("fixture command");
        command.arguments = arguments;
        command.timeout = Duration::from_secs(30);
        spawn_owned(&command).expect("spawn owned process tree")
    }

    #[cfg(any(windows, unix))]
    fn spawn_unrelated_fixture() -> KillOnDrop {
        let (program, arguments) = wrapped_shell_command(
            long_running_shell_command().0,
            long_running_shell_command().1,
        );
        let mut command = Command::new(program);
        command.args(arguments);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        KillOnDrop(command.spawn().expect("spawn unrelated process"))
    }

    #[cfg(any(windows, unix))]
    fn descendant_ids(root_id: u32) -> Vec<u32> {
        let processes = platform_process_list().expect("process inventory");
        let parents = processes
            .iter()
            .map(|process| (process.id, process.parent_id))
            .collect::<BTreeMap<_, _>>();
        processes
            .iter()
            .filter_map(|process| {
                let candidate = process.id;
                let mut current = candidate;
                for _ in 0..64 {
                    let parent = *parents.get(&current)?;
                    if parent == root_id {
                        return Some(candidate);
                    }
                    if parent == 0 || parent == current {
                        break;
                    }
                    current = parent;
                }
                None
            })
            .collect()
    }

    #[cfg(any(windows, unix))]
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ObservedProcess {
        id: u32,
        start_identity: String,
    }

    #[cfg(any(windows, unix))]
    fn wait_for_descendants(root_id: u32) -> Vec<ObservedProcess> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let descendants = descendant_ids(root_id)
                .into_iter()
                .map(|id| {
                    let start_identity =
                        child_process_tree::start_identity(id).unwrap_or_else(|error| {
                            panic!("descendant {id} start identity unavailable: {error}")
                        });
                    ObservedProcess { id, start_identity }
                })
                .collect::<Vec<_>>();
            if !descendants.is_empty() {
                return descendants;
            }
            assert!(
                Instant::now() < deadline,
                "owned process {root_id} did not create a descendant"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(any(windows, unix))]
    fn observed_process_is_live(
        process: &ObservedProcess,
        observation: child_process_tree::ProcessObservation,
    ) -> Result<bool, String> {
        match observation {
            child_process_tree::ProcessObservation::Live {
                start_identity: Some(current),
            } => Ok(current == process.start_identity),
            child_process_tree::ProcessObservation::Dead { .. } => Ok(false),
            child_process_tree::ProcessObservation::Live {
                start_identity: None,
            } => Err("live process has no start identity".to_owned()),
            child_process_tree::ProcessObservation::Unknown { reason } => Err(reason),
            _ => Err("unrecognized process observation".to_owned()),
        }
    }

    #[cfg(any(windows, unix))]
    fn wait_for_processes_to_disappear(processes: &[ObservedProcess]) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let observations = processes
                .iter()
                .map(|process| {
                    let observation = child_process_tree::observe(process.id);
                    let live = observed_process_is_live(process, observation.clone());
                    (process.clone(), observation, live)
                })
                .collect::<Vec<_>>();
            if observations
                .iter()
                .all(|(_, _, live)| matches!(live, Ok(false)))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "owned descendants survived cleanup or could not be observed: {observations:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn environment_and_duration_are_typed() {
        assert!(engine().eval::<bool>("std::env::has(\"PATH\")").unwrap());
        assert!(
            engine()
                .eval::<bool>("std::env::get(\"PATH\").len > 0")
                .unwrap()
        );
        assert_eq!(
            engine()
                .eval::<rhai::INT>("std::time::Duration::from_secs(2).millis")
                .unwrap(),
            2_000
        );
        assert_eq!(
            engine().eval::<rhai::INT>("std::process::id()").unwrap(),
            rhai::INT::from(std::process::id())
        );
        assert_eq!(
            engine()
                .eval::<rhai::INT>("std::time::Duration::from_secs(3600).millis")
                .unwrap(),
            3_600_000
        );
        let error = engine()
            .eval::<()>("std::time::Duration::from_millis(3600001)")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duration_millis"));
    }

    #[test]
    fn process_list_contains_the_current_process_with_a_name() {
        assert!(
            engine()
                .eval::<bool>(
                    r#"
                        let found = false;
                        for process in std::process::list() {
                            if process.id == std::process::id()
                                    && process.executable_name.len > 0
                                    && process.parent_id >= 0 {
                                found = true;
                            }
                        }
                        found
                    "#,
                )
                .unwrap()
        );
    }

    #[test]
    #[cfg(any(windows, unix))]
    fn process_kill_terminates_an_arbitrary_operating_system_process() {
        let (program, arguments) = wrapped_shell_command(
            long_running_shell_command().0,
            long_running_shell_command().1,
        );
        let mut child = Command::new(program);
        child.args(arguments);
        let mut child = child
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        engine()
            .eval::<()>(&format!("std::process::kill({})", child.id()))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("std::process::kill did not terminate PID {}", child.id());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn command_preserves_environment_output_and_exit_code() {
        let mut command = shell_wrapped_process_command(
            "set",
            "printenv",
            &[],
        );
        command.environment
            .insert("AGENTERM_PROCESS_TEST".to_owned(), Some("argv-safe".to_owned()));
        let output = command_output(&mut command).unwrap();
        assert!(output.success);
        assert_eq!(output.exit_code, 0);
        let stdout = output_text(&output.stdout, "process_stdout_not_utf8").unwrap();
        assert!(stdout.contains("AGENTERM_PROCESS_TEST=argv-safe"));
        assert_eq!(
            output_text(&output.stderr, "process_stderr_not_utf8").unwrap(),
            ""
        );
    }

    #[test]
    fn command_accepts_arbitrary_binary_stdin() {
        let mut command = process_command("unused").unwrap();
        command_stdin_bytes(&mut command, ScriptBytes(vec![0, 127, 128, 255])).unwrap();
        assert_eq!(command.stdin, vec![0, 127, 128, 255]);
        let oversized = ScriptBytes(vec![0; MAX_STDIN_BYTES + 1]);
        assert!(
            command_stdin_bytes(&mut command, oversized)
                .unwrap_err()
                .to_string()
                .contains("process_stdin_too_large")
        );
    }

    #[test]
    fn command_can_redirect_stdout_to_one_explicit_file() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-process-stdout-{}.txt",
            std::process::id()
        ));
        let script_path = path.to_string_lossy().replace('\\', "\\\\");
        let mut command = shell_wrapped_process_command(
            "<nul set /p =redirected&exit /b 0",
            "printf redirected",
            &[],
        );
        command.stdout_file = Some(PathBuf::from(script_path));
        let output = command_output(&mut command).unwrap();
        assert!(output.success);
        assert!(output.stdout.0.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "redirected");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn command_can_redirect_stderr_to_one_explicit_file() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-process-stderr-{}.txt",
            std::process::id()
        ));
        let script_path = path.to_string_lossy().replace('\\', "\\\\");
        let mut command = shell_wrapped_process_command(
            "<nul set /p =redirected 1>&2&exit /b 0",
            "printf redirected >&2",
            &[],
        );
        command.stderr_file = Some(PathBuf::from(script_path));
        let output = command_output(&mut command).unwrap();
        assert!(output.success);
        assert!(output.stderr.0.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "redirected");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn output_can_require_a_successful_child_exit() {
        let mut command = shell_wrapped_process_command("exit /b 7", "exit 7", &[]);
        let mut output = command_output(&mut command).unwrap();
        let error = output_require_success(&mut output, "test-child")
            .unwrap_err()
            .to_string();
        assert!(error.contains("child_nonzero"));
        assert!(error.contains("test-child"));
        assert!(error.contains("code 7"));
    }

    #[test]
    fn required_child_failure_is_a_catchable_typed_error() {
        let mut command = shell_wrapped_process_command("exit /b 7", "exit 7", &[]);
        let mut output = command_output(&mut command).unwrap();
        let error = output_require_success(&mut output, "test-child").unwrap_err();
        let fields = crate::script_error::runtime_error_fields(&error).expect("error fields");
        assert_eq!(fields.class, "child");
        assert_eq!(
            fields.code,
            "child_nonzero"
        );
        assert_eq!(
            fields.operation,
            "std.process.Output.require_success"
        );
        assert_eq!(
            fields.target_kind,
            "child_process"
        );
        assert!(!fields.retryable);
        assert!(!fields.truncated);
        assert_eq!(fields.cause_class.as_deref(), Some("exit_status"));
    }

    #[test]
    fn child_id_remains_stable_after_wait() {
        let mut command = shell_wrapped_process_command("exit", "exit", &[]);
        command.arguments.push("0".to_string());
        let mut child = command_start(&mut command).unwrap();
        let before = child_id(&mut child).unwrap();
        child_wait_with_output(&mut child).unwrap();
        let after = child_id(&mut child).unwrap();
        let facts = child_platform_facts(&mut child).unwrap();
        let state = child_state(&mut child).unwrap();
        assert!(before > 0);
        assert_eq!(before, after);
        assert_eq!(state, "exited");
        assert_eq!(
            facts.top_level_window_supported,
            crate::platform::hosted_script_worker_available()
        );
        assert!(!facts.top_level_window_present);
        assert_eq!(facts.top_level_window_id, 0);
    }

    #[test]
    fn command_timeout_is_typed() {
        let (timeout_command, timeout_arguments) = long_running_process_command_timeout();
        let mut command = shell_wrapped_process_command(
            timeout_command,
            timeout_command,
            timeout_arguments,
        );
        command.timeout = Duration::from_millis(10);
        assert!(
            command_output(&mut command)
                .unwrap_err()
                .to_string()
                .contains("process_timeout")
        );
    }

    #[test]
    #[cfg(any(windows, unix))]
    fn child_kill_tree_reaps_descendants_without_touching_unrelated_processes() {
        let mut owned = spawn_owned_tree_fixture();
        let owned_id = u32::try_from(child_id(&mut owned).unwrap()).unwrap();
        let descendants = wait_for_descendants(owned_id);
        let mut unrelated = spawn_unrelated_fixture();

        let mut scope = rhai::Scope::new();
        scope.push("owned", owned.clone());
        engine()
            .eval_with_scope::<()>(&mut scope, "owned.kill_tree()")
            .expect("public Child.kill_tree API");
        let output = child_wait_with_output_for(&mut owned, ScriptDuration(Duration::from_secs(2)))
            .expect("reap killed root process");
        assert!(!output.success);
        wait_for_processes_to_disappear(&descendants);
        assert!(
            unrelated.0.try_wait().unwrap().is_none(),
            "owned tree cleanup killed an unrelated process"
        );
    }

    #[test]
    #[cfg(any(windows, unix))]
    fn cleanup_observation_tracks_process_identity_instead_of_pid_presence() {
        let process = ObservedProcess {
            id: 42,
            start_identity: "original".to_owned(),
        };
        assert!(
            observed_process_is_live(
                &process,
                child_process_tree::ProcessObservation::Live {
                    start_identity: Some("original".to_owned()),
                },
            )
            .unwrap()
        );
        assert!(
            !observed_process_is_live(
                &process,
                child_process_tree::ProcessObservation::Live {
                    start_identity: Some("reused-pid".to_owned()),
                },
            )
            .unwrap()
        );
        assert!(
            !observed_process_is_live(
                &process,
                child_process_tree::ProcessObservation::Dead {
                    reason: "exited".to_owned(),
                },
            )
            .unwrap()
        );
        assert!(
            observed_process_is_live(
                &process,
                child_process_tree::ProcessObservation::Unknown {
                    reason: "query failed".to_owned(),
                },
            )
            .is_err()
        );
    }

    #[test]
    #[cfg(any(windows, unix))]
    fn child_wait_timeout_reaps_descendants_without_touching_unrelated_processes() {
        let mut owned = spawn_owned_tree_fixture();
        let owned_id = u32::try_from(child_id(&mut owned).unwrap()).unwrap();
        let descendants = wait_for_descendants(owned_id);
        let mut unrelated = spawn_unrelated_fixture();

        let error =
            child_wait_with_output_for(&mut owned, ScriptDuration(Duration::from_millis(10)))
                .unwrap_err()
                .to_string();
        assert!(error.contains("process_timeout"));
        assert!(owned.0.lock().unwrap().child.is_none());
        wait_for_processes_to_disappear(&descendants);
        assert!(
            unrelated.0.try_wait().unwrap().is_none(),
            "timeout cleanup killed an unrelated process"
        );
    }

    #[test]
    fn exited_child_receives_a_separate_bounded_output_drain_window() {
        let before = Instant::now();
        let expired = before - Duration::from_secs(1);
        assert!(output_drain_deadline(expired) >= before + OUTPUT_DRAIN_GRACE);

        let future = before + Duration::from_secs(5);
        assert_eq!(output_drain_deadline(future), future);
    }

    #[test]
    fn child_streams_are_live_bounded_and_preserve_final_capture() {
        let mut command = shell_wrapped_process_command(
            "echo abcdef",
            "printf abcdef",
            &[],
        );
        let mut child = command_start(&mut command).unwrap();
        let mut stream = child_stdout(&mut child).unwrap();
        let mut scope = rhai::Scope::new();
        scope.push("stream", stream.clone());
        let mut engine = engine();
        let first = engine
            .eval_with_scope::<String>(
                &mut scope,
                "stream.read(2, std::time::Duration::from_secs(1)).to_text()",
            )
            .unwrap();
        let rest = engine
            .eval_with_scope::<String>(
                &mut scope,
                "stream.collect(16, std::time::Duration::from_secs(1)).to_text()",
            )
            .unwrap();
        let stream_complete = engine
            .eval_with_scope::<bool>(&mut scope, "stream.complete")
            .unwrap();
        let output = child_wait_with_output(&mut child).unwrap();
        assert_eq!(first, "ab");
        assert!(rest.starts_with("cdef"));
        assert!(output_text(&output.stdout, "process_stdout_not_utf8")
            .unwrap()
            .starts_with("abcdef"));
        assert!(stream_complete);
        assert!(output.complete);
    }

    #[test]
    fn truncated_process_capture_is_not_reported_as_complete() {
        let mut command = shell_wrapped_process_command(
            "echo abcdefgh",
            "printf abcdefgh",
            &[],
        );
        command_capture_limit(&mut command, 4).unwrap();
        let output = command_output(&mut command).unwrap();
        assert_eq!(output.stdout.0, b"abcd");
        assert!(output.truncated);
        assert!(!output.complete);
    }
}

