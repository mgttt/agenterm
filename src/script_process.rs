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
        discard_buffered, from_process_reader, from_reader, mark_process_exited,
    },
};

const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(1);
const DEFAULT_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_STDIN_BYTES: usize = 4 * 1024 * 1024;

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
    stdout: ScriptStream,
    stderr: ScriptStream,
    deadline: Instant,
    completed: Option<ScriptOutput>,
}

impl Drop for ChildState {
    fn drop(&mut self) {
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
    engine.register_fn("window_key", child_window_key);
    engine.register_fn("window_pointer", child_window_pointer);
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

#[cfg(windows)]
fn platform_process_kill(id: u32) -> Result<(), Box<EvalAltResult>> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };

    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, id) };
    if process.is_null() {
        return Err(runtime_error(
            "host",
            "process_kill_open",
            "std.process.kill",
            "unable to open the selected operating-system process",
            false,
            "process",
            false,
            Some("os"),
        ));
    }
    let terminated = unsafe { TerminateProcess(process, 1) };
    unsafe {
        CloseHandle(process);
    }
    if terminated == 0 {
        return Err(runtime_error(
            "host",
            "process_kill",
            "std.process.kill",
            "unable to terminate the selected operating-system process",
            false,
            "process",
            false,
            Some("os"),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn platform_process_kill(id: u32) -> Result<(), Box<EvalAltResult>> {
    let id = libc::pid_t::try_from(id).map_err(|_| {
        runtime_error(
            "configuration",
            "process_id_invalid",
            "std.process.kill",
            "process ID exceeds the platform range",
            false,
            "process",
            false,
            Some("integer_range"),
        )
    })?;
    if unsafe { libc::kill(id, libc::SIGKILL) } != 0 {
        return Err(runtime_error(
            "host",
            "process_kill",
            "std.process.kill",
            "unable to terminate the selected operating-system process",
            false,
            "process",
            false,
            Some("os"),
        ));
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn platform_process_kill(_id: u32) -> Result<(), Box<EvalAltResult>> {
    Err(runtime_error(
        "host",
        "process_kill_unsupported",
        "std.process.kill",
        "operating-system process termination is not supported on this platform",
        false,
        "process",
        false,
        Some("platform"),
    ))
}

#[cfg(windows)]
fn platform_process_list() -> Result<Vec<ScriptProcessInfo>, Box<EvalAltResult>> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(runtime_error(
            "host",
            "process_list_failed",
            "std.process.list",
            "unable to capture the operating-system process inventory",
            true,
            "process_inventory",
            false,
            Some("os"),
        ));
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut processes = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while present {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable_name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if !executable_name.is_empty() {
            processes.push(ScriptProcessInfo {
                id: entry.th32ProcessID,
                executable_name,
            });
        }
        present = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        CloseHandle(snapshot);
    }
    Ok(processes)
}

#[cfg(target_os = "linux")]
fn platform_process_list() -> Result<Vec<ScriptProcessInfo>, Box<EvalAltResult>> {
    let entries = std::fs::read_dir("/proc").map_err(|_| {
        runtime_error(
            "host",
            "process_list_failed",
            "std.process.list",
            "unable to read the operating-system process inventory",
            true,
            "process_inventory",
            false,
            Some("os"),
        )
    })?;
    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(executable) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let Some(executable_name) = executable.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        processes.push(ScriptProcessInfo {
            id,
            executable_name: executable_name.to_owned(),
        });
    }
    Ok(processes)
}

#[cfg(target_os = "macos")]
fn platform_process_list() -> Result<Vec<ScriptProcessInfo>, Box<EvalAltResult>> {
    use std::ffi::{CStr, c_char, c_int, c_void};

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDPATHINFO_MAXSIZE: u32 = 4 * 1024;

    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listpids(
            process_type: u32,
            type_info: u32,
            buffer: *mut c_void,
            buffer_size: c_int,
        ) -> c_int;
        fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffer_size: u32) -> c_int;
    }

    let required = unsafe { proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if required <= 0 {
        return Err(runtime_error(
            "host",
            "process_list_failed",
            "std.process.list",
            "unable to size the operating-system process inventory",
            true,
            "process_inventory",
            false,
            Some("os"),
        ));
    }
    let capacity = usize::try_from(required).unwrap_or_default() / size_of::<c_int>() + 32;
    let mut ids: Vec<c_int> = vec![0; capacity];
    let buffer_size = c_int::try_from(ids.len() * size_of::<c_int>()).map_err(|_| {
        runtime_error(
            "limit",
            "process_list_too_large",
            "std.process.list",
            "operating-system process inventory exceeds the supported size",
            false,
            "process_inventory",
            false,
            Some("integer_conversion"),
        )
    })?;
    let bytes = unsafe { proc_listpids(PROC_ALL_PIDS, 0, ids.as_mut_ptr().cast(), buffer_size) };
    if bytes <= 0 {
        return Err(runtime_error(
            "host",
            "process_list_failed",
            "std.process.list",
            "unable to capture the operating-system process inventory",
            true,
            "process_inventory",
            false,
            Some("os"),
        ));
    }
    ids.truncate(usize::try_from(bytes).unwrap_or_default() / size_of::<c_int>());
    let mut processes = Vec::new();
    for id in ids.into_iter().filter(|id| *id > 0) {
        let mut path: Vec<c_char> =
            vec![0; usize::try_from(PROC_PIDPATHINFO_MAXSIZE).unwrap_or_default()];
        let length =
            unsafe { proc_pidpath(id, path.as_mut_ptr().cast(), PROC_PIDPATHINFO_MAXSIZE) };
        if length <= 0 {
            continue;
        }
        let full_path = unsafe { CStr::from_ptr(path.as_ptr()) }.to_string_lossy();
        let executable_name = std::path::Path::new(full_path.as_ref())
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        if !executable_name.is_empty() {
            let Ok(id) = u32::try_from(id) else {
                continue;
            };
            processes.push(ScriptProcessInfo {
                id,
                executable_name,
            });
        }
    }
    Ok(processes)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_process_list() -> Result<Vec<ScriptProcessInfo>, Box<EvalAltResult>> {
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "current-process".to_owned());
    Ok(vec![ScriptProcessInfo {
        id: std::process::id(),
        executable_name,
    }])
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
    let stdout = if command.stdout_file.is_some() {
        from_reader(std::io::empty(), "bytes", 0)
    } else {
        from_process_reader(
            child.stdout.take().ok_or("process_stdout_unavailable")?,
            "bytes",
            command.capture_bytes,
        )
    };
    let stderr = if command.stderr_file.is_some() {
        from_reader(std::io::empty(), "bytes", 0)
    } else {
        from_process_reader(
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

#[cfg(windows)]
fn find_top_level_window(id: u32) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};

    struct Search {
        id: u32,
        window: windows_sys::Win32::Foundation::HWND,
    }

    unsafe extern "system" fn visit(
        window: windows_sys::Win32::Foundation::HWND,
        parameter: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::core::BOOL {
        let search = unsafe { &mut *(parameter as *mut Search) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
        }
        if owner == search.id {
            search.window = window;
            0
        } else {
            1
        }
    }

    let mut search = Search {
        id,
        window: core::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(
            Some(visit),
            (&mut search as *mut Search).cast::<core::ffi::c_void>() as isize,
        );
    }
    search.window
}

#[cfg(windows)]
fn process_platform_facts(id: u32) -> ScriptProcessPlatformFacts {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    let window = find_top_level_window(id);
    let foreground_window = unsafe { GetForegroundWindow() };
    let top_level_window_title = if window.is_null() {
        String::new()
    } else {
        let length = unsafe { GetWindowTextLengthW(window) };
        if length <= 0 {
            String::new()
        } else {
            let mut buffer = vec![0_u16; length as usize + 1];
            let copied =
                unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
        }
    };
    ScriptProcessPlatformFacts {
        top_level_window_supported: true,
        top_level_window_present: !window.is_null(),
        top_level_window_id: window as isize as i64,
        top_level_window_title,
        foreground_window_id: foreground_window as isize as i64,
        top_level_window_is_foreground: !window.is_null() && window == foreground_window,
    }
}

#[cfg(not(windows))]
fn process_platform_facts(_id: u32) -> ScriptProcessPlatformFacts {
    ScriptProcessPlatformFacts {
        top_level_window_supported: false,
        top_level_window_present: false,
        top_level_window_id: 0,
        top_level_window_title: String::new(),
        foreground_window_id: 0,
        top_level_window_is_foreground: false,
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

fn child_process_id(child: &ScriptChild) -> Result<u32, Box<EvalAltResult>> {
    Ok(child
        .0
        .lock()
        .map_err(|_| "process_child_state_poisoned")?
        .id)
}

#[cfg(windows)]
fn child_window_key(child: &mut ScriptChild, key: &str) -> Result<(), Box<EvalAltResult>> {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
            VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{PostMessageW, WM_KEYDOWN, WM_KEYUP},
    };

    let virtual_key = match key {
        "Backspace" => VK_BACK,
        "Delete" => VK_DELETE,
        "Down" => VK_DOWN,
        "End" => VK_END,
        "Enter" => VK_RETURN,
        "Escape" => VK_ESCAPE,
        "Home" => VK_HOME,
        "Left" => VK_LEFT,
        "Right" => VK_RIGHT,
        "Tab" => VK_TAB,
        "Up" => VK_UP,
        _ => {
            return Err(process_window_error(
                "process_window_key_invalid",
                "Child.window_key",
                "window key must be Backspace, Delete, Down, End, Enter, Escape, Home, Left, Right, Tab, or Up",
                Some("invalid_input"),
            ));
        }
    };
    let window = find_top_level_window(child_process_id(child)?);
    if window.is_null() {
        return Err(process_window_error(
            "process_window_not_found",
            "Child.window_key",
            "child has no top-level window",
            Some("not_found"),
        ));
    }
    let pressed = unsafe { PostMessageW(window, WM_KEYDOWN, usize::from(virtual_key), 0) };
    let released = unsafe { PostMessageW(window, WM_KEYUP, usize::from(virtual_key), 0) };
    if pressed == 0 || released == 0 {
        return Err(process_window_error(
            "process_window_input",
            "Child.window_key",
            "native window key delivery failed",
            Some("platform_error"),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn child_window_key(_child: &mut ScriptChild, _key: &str) -> Result<(), Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_key",
        "native child-window input is not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn child_window_pointer(
    child: &mut ScriptChild,
    action: &str,
    x: rhai::INT,
    y: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, ChildWindowFromPointEx, SendMessageW,
        WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

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
    let window = find_top_level_window(child_process_id(child)?);
    if window.is_null() {
        return Err(process_window_error(
            "process_window_not_found",
            "Child.window_pointer",
            "child has no top-level window",
            Some("not_found"),
        ));
    }
    let point = (((y & 0xffff) << 16) | (x & 0xffff)) as isize;
    if action == "click" {
        let child = unsafe {
            ChildWindowFromPointEx(window, POINT { x, y }, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED)
        };
        if !child.is_null() && child != window {
            unsafe {
                SendMessageW(child, BM_CLICK, 0, 0);
            }
        } else {
            unsafe {
                SendMessageW(window, WM_LBUTTONDOWN, 0, point);
                SendMessageW(window, WM_LBUTTONUP, 0, point);
            }
        }
        return Ok(());
    }
    let (message, button, parameter) = match action {
        "down" => (WM_LBUTTONDOWN, 0, point),
        "move" => (WM_MOUSEMOVE, 0, point),
        "move-held" => (WM_MOUSEMOVE, 1, point),
        "up" => (WM_LBUTTONUP, 0, point),
        "capture-changed" => (WM_CAPTURECHANGED, 0, 0),
        _ => {
            return Err(process_window_error(
                "process_window_pointer_action_invalid",
                "Child.window_pointer",
                "window pointer action must be click, down, move, move-held, up, or capture-changed",
                Some("invalid_input"),
            ));
        }
    };
    unsafe {
        SendMessageW(window, message, button, parameter);
    }
    Ok(())
}

#[cfg(not(windows))]
fn child_window_pointer(
    _child: &mut ScriptChild,
    _action: &str,
    _x: rhai::INT,
    _y: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_pointer",
        "native child-window input is not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn resolve_child_window(
    child: &ScriptChild,
    operation: &'static str,
) -> Result<windows_sys::Win32::Foundation::HWND, Box<EvalAltResult>> {
    let window = find_top_level_window(child_process_id(child)?);
    if window.is_null() {
        return Err(process_window_error(
            "process_window_not_found",
            operation,
            "child has no top-level window",
            Some("not_found"),
        ));
    }
    Ok(window)
}

#[cfg(windows)]
fn child_window_message(
    child: &mut ScriptChild,
    message: rhai::INT,
    wparam: rhai::INT,
    lparam: rhai::INT,
) -> Result<rhai::INT, Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

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
    let window = resolve_child_window(child, "Child.window_message")?;
    Ok(unsafe { SendMessageW(window, message, wparam, lparam) } as rhai::INT)
}

#[cfg(not(windows))]
fn child_window_message(
    _child: &mut ScriptChild,
    _message: rhai::INT,
    _wparam: rhai::INT,
    _lparam: rhai::INT,
) -> Result<rhai::INT, Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_message",
        "native child-window messaging is not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn native_rect(
    child: &ScriptChild,
    client: bool,
    operation: &'static str,
) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    };

    let window = resolve_child_window(child, operation)?;
    let mut rect = RECT::default();
    let succeeded = unsafe {
        if client {
            GetClientRect(window, &mut rect)
        } else {
            GetWindowRect(window, &mut rect)
        }
    };
    if succeeded == 0 {
        return Err(process_window_error(
            "process_window_rect",
            operation,
            "native window bounds could not be read",
            Some("platform_error"),
        ));
    }
    Ok(ScriptWindowRect {
        left: i64::from(rect.left),
        top: i64::from(rect.top),
        right: i64::from(rect.right),
        bottom: i64::from(rect.bottom),
    })
}

#[cfg(windows)]
fn child_window_rect(child: &mut ScriptChild) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    native_rect(child, false, "Child.window_rect")
}

#[cfg(not(windows))]
fn child_window_rect(_child: &mut ScriptChild) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_rect",
        "native child-window bounds are not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn child_window_client_rect(
    child: &mut ScriptChild,
) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    native_rect(child, true, "Child.window_client_rect")
}

#[cfg(not(windows))]
fn child_window_client_rect(
    _child: &mut ScriptChild,
) -> Result<ScriptWindowRect, Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_client_rect",
        "native child-window client bounds are not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn child_window_resize(
    child: &mut ScriptChild,
    width: rhai::INT,
    height: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos,
    };

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
    let window = resolve_child_window(child, "Child.window_resize")?;
    let succeeded = unsafe {
        SetWindowPos(
            window,
            std::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if succeeded == 0 {
        return Err(process_window_error(
            "process_window_resize",
            "Child.window_resize",
            "native window resize failed",
            Some("platform_error"),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn child_window_resize(
    _child: &mut ScriptChild,
    _width: rhai::INT,
    _height: rhai::INT,
) -> Result<(), Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "Child.window_resize",
        "native child-window resize is not implemented on this platform",
        Some("unsupported"),
    ))
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
    #[cfg(windows)]
    {
        resolve_window_control(&control, "Child.window_control")?;
    }
    Ok(control)
}

#[cfg(windows)]
fn resolve_window_control(
    control: &ScriptWindowControl,
    operation: &'static str,
) -> Result<windows_sys::Win32::Foundation::HWND, Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;

    let window = resolve_child_window(&control.child, operation)?;
    let item = unsafe { GetDlgItem(window, control.id) };
    if item.is_null() {
        return Err(process_window_error(
            "process_window_control_not_found",
            operation,
            "native child control was not found",
            Some("not_found"),
        ));
    }
    Ok(item)
}

#[cfg(windows)]
fn window_control_visible(control: &mut ScriptWindowControl) -> Result<bool, Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    Ok(unsafe { IsWindowVisible(resolve_window_control(control, "WindowControl.visible")?) != 0 })
}

