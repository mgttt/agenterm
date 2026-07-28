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
    script_stdlib::{ScriptBytes, ScriptPath},
    script_stream::{
        CapturedStream, ScriptStream, cancel as cancel_stream, capture_after_close,
        discard_buffered, from_reader,
    },
};

const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const MAX_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_STDIN_BYTES: usize = 256 * 1024;

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
    timeout: Duration,
    capture_bytes: usize,
}

#[derive(Clone)]
pub struct ScriptChild(Arc<Mutex<ChildState>>);

impl std::fmt::Debug for ScriptChild {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Child").finish_non_exhaustive()
    }
}

struct ChildState {
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
    engine.register_get("stdout", child_stdout);
    engine.register_get("stderr", child_stderr);
    engine.register_fn("kill", child_kill);
    engine.register_fn("wait_with_output", child_wait_with_output);
    engine.register_fn("wait_with_output", child_wait_with_output_for);

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
    module.set_native_fn("command", process_command);
    module
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
    process
        .args(&command.arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let stdout = child.stdout.take().ok_or("process_stdout_unavailable")?;
    let stderr = child.stderr.take().ok_or("process_stderr_unavailable")?;
    let limit = command.capture_bytes;
    let stdout = from_reader(stdout, "bytes", limit);
    let stderr = from_reader(stderr, "bytes", limit);
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&command.stdin)
            .map_err(|error| format!("process_stdin_write: {error}"))?;
    }
    Ok(ScriptChild(Arc::new(Mutex::new(ChildState {
        child: Some(child),
        stdout,
        stderr,
        deadline: Instant::now() + command.timeout,
        completed: None,
    }))))
}

fn child_id(child: &mut ScriptChild) -> Result<rhai::INT, Box<EvalAltResult>> {
    let state = child.0.lock().map_err(|_| "process_child_state_poisoned")?;
    Ok(i64::from(
        state
            .child
            .as_ref()
            .map(Child::id)
            .ok_or("process_child_completed")?,
    ))
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
    let (stdout, stderr) = finish_capture(state, deadline)?;
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
    Err(format!(
        "child_nonzero: {code}: process exited with code {}{}",
        output.exit_code,
        if output.truncated {
            " (captured output truncated)"
        } else {
            ""
        }
    )
    .into())
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