#[cfg(not(windows))]
fn window_control_visible(_control: &mut ScriptWindowControl) -> Result<bool, Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "WindowControl.visible",
        "native child controls are not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn window_control_text(control: &mut ScriptWindowControl) -> Result<String, Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    let item = resolve_window_control(control, "WindowControl.text")?;
    let length = unsafe { GetWindowTextLengthW(item) };
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or(0).saturating_add(1)];
    let copied = unsafe {
        GetWindowTextW(
            item,
            buffer.as_mut_ptr(),
            i32::try_from(buffer.len()).unwrap_or(i32::MAX),
        )
    };
    if copied < 0 {
        return Err(process_window_error(
            "process_window_control_text",
            "WindowControl.text",
            "native child control text could not be read",
            Some("platform_error"),
        ));
    }
    Ok(String::from_utf16_lossy(
        &buffer[..usize::try_from(copied).unwrap_or(0)],
    ))
}

#[cfg(not(windows))]
fn window_control_text(_control: &mut ScriptWindowControl) -> Result<String, Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "WindowControl.text",
        "native child controls are not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn window_control_set_text(
    control: &mut ScriptWindowControl,
    text: &str,
) -> Result<(), Box<EvalAltResult>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETTEXT};

    let item = resolve_window_control(control, "WindowControl.set_text")?;
    let wide = std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { SendMessageW(item, WM_SETTEXT, 0, wide.as_ptr() as isize) } == 0 {
        return Err(process_window_error(
            "process_window_control_text",
            "WindowControl.set_text",
            "native child control text could not be written",
            Some("platform_error"),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn window_control_set_text(
    _control: &mut ScriptWindowControl,
    _text: &str,
) -> Result<(), Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "WindowControl.set_text",
        "native child controls are not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn window_control_click(control: &mut ScriptWindowControl) -> Result<(), Box<EvalAltResult>> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, SendMessageW};

    let item = resolve_window_control(control, "WindowControl.click")?;
    unsafe {
        SendMessageW(item, BM_CLICK, 0, 0);
    }
    Ok(())
}

#[cfg(not(windows))]
fn window_control_click(_control: &mut ScriptWindowControl) -> Result<(), Box<EvalAltResult>> {
    Err(process_window_error(
        "process_window_input_unsupported",
        "WindowControl.click",
        "native child controls are not implemented on this platform",
        Some("unsupported"),
    ))
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
            if let Some(process) = state.child.as_mut() {
                let _ = process.kill();
                let _ = process.wait();
            }
            cancel_stream(&state.stdout);
            cancel_stream(&state.stderr);
            state.child.take();
            return Err("process_timeout: child exceeded its deadline".into());
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
    let (stdout, stderr) = finish_capture(state, output_drain_deadline(deadline))?;
    state.child.take();
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

    fn engine() -> Engine {
        let mut engine = Engine::new();
        crate::script_stdlib::register_local(&mut engine);
        engine
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
                                    && process.executable_name.len > 0 {
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
        let mut child = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(["/d", "/s", "/c", "ping -n 30 127.0.0.1 >nul"]);
            command
        } else {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "sleep 30"]);
            command
        }
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
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "echo %AGENTERM_PROCESS_TEST%&echo error 1>&2&exit /b 7"]);
                c.env("AGENTERM_PROCESS_TEST", "argv-safe");
                let o = c.output();
                let stdout = o.stdout_text();
                let stderr = o.stderr_text();
                stdout.trim();
                stderr.trim();
                #{ success: o.success, code: o.exit_code,
                   stdout: stdout, stderr: stderr }
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "printf '%s' \"$AGENTERM_PROCESS_TEST\"; printf error >&2; exit 7"]);
                c.env("AGENTERM_PROCESS_TEST", "argv-safe");
                let o = c.output();
                #{ success: o.success, code: o.exit_code,
                   stdout: o.stdout_text(), stderr: o.stderr_text() }
            "#
        };
        let result = engine().eval::<rhai::Map>(source).unwrap();
        assert!(!result["success"].as_bool().unwrap());
        assert_eq!(result["code"].as_int().unwrap(), 7);
        assert_eq!(result["stdout"].clone().into_string().unwrap(), "argv-safe");
        assert_eq!(result["stderr"].clone().into_string().unwrap(), "error");
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
        let source = if cfg!(windows) {
            format!(
                r#"
                    let c = std::process::command("cmd.exe");
                    c.args(["/d", "/s", "/c",
                        "<nul set /p =redirected&exit /b 0"]);
                    c.stdout_file("{script_path}");
                    let output = c.output();
                    #{{ success: output.success, stdout: output.stdout_text() }}
                "#
            )
        } else {
            format!(
                r#"
                    let c = std::process::command("/bin/sh");
                    c.args(["-c", "printf redirected"]);
                    c.stdout_file("{script_path}");
                    let output = c.output();
                    #{{ success: output.success, stdout: output.stdout_text() }}
                "#
            )
        };
        let result = engine().eval::<rhai::Map>(&source).unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["stdout"].clone().into_string().unwrap(), "");
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
        let source = if cfg!(windows) {
            format!(
                r#"
                    let c = std::process::command("cmd.exe");
                    c.args(["/d", "/s", "/c",
                        "<nul set /p =redirected 1>&2&exit /b 0"]);
                    c.stderr_file("{script_path}");
                    let output = c.output();
                    #{{ success: output.success, stderr: output.stderr_text() }}
                "#
            )
        } else {
            format!(
                r#"
                    let c = std::process::command("/bin/sh");
                    c.args(["-c", "printf redirected >&2"]);
                    c.stderr_file("{script_path}");
                    let output = c.output();
                    #{{ success: output.success, stderr: output.stderr_text() }}
                "#
            )
        };
        let result = engine().eval::<rhai::Map>(&source).unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["stderr"].clone().into_string().unwrap(), "");
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "redirected");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn output_can_require_a_successful_child_exit() {
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "exit /b 7"]);
                let output = c.output();
                output.require_success("test-child");
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "exit 7"]);
                let output = c.output();
                output.require_success("test-child");
            "#
        };
        let error = engine().eval::<()>(source).unwrap_err().to_string();
        assert!(error.contains("child_nonzero"));
        assert!(error.contains("test-child"));
        assert!(error.contains("code 7"));
    }

    #[test]
    fn required_child_failure_is_a_catchable_typed_error() {
        let source = if cfg!(windows) {
            r#"
                let caught = ();
                try {
                    let c = std::process::command("cmd.exe");
                    c.args(["/d", "/s", "/c", "exit /b 7"]);
                    let output = c.output();
                    output.require_success("test-child");
                } catch (error) {
                    caught = error;
                }
                caught
            "#
        } else {
            r#"
                let caught = ();
                try {
                    let c = std::process::command("/bin/sh");
                    c.args(["-c", "exit 7"]);
                    let output = c.output();
                    output.require_success("test-child");
                } catch (error) {
                    caught = error;
                }
                caught
            "#
        };
        let error = engine().eval::<rhai::Map>(source).unwrap();
        assert_eq!(error["class"].clone().into_string().unwrap(), "child");
        assert_eq!(
            error["code"].clone().into_string().unwrap(),
            "child_nonzero"
        );
        assert_eq!(
            error["operation"].clone().into_string().unwrap(),
            "std.process.Output.require_success"
        );
        assert_eq!(
            error["target_kind"].clone().into_string().unwrap(),
            "child_process"
        );
        assert!(!error["retryable"].as_bool().unwrap());
        assert!(!error["truncated"].as_bool().unwrap());
        assert_eq!(
            error["cause_class"].clone().into_string().unwrap(),
            "exit_status"
        );
    }

    #[test]
    fn child_id_remains_stable_after_wait() {
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "exit /b 0"]);
                let child = c.start();
                let before = child.id;
                child.wait_with_output();
                let facts = child.platform_facts;
                #{ before: before, after: child.id, state: child.state,
                   window_supported: facts.top_level_window_supported,
                   window_present: facts.top_level_window_present,
                   window_id: facts.top_level_window_id }
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "exit 0"]);
                let child = c.start();
                let before = child.id;
                child.wait_with_output();
                let facts = child.platform_facts;
                #{ before: before, after: child.id, state: child.state,
                   window_supported: facts.top_level_window_supported,
                   window_present: facts.top_level_window_present,
                   window_id: facts.top_level_window_id }
            "#
        };
        let result = engine().eval::<rhai::Map>(source).unwrap();
        assert!(result["before"].as_int().unwrap() > 0);
        assert_eq!(
            result["before"].as_int().unwrap(),
            result["after"].as_int().unwrap()
        );
        assert_eq!(result["state"].clone().into_string().unwrap(), "exited");
        assert_eq!(result["window_supported"].as_bool().unwrap(), cfg!(windows));
        assert!(!result["window_present"].as_bool().unwrap());
        assert_eq!(result["window_id"].as_int().unwrap(), 0);
    }

    #[test]
    fn command_timeout_is_typed() {
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "ping -n 6 127.0.0.1 >nul"]);
                c.timeout(std::time::Duration::from_millis(10));
                c.output()
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "sleep 5"]);
                c.timeout(std::time::Duration::from_millis(10));
                c.output()
            "#
        };
        assert!(
            engine()
                .eval::<ScriptOutput>(source)
                .unwrap_err()
                .to_string()
                .contains("process_timeout")
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
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "<nul set /p =abcdef"]);
                let child = c.start();
                let stream = child.stdout;
                let first = stream.read(2,
                    std::time::Duration::from_secs(1)).to_text();
                let rest = stream.collect(16,
                    std::time::Duration::from_secs(1)).to_text();
                let output = child.wait_with_output();
                #{first: first, rest: rest, final: output.stdout_text(),
                  stream_complete: stream.complete, output_complete: output.complete}
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "printf abcdef"]);
                let child = c.start();
                let stream = child.stdout;
                let first = stream.read(2,
                    std::time::Duration::from_secs(1)).to_text();
                let rest = stream.collect(16,
                    std::time::Duration::from_secs(1)).to_text();
                let output = child.wait_with_output();
                #{first: first, rest: rest, final: output.stdout_text(),
                  stream_complete: stream.complete, output_complete: output.complete}
            "#
        };
        let result = engine().eval::<rhai::Map>(source).unwrap();
        assert_eq!(result["first"].clone().into_string().unwrap(), "ab");
        assert_eq!(result["rest"].clone().into_string().unwrap(), "cdef");
        assert_eq!(result["final"].clone().into_string().unwrap(), "abcdef");
        assert!(result["stream_complete"].as_bool().unwrap());
        assert!(result["output_complete"].as_bool().unwrap());
    }

    #[test]
    fn truncated_process_capture_is_not_reported_as_complete() {
        let source = if cfg!(windows) {
            r#"
                let c = std::process::command("cmd.exe");
                c.args(["/d", "/s", "/c", "<nul set /p =abcdefgh"]);
                c.capture_limit(4);
                c.output()
            "#
        } else {
            r#"
                let c = std::process::command("/bin/sh");
                c.args(["-c", "printf abcdefgh"]);
                c.capture_limit(4);
                c.output()
            "#
        };
        let output = engine().eval::<ScriptOutput>(source).unwrap();
        assert_eq!(output.stdout.0, b"abcd");
        assert!(output.truncated);
        assert!(!output.complete);
    }
}
