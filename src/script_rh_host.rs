use std::path::Path;

use agenterm_rh::{RH_HOST_API_VERSION, RhError, RhNativeModule};

/// Fleet bridge injected into rh host eval: (operation_id, params_json) → result JSON.
pub type FleetBridgeFn = Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

#[derive(Debug)]
pub(crate) enum RhHostEntryValue {
    // Constructed-by-nobody today: the typed entry-value channel is staged
    // ahead of its AOT-pack consumer. Graybox inventory
    // plan/design-binary-size-and-reuse.md §5.3 holds the delete-by
    // condition; wiring either variant removes its attribute.
    #[expect(dead_code, reason = "typed entry-value channel staged for AOT wiring")]
    Unit,
    #[expect(dead_code, reason = "typed entry-value channel staged for AOT wiring")]
    Value(serde_json::Value),
}

#[derive(Debug)]
pub(crate) struct RhPackEntryResult {
    pub entry_value: i64,
    pub host_value: Option<RhHostEntryValue>,
}

pub fn broker_fleet_bridge<F>(call: F) -> FleetBridgeFn
where
    F: Fn(&str, serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
{
    Box::new(move |operation_id, params| {
        let parameters = serde_json::from_str(params).unwrap_or_else(|_| serde_json::json!({}));
        call(
            "fleet.call",
            serde_json::json!({
                "operation_id": operation_id,
                "parameters": parameters,
            }),
        )
        .map(|value| value.to_string())
    })
}

pub fn set_fleet_bridge(bridge: FleetBridgeFn) {
    FLEET_BRIDGE.with(|slot| {
        *slot.borrow_mut() = Some(bridge);
    });
}

pub fn clear_fleet_bridge() {
    FLEET_BRIDGE.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// Run a pack's entry with the host callbacks registered and no fleet bridge.
/// The pack probes use this; loading a pack must never execute it.
pub fn call_pack_entry_with_host_registration(native_path: &Path) -> Result<i64, RhError> {
    crate::script_rh_run::with_run_context(crate::script_rh_run::RhRunContext::default(), || {
        let module = RhNativeModule::load(native_path)?;
        register_native_module(&module)?;
        clear_host_error();
        let entry_value = module.call_entry();
        take_host_error().map_or(Ok(entry_value), Err)
    })
}

/// Same as [`call_pack_entry_with_host_registration`], but also returns whatever
/// the script `print`ed.
///
/// `print` is deliberately not written straight to stdout by the host utility:
/// in the worker protocol stdout carries the JSON result, so a script's output
/// would corrupt it. That is why the utility only writes to the run context's
/// capture (or to stderr under `AGENTERM_SCRIPT_WORKER_STDERR=inherit`). The
/// dev-loop commands (`rh eval`, `rh run-smoke`) have no such constraint and
/// *should* show the output -- without this they silently swallowed every
/// `print`, which is the opposite of what a debugging command is for.
pub fn call_pack_entry_capturing_output(native_path: &Path) -> Result<(i64, String), RhError> {
    let capture = std::sync::Arc::new(crate::script_rh_run::RhOutputCapture::new(1 << 20));
    let context = crate::script_rh_run::RhRunContext {
        output_capture: Some(std::sync::Arc::clone(&capture)),
        ..Default::default()
    };
    let entry_value = crate::script_rh_run::with_run_context(context, || {
        let module = RhNativeModule::load(native_path)?;
        register_native_module(&module)?;
        clear_host_error();
        let entry_value = module.call_entry();
        take_host_error().map_or(Ok(entry_value), Err)
    })?;
    Ok((entry_value, capture.finish()?))
}

pub fn call_cached_pack_entry_with_fleet(bridge: FleetBridgeFn) -> Result<i64, RhError> {
    let native_path = crate::script_rh_pack::cached_native_path()
        .ok_or_else(|| RhError::Compile("AGENTERM_RH_PACK native path is unavailable".into()))?;
    call_pack_entry_with_fleet(
        native_path,
        bridge,
        crate::script_rh_run::RhRunContext::default(),
    )
}

pub fn call_pack_entry_with_fleet(
    native_path: &Path,
    bridge: FleetBridgeFn,
    context: crate::script_rh_run::RhRunContext,
) -> Result<i64, RhError> {
    crate::script_rh_run::with_run_context(context, || {
        set_fleet_bridge(bridge);
        let module = RhNativeModule::load(native_path)?;
        register_native_module(&module)?;
        clear_host_error();
        let entry_value = module.call_entry();
        take_host_error().map_or(Ok(entry_value), Err)
    })
}

pub fn call_pack_entry_with_host(
    native_path: &Path,
    fleet_bridge: Option<FleetBridgeFn>,
    context: crate::script_rh_run::RhRunContext,
) -> Result<i64, RhError> {
    call_pack_entry_with_host_result(native_path, fleet_bridge, context)
        .map(|result| result.entry_value)
}

pub(crate) fn call_pack_entry_with_host_result(
    native_path: &Path,
    fleet_bridge: Option<FleetBridgeFn>,
    context: crate::script_rh_run::RhRunContext,
) -> Result<RhPackEntryResult, RhError> {
    crate::script_rh_run::with_run_context(context, || {
        if let Some(bridge) = fleet_bridge {
            set_fleet_bridge(bridge);
        }
        let module = RhNativeModule::load(native_path)?;
        register_native_module(&module)?;
        clear_host_error();
        let entry_value = module.call_entry();
        if let Some(error) = take_host_error() {
            return Err(error);
        }
        Ok(RhPackEntryResult {
            entry_value,
            host_value: None,
        })
    })
}

pub fn register_native_module(module: &RhNativeModule) -> Result<(), RhError> {
    let api = module.host_api_version();
    // A pack below the host's api gets `None` for some callbacks, and
    // `RhNativeModule::register_host_*` substitutes DUMMIES for those: the
    // dummy `args_len` returns -4 and the dummy utility call swallows `print`
    // and `rh_fail`. A script then sees `args.len == -4`, silently misses every
    // branch that compares it, and runs a completely different path with no
    // error anywhere -- which is exactly how `fresh-clone-rehearsal --self-test`
    // came to run a full clone-and-build inside a unit test. The pack cache key
    // already pins the api version, so a mismatch here is a stale artifact or a
    // bug, never a normal condition: say so instead of degrading in silence.
    if api < RH_HOST_API_VERSION {
        eprintln!(
            "rh pack host api {api} is older than this host's {RH_HOST_API_VERSION}; \
             callbacks above that version are stubs (args.len reads -4, print is dropped)"
        );
    }
    if api >= RH_HOST_API_VERSION {
        module.register_host_v10(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
            Some(host_utility_call),
            Some(host_json_call),
        )
    } else if api >= 9 {
        module.register_host_v9(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
            Some(host_utility_call),
        )
    } else if api >= 8 {
        module.register_host_v8(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
            Some(host_utility_call),
        )
    } else if api >= 7 {
        module.register_host_v7(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
        )
    } else if api >= 6 {
        module.register_host_v6(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
        )
    } else if api >= 5 {
        module.register_host_v5(
            host_fleet_call,
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
        )
    } else if api >= 4 {
        module.register_host_v4(host_fleet_call, Some(host_std_fs_exists_call))
    } else if api >= 3 {
        module.register_host_v3(host_fleet_call, None)
    } else if api >= 2 {
        module.register_host_v2(host_fleet_call)
    } else {
        Err(RhError::Compile(format!(
            "rh pack host api {api} is older than the minimum supported version 2"
        )))
    }
}

extern "C" fn host_utility_call(operation: u32, input: *const u8, input_len: u32) -> i32 {
    if input.is_null() {
        return -1;
    }
    let input = match unsafe { read_utf8(input, input_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    match operation {
        agenterm_rh::RH_HOST_UTILITY_FAIL => {
            record_host_error("rh_fail", &input);
            -5
        }
        agenterm_rh::RH_HOST_UTILITY_EXISTS_CASE_EXACT => {
            match path_exists_case_exact(Path::new(&input)) {
                Ok(true) => 1,
                Ok(false) => 0,
                Err(error) => {
                    record_host_error("rh_std_fs_exists_case_exact", &error.to_string());
                    -5
                }
            }
        }
        agenterm_rh::RH_HOST_UTILITY_PROCESS_STATUS => match host_process_status(&input) {
            Ok(code) => code,
            Err(error) => {
                record_host_error("rh_process_status", &error);
                -5
            }
        },
        agenterm_rh::RH_HOST_UTILITY_PROCESS_STDOUT_FILE => {
            match host_process_stdout_file(&input) {
                Ok(code) => code,
                Err(error) => {
                    record_host_error("rh_process_stdout_file", &error);
                    -5
                }
            }
        }
        agenterm_rh::RH_HOST_UTILITY_PRINT => {
            emit_host_trace(&input);
            0
        }
        _ => {
            record_host_error("rh_utility", &format!("unknown operation {operation}"));
            -5
        }
    }
}

/// Parsed process-launch request: `(program, args, timeout_ms, stdin,
/// options)` — named so the signature stays readable at the call sites and
/// the clippy type-complexity gate stays quiet.
type ProcessRequest = (
    String,
    Vec<String>,
    u64,
    Option<String>,
    crate::script_process::ProcessHostOptions,
);

fn host_process_request(request: &str) -> Result<ProcessRequest, String> {
    let request: serde_json::Value =
        serde_json::from_str(request).map_err(|error| format!("process_request_json: {error}"))?;
    let program = request["program"]
        .as_str()
        .ok_or_else(|| "process_program_type: expected string".to_owned())?
        .to_owned();
    let arguments = request["args"]
        .as_array()
        .ok_or_else(|| "process_arguments_type: expected array".to_owned())?;
    if arguments.len() > 256 {
        return Err("process_arguments_too_many: maximum is 256".to_owned());
    }
    let arguments = arguments
        .iter()
        .map(|argument| {
            let argument = argument
                .as_str()
                .ok_or_else(|| "process_arguments_type: expected strings".to_owned())?;
            if argument.len() > 4096 {
                return Err("process_argument_too_large: maximum is 4096 bytes".to_owned());
            }
            Ok(argument.to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let requested_timeout = request["timeout_ms"]
        .as_u64()
        .ok_or_else(|| "process_timeout_type: expected positive integer".to_owned())?;
    let timeout_ms = crate::script_rh_run::current_run_context()
        .and_then(|context| context.budgets)
        .map_or(requested_timeout, |budgets| {
            requested_timeout.min(budgets.wall_time_ms)
        });
    let stdout_path = match request.get("stdout_path") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => {
            let path = value
                .as_str()
                .ok_or_else(|| "process_stdout_path_type: expected string".to_owned())?;
            if path.is_empty() {
                return Err("process_stdout_path_empty".to_owned());
            }
            if path.len() > 4096 {
                return Err("process_stdout_path_too_large: maximum is 4096 bytes".to_owned());
            }
            Some(path.to_owned())
        }
    };
    let mut options = crate::script_process::ProcessHostOptions::default();
    if let Some(current_dir) = request.get("current_dir") {
        let current_dir = current_dir
            .as_str()
            .ok_or_else(|| "process_current_dir_type: expected string".to_owned())?;
        if current_dir.is_empty() {
            return Err("process_current_dir_empty".to_owned());
        }
        if current_dir.len() > 4096 {
            return Err("process_current_dir_too_large: maximum is 4096 bytes".to_owned());
        }
        options.current_dir = Some(std::path::PathBuf::from(current_dir));
    }
    if let Some(env) = request.get("env") {
        let env = env
            .as_object()
            .ok_or_else(|| "process_env_type: expected object".to_owned())?;
        if env.len() > 256 {
            return Err("process_env_too_many: maximum is 256 entries".to_owned());
        }
        for (name, value) in env {
            if name.len() > 256 {
                return Err("process_env_name_too_large: maximum is 256 bytes".to_owned());
            }
            let value = value
                .as_str()
                .ok_or_else(|| "process_env_type: expected string values".to_owned())?;
            if value.len() > 4096 {
                return Err("process_env_value_too_large: maximum is 4096 bytes".to_owned());
            }
            options
                .environment
                .insert(name.clone(), Some(value.to_owned()));
        }
    }
    if let Some(removes) = request.get("env_remove") {
        let removes = removes
            .as_array()
            .ok_or_else(|| "process_env_remove_type: expected array".to_owned())?;
        if removes.len() > 256 {
            return Err("process_env_remove_too_many: maximum is 256 entries".to_owned());
        }
        for name in removes {
            let name = name
                .as_str()
                .ok_or_else(|| "process_env_remove_type: expected strings".to_owned())?;
            if name.len() > 256 {
                return Err("process_env_remove_name_too_large: maximum is 256 bytes".to_owned());
            }
            options.environment.insert(name.to_owned(), None);
        }
    }
    Ok((program, arguments, timeout_ms, stdout_path, options))
}

fn host_process_status(request: &str) -> Result<i32, String> {
    let (program, arguments, timeout_ms, _, options) = host_process_request(request)?;
    let code = crate::script_process::command_status(&program, &arguments, timeout_ms, &options)?;
    i32::try_from(code).map_err(|_| format!("process_exit_code_overflow: {code}"))
}

fn host_process_stdout_file(request: &str) -> Result<i32, String> {
    let (program, arguments, timeout_ms, stdout_path, options) = host_process_request(request)?;
    let stdout_path =
        stdout_path.ok_or_else(|| "process_stdout_path_type: expected string".to_owned())?;
    let code = crate::script_process::run_command_stdout_file(
        &program,
        &arguments,
        timeout_ms,
        &stdout_path,
        &options,
    )?;
    i32::try_from(code).map_err(|_| format!("process_exit_code_overflow: {code}"))
}

fn path_exists_case_exact(path: &Path) -> std::io::Result<bool> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    let Some(expected_name) = path.file_name() else {
        return Ok(false);
    };
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        if entry.file_name() == expected_name {
            return entry.file_type().map(|kind| kind.is_file());
        }
    }
    Ok(false)
}

extern "C" fn host_fs_read_call(
    path: *const u8,
    path_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32 {
    if path.is_null() || out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let path = match unsafe { read_utf8(path, path_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            record_host_error("rh_std_fs_read_to_string", &error.to_string());
            return -5;
        }
    };
    let bytes = content.as_bytes();
    if bytes.len() > out_cap as usize {
        record_host_error(
            "rh_std_fs_read_to_string",
            "file exceeds the host output buffer",
        );
        return -3;
    }
    let Ok(length) = i32::try_from(bytes.len()) else {
        record_host_error("rh_std_fs_read_to_string", "file length exceeds i32");
        return -3;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    length
}

extern "C" fn host_arg_call(index: u32, out_buf: *mut u8, out_cap: u32) -> i32 {
    if out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let Some(context) = crate::script_rh_run::current_run_context() else {
        record_host_error("rh_arg", "run context is unavailable");
        return -5;
    };
    let Some(arguments) = context
        .arguments
        .and_then(|value| value.as_array().cloned())
    else {
        record_host_error("rh_arg", "run context arguments must be an array");
        return -5;
    };
    let Some(argument) = arguments
        .get(index as usize)
        .and_then(|value| value.as_str())
    else {
        record_host_error(
            "rh_arg",
            &format!("argument {index} is unavailable or not a string"),
        );
        return -5;
    };
    let bytes = argument.as_bytes();
    if bytes.len() > out_cap as usize {
        record_host_error("rh_arg", "argument exceeds the host output buffer");
        return -3;
    }
    let Ok(length) = i32::try_from(bytes.len()) else {
        record_host_error("rh_arg", "argument length exceeds i32");
        return -3;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    length
}

extern "C" fn host_args_len_call() -> i64 {
    let Some(context) = crate::script_rh_run::current_run_context() else {
        return 0;
    };
    let Some(arguments) = context.arguments else {
        return 0;
    };
    let Some(arguments) = arguments.as_array() else {
        record_host_error("rh_args_len", "run context arguments must be an array");
        return -5;
    };
    match i64::try_from(arguments.len()) {
        Ok(length) => length,
        Err(error) => {
            record_host_error("rh_args_len", &error.to_string());
            -5
        }
    }
}

extern "C" fn host_std_fs_exists_call(path: *const u8, path_len: u32) -> i32 {
    if path.is_null() {
        return -1;
    }
    let path = match unsafe { read_utf8(path, path_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    match Path::new(&path).try_exists() {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(error) => {
            record_host_error("rh_std_fs_exists", &error.to_string());
            -5
        }
    }
}

extern "C" fn host_fleet_call(
    operation_id: *const u8,
    operation_id_len: u32,
    params_json: *const u8,
    params_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32 {
    if operation_id.is_null() || params_json.is_null() || out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let operation_id = match unsafe { read_utf8(operation_id, operation_id_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    let params_json = match unsafe { read_utf8(params_json, params_json_len) } {
        Ok(value) => value,
        Err(()) => return -3,
    };
    let response: Result<String, i32> = FLEET_BRIDGE.with(|slot| {
        let guard = slot.borrow();
        let Some(bridge) = guard.as_ref() else {
            return Err(-4);
        };
        bridge(operation_id.as_str(), params_json.as_str()).map_err(|_| -5)
    });
    write_response(response, out_buf, out_cap)
}

fn json_pid(input: &serde_json::Value) -> Result<u32, i32> {
    input
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or(-5)
}

fn json_i32_field(input: &serde_json::Value, field: &str) -> Result<i32, i32> {
    input
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(-5)
}

extern "C" fn host_json_call(
    operation: *const u8,
    operation_len: u32,
    input_json: *const u8,
    input_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32 {
    if operation.is_null() || input_json.is_null() || out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let operation = match unsafe { read_utf8(operation, operation_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    let input: serde_json::Value = match unsafe { read_utf8(input_json, input_json_len) }
        .and_then(|value| serde_json::from_str(&value).map_err(|_| ()))
    {
        Ok(value) => value,
        Err(()) => return -3,
    };
    let response = match operation.as_str() {
        "process.platform_facts" => input
            .get("pid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .map(crate::script_process::process_platform_facts_json)
            .ok_or(-5),
        "process.list" => crate::script_process::process_list_json().map_err(|_| -5),
        "crypto.tree_metadata_digest" => input
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or(-5)
            .map(|path| crate::incremental_wrapper::tree_metadata_digest_json(Path::new(path))),
        "image.inspect_png" => input
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or(-5)
            .and_then(|path| {
                crate::script_image::inspect_png_json(Path::new(path)).map_err(|_| -5)
            }),
        "process.window_key" => {
            use crate::platform::contract::script_window::ScriptWindowKey;

            let pid = input
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or(-5);
            let key = input
                .get("key")
                .and_then(serde_json::Value::as_str)
                .and_then(|key| match key {
                    "Backspace" => Some(ScriptWindowKey::Backspace),
                    "Delete" => Some(ScriptWindowKey::Delete),
                    "Down" => Some(ScriptWindowKey::Down),
                    "End" => Some(ScriptWindowKey::End),
                    "Enter" => Some(ScriptWindowKey::Enter),
                    "Escape" => Some(ScriptWindowKey::Escape),
                    "F2" => Some(ScriptWindowKey::F2),
                    "Home" => Some(ScriptWindowKey::Home),
                    "Left" => Some(ScriptWindowKey::Left),
                    "Right" => Some(ScriptWindowKey::Right),
                    "Tab" => Some(ScriptWindowKey::Tab),
                    "Up" => Some(ScriptWindowKey::Up),
                    _ => None,
                })
                .ok_or(-5);
            match (pid, key) {
                (Ok(pid), Ok(key)) => crate::platform::services::script_window::key(pid, key)
                    .map(|()| serde_json::json!({ "ok": true }))
                    .map_err(|_| -5),
                _ => Err(-5),
            }
        }
        "process.window_control" => match (json_pid(&input), json_i32_field(&input, "id")) {
            (Ok(pid), Ok(id)) => crate::platform::services::script_window::control_exists(pid, id)
                .map(|()| serde_json::json!({ "ok": true }))
                .map_err(|_| -5),
            _ => Err(-5),
        },
        "process.window_control_visible" => {
            match (json_pid(&input), json_i32_field(&input, "id")) {
                (Ok(pid), Ok(id)) => {
                    crate::platform::services::script_window::control_visible(pid, id)
                        .map(|visible| serde_json::json!({ "visible": visible }))
                        .map_err(|_| -5)
                }
                _ => Err(-5),
            }
        }
        "process.window_control_text" => match (json_pid(&input), json_i32_field(&input, "id")) {
            (Ok(pid), Ok(id)) => crate::platform::services::script_window::control_text(pid, id)
                .map(|text| serde_json::json!({ "text": text }))
                .map_err(|_| -5),
            _ => Err(-5),
        },
        "process.window_control_set_text" => {
            match (
                json_pid(&input),
                json_i32_field(&input, "id"),
                input.get("text").and_then(serde_json::Value::as_str),
            ) {
                (Ok(pid), Ok(id), Some(text)) => {
                    crate::platform::services::script_window::control_set_text(pid, id, text)
                        .map(|()| serde_json::json!({ "ok": true }))
                        .map_err(|_| -5)
                }
                _ => Err(-5),
            }
        }
        "process.window_control_click" => match (json_pid(&input), json_i32_field(&input, "id")) {
            (Ok(pid), Ok(id)) => crate::platform::services::script_window::control_click(pid, id)
                .map(|()| serde_json::json!({ "ok": true }))
                .map_err(|_| -5),
            _ => Err(-5),
        },
        "process.window_message" => {
            use crate::platform::contract::script_window::ScriptWindowMessage;

            let pid = json_pid(&input);
            let message = input
                .get("message")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            let wparam = input
                .get("wparam")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            let lparam = input
                .get("lparam")
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| isize::try_from(value).ok());
            match (pid, message, wparam, lparam) {
                (Ok(pid), Some(message), Some(wparam), Some(lparam)) => {
                    crate::platform::services::script_window::message(
                        pid,
                        ScriptWindowMessage {
                            message,
                            wparam,
                            lparam,
                        },
                    )
                    .map(|value| serde_json::json!({ "value": value }))
                    .map_err(|_| -5)
                }
                _ => Err(-5),
            }
        }
        "process.window_pointer" => {
            use crate::platform::contract::script_window::ScriptWindowPointerAction;

            let pid = json_pid(&input);
            let action = input
                .get("action")
                .and_then(serde_json::Value::as_str)
                .and_then(|action| match action {
                    "click" => Some(ScriptWindowPointerAction::Click),
                    "down" => Some(ScriptWindowPointerAction::Down),
                    "move" => Some(ScriptWindowPointerAction::Move),
                    "move-held" => Some(ScriptWindowPointerAction::MoveHeld),
                    "up" => Some(ScriptWindowPointerAction::Up),
                    "capture-changed" => Some(ScriptWindowPointerAction::CaptureChanged),
                    _ => None,
                });
            let x = json_i32_field(&input, "x");
            let y = json_i32_field(&input, "y");
            match (pid, action, x, y) {
                (Ok(pid), Some(action), Ok(x), Ok(y)) => {
                    crate::platform::services::script_window::pointer(pid, action, x, y)
                        .map(|()| serde_json::json!({ "ok": true }))
                        .map_err(|_| -5)
                }
                _ => Err(-5),
            }
        }
        "process.window_resize" => match (
            json_pid(&input),
            json_i32_field(&input, "width"),
            json_i32_field(&input, "height"),
        ) {
            (Ok(pid), Ok(width), Ok(height)) if width > 0 && height > 0 => {
                crate::platform::services::script_window::resize(pid, width, height)
                    .map(|()| serde_json::json!({ "ok": true }))
                    .map_err(|_| -5)
            }
            _ => Err(-5),
        },
        "process.window_rect" | "process.window_client_rect" => {
            let client = operation == "process.window_client_rect";
            match json_pid(&input) {
                Ok(pid) => crate::platform::services::script_window::rect(pid, client)
                    .map(|rect| {
                        serde_json::json!({
                            "left": rect.left,
                            "top": rect.top,
                            "right": rect.right,
                            "bottom": rect.bottom,
                        })
                    })
                    .map_err(|_| -5),
                Err(code) => Err(code),
            }
        }
        // Record the typed clipboard code before collapsing to -5. Discarding it
        // left scripts with a bare `host_json_call_failed: clipboard.get_text: -5`
        // that cannot distinguish "another process holds the clipboard" from
        // "the clipboard holds no Unicode text" from "unsupported platform" --
        // three different remedies behind one number.
        "clipboard.get_text" => crate::platform::services::script_clipboard::get_text()
            .map(|text| serde_json::json!({ "text": text }))
            .map_err(|error| {
                record_host_error("rh_clipboard_get_text", &clipboard_error_detail(&error));
                -5
            }),
        "clipboard.set_text" => match input.get("text").and_then(serde_json::Value::as_str) {
            Some(text) => crate::platform::services::script_clipboard::set_text(text)
                .map(|()| serde_json::json!({ "ok": true }))
                .map_err(|error| {
                    record_host_error("rh_clipboard_set_text", &clipboard_error_detail(&error));
                    -5
                }),
            None => {
                record_host_error("rh_clipboard_set_text", "missing text field");
                Err(-5)
            }
        },
        _ => Err(-4),
    }
    .and_then(|value| serde_json::to_string(&value).map_err(|_| -5));
    write_response(response, out_buf, out_cap)
}

/// `code(cause): message` for a clipboard failure.
fn clipboard_error_detail(
    error: &crate::platform::contract::script_clipboard::ScriptClipboardError,
) -> String {
    match error.cause {
        Some(cause) => format!("{}({cause}): {}", error.code, error.message),
        None => format!("{}: {}", error.code, error.message),
    }
}

fn clear_host_error() {
    HOST_ERROR.with(|slot| {
        slot.borrow_mut().take();
    });
    HOST_ERROR_COUNT.with(|slot| slot.set(0));
}

fn record_host_error(code: &'static str, detail: &str) {
    let ordinal = HOST_ERROR_COUNT.with(|slot| {
        let next = slot.get().saturating_add(1);
        slot.set(next);
        next
    });
    // Only the first failure survives to the caller, because `require` records
    // and continues -- so a long script reports failure #1 and hides every one
    // after it. Trace each of them where `print` already goes, so one run
    // yields the whole ordered list next to the STEP stamps instead of costing
    // a fresh run per failure.
    emit_host_trace(&format!("RH_FAIL[{ordinal}] {code}: {detail}"));
    HOST_ERROR.with(|slot| {
        let mut error = slot.borrow_mut();
        if error.is_none() {
            *error = Some(RhError::Compile(format!("{code}: {detail}")));
        }
    });
}

/// Send one diagnostic line down the same two channels as script `print`:
/// an inherited worker stderr and the run's output capture.
fn emit_host_trace(line: &str) {
    if std::env::var_os("AGENTERM_SCRIPT_WORKER_STDERR").is_some_and(|value| value == "inherit") {
        eprintln!("{line}");
    }
    if let Some(capture) =
        crate::script_rh_run::current_run_context().and_then(|context| context.output_capture)
    {
        capture.push_line(line);
    }
}

fn take_host_error() -> Option<RhError> {
    HOST_ERROR.with(|slot| slot.borrow_mut().take())
}

fn write_response(response: Result<String, i32>, out_buf: *mut u8, out_cap: u32) -> i32 {
    let Ok(json) = response else {
        return response.unwrap_err();
    };
    let bytes = json.as_bytes();
    if bytes.len() > out_cap as usize {
        return -(bytes.len() as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    bytes.len() as i32
}

unsafe fn read_utf8(ptr: *const u8, len: u32) -> Result<String, ()> {
    if len == 0 {
        return Ok(String::new());
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(slice)
        .map(str::to_owned)
        .map_err(|_| ())
}

std::thread_local! {
    static FLEET_BRIDGE: std::cell::RefCell<Option<FleetBridgeFn>> =
        const { std::cell::RefCell::new(None) };
    static HOST_ERROR: std::cell::RefCell<Option<RhError>> =
        const { std::cell::RefCell::new(None) };
    static HOST_ERROR_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::{call_pack_entry_with_host, clear_fleet_bridge};
    use std::sync::{Arc, Mutex};

    #[test]
    fn host_std_fs_exists_call_reports_present_and_missing_paths() {
        let present = std::env::temp_dir();
        let present = present.to_string_lossy();
        assert_eq!(
            super::host_std_fs_exists_call(present.as_ptr(), present.len() as u32),
            1
        );

        let missing = present.to_string() + "/agenterm-rh-definitely-missing";
        assert_eq!(
            super::host_std_fs_exists_call(missing.as_ptr(), missing.len() as u32),
            0
        );
    }

    #[test]
    fn host_arg_call_returns_utf8_and_reports_missing_index() {
        let context = crate::script_rh_run::RhRunContext {
            arguments: Some(serde_json::json!(["αβ"])),
            ..Default::default()
        };
        crate::script_rh_run::with_run_context(context, || {
            super::clear_host_error();
            let mut output = [0_u8; 16];
            let wrote = super::host_arg_call(0, output.as_mut_ptr(), output.len() as u32);
            assert_eq!(wrote, "αβ".len() as i32);
            assert_eq!(&output[..wrote as usize], "αβ".as_bytes());

            assert_eq!(
                super::host_arg_call(1, output.as_mut_ptr(), output.len() as u32),
                -5
            );
            let error = super::take_host_error().expect("typed host error");
            assert!(error.to_string().contains("argument 1 is unavailable"));
        });
    }

    #[test]
    fn host_fs_read_call_returns_bounded_utf8_and_typed_missing_error() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let path = path.to_string_lossy();
        let mut output = vec![0_u8; agenterm_rh::RH_HOST_OUT_CAP as usize];
        let wrote = super::host_fs_read_call(
            path.as_ptr(),
            path.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
        );
        assert!(wrote > 0);
        let content = std::str::from_utf8(&output[..wrote as usize]).expect("utf8");
        assert!(content.contains("name = \"agenterm\""));

        super::clear_host_error();
        let missing = format!("{path}.missing");
        assert_eq!(
            super::host_fs_read_call(
                missing.as_ptr(),
                missing.len() as u32,
                output.as_mut_ptr(),
                output.len() as u32,
            ),
            -5
        );
        assert!(
            super::take_host_error()
                .expect("typed host error")
                .to_string()
                .contains("rh_std_fs_read_to_string")
        );
    }

    #[test]
    fn host_json_call_returns_process_platform_facts() {
        let operation = "process.platform_facts";
        let input = format!(r#"{{"pid":{}}}"#, std::process::id());
        let mut output = vec![0_u8; agenterm_rh::RH_HOST_OUT_CAP as usize];
        let wrote = super::host_json_call(
            operation.as_ptr(),
            operation.len() as u32,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
        );
        assert!(wrote > 0);
        let value: serde_json::Value =
            serde_json::from_slice(&output[..wrote as usize]).expect("platform facts JSON");
        assert!(value.get("top_level_window_supported").is_some());
        assert!(value.get("top_level_window_present").is_some());
        assert!(value.get("top_level_window_title").is_some());

        let operation = "process.list";
        let input = "{}";
        let wrote = super::host_json_call(
            operation.as_ptr(),
            operation.len() as u32,
            input.as_ptr(),
            input.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
        );
        assert!(wrote > 0);
        let processes: serde_json::Value =
            serde_json::from_slice(&output[..wrote as usize]).expect("process list JSON");
        assert!(
            processes
                .as_array()
                .is_some_and(|items| items.iter().any(|process| process
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    == Some(u64::from(std::process::id()))))
        );
    }

    #[test]
    fn host_utility_reports_failure_and_case_exact_file_names() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let file = file.to_string_lossy();
        assert_eq!(
            super::host_utility_call(
                agenterm_rh::RH_HOST_UTILITY_EXISTS_CASE_EXACT,
                file.as_ptr(),
                file.len() as u32,
            ),
            1
        );
        let wrong_case = file.replace("Cargo.toml", "cargo.toml");
        assert_eq!(
            super::host_utility_call(
                agenterm_rh::RH_HOST_UTILITY_EXISTS_CASE_EXACT,
                wrong_case.as_ptr(),
                wrong_case.len() as u32,
            ),
            0
        );

        super::clear_host_error();
        let message = "native task failure";
        assert_eq!(
            super::host_utility_call(
                agenterm_rh::RH_HOST_UTILITY_FAIL,
                message.as_ptr(),
                message.len() as u32,
            ),
            -5
        );
        assert!(
            super::take_host_error()
                .expect("typed failure")
                .to_string()
                .contains(message)
        );
    }

    #[test]
    fn every_recorded_failure_is_traced_even_though_only_the_first_is_returned() {
        let capture = std::sync::Arc::new(crate::script_rh_run::RhOutputCapture::new(4096));
        let context = crate::script_rh_run::RhRunContext {
            output_capture: Some(capture.clone()),
            ..Default::default()
        };
        crate::script_rh_run::with_run_context(context, || {
            super::clear_host_error();
            for message in ["first failure", "second failure"] {
                assert_eq!(
                    super::host_utility_call(
                        agenterm_rh::RH_HOST_UTILITY_FAIL,
                        message.as_ptr(),
                        message.len() as u32,
                    ),
                    -5
                );
            }
            let returned = super::take_host_error().expect("typed failure");
            assert!(returned.to_string().contains("first failure"));
            assert!(!returned.to_string().contains("second failure"));
        });
        let traced = capture.finish().expect("captured trace");
        assert!(
            traced.contains("RH_FAIL[1] rh_fail: first failure"),
            "trace missing the first failure: {traced}"
        );
        assert!(
            traced.contains("RH_FAIL[2] rh_fail: second failure"),
            "the failure the returned error hides must still be traced: {traced}"
        );
    }

    #[test]
    fn host_process_status_returns_exit_code_and_typed_spawn_failure() {
        let request = serde_json::json!({
            "program": "git",
            "args": ["--version"],
            "timeout_ms": 2000
        });
        assert_eq!(
            super::host_process_status(&request.to_string()).expect("git status"),
            0
        );

        let missing = serde_json::json!({
            "program": "agenterm-definitely-missing-process",
            "args": [],
            "timeout_ms": 2000
        });
        assert!(
            super::host_process_status(&missing.to_string())
                .expect_err("missing process")
                .contains("process_spawn")
        );
    }

    #[test]
    fn host_process_stdout_file_writes_captured_stdout() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-rh-stdout-file-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let request = serde_json::json!({
            "program": "git",
            "args": ["--version"],
            "timeout_ms": 2000,
            "stdout_path": path.to_string_lossy(),
        });
        assert_eq!(
            super::host_process_stdout_file(&request.to_string()).expect("git stdout"),
            0
        );
        let text = std::fs::read_to_string(&path).expect("stdout file");
        let _ = std::fs::remove_file(&path);
        assert!(text.to_ascii_lowercase().contains("git"), "{text}");
    }

    #[test]
    fn host_process_stdout_file_honors_current_dir_and_env() {
        let repo = std::env::current_dir().expect("cwd");
        let path =
            std::env::temp_dir().join(format!("agenterm-rh-stdout-env-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let request = serde_json::json!({
            "program": "git",
            "args": ["rev-parse", "--show-prefix"],
            "timeout_ms": 2000,
            "stdout_path": path.to_string_lossy(),
            "current_dir": repo.to_string_lossy(),
            "env": {
                "AGENTERM_RH_HOST_ENV_PROBE": "marker",
            },
            "env_remove": ["THIS_ENV_SHOULD_NOT_EXIST_12345"],
        });
        assert_eq!(
            super::host_process_stdout_file(&request.to_string()).expect("git prefix"),
            0
        );
        let text = std::fs::read_to_string(&path).expect("stdout file");
        let _ = std::fs::remove_file(&path);
        assert!(
            text.trim().is_empty() || text.contains('/'),
            "expected repo-relative prefix, got {text:?}"
        );
        assert_eq!(
            std::env::var("AGENTERM_RH_HOST_ENV_PROBE"),
            Err(std::env::VarError::NotPresent)
        );
    }

    #[test]
    fn native_pack_uses_std_fs_exists_fast_path() {
        let dir =
            std::env::temp_dir().join(format!("agenterm-rh-host-exists-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        let source = format!(
            "fn entry() {{ if std::fs::exists(`{}`) {{ 42 }} else {{ 0 }} }}",
            dir.display()
        );
        agenterm_rh::build_pack_dir(&source, &dir).expect("build");
        let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
        let value =
            call_pack_entry_with_host(&native, None, crate::script_rh_run::RhRunContext::default())
                .expect("entry");
        assert_eq!(value, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_pack_calls_host_fleet_bridge() {
        let dir =
            std::env::temp_dir().join(format!("agenterm-rh-host-fleet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        agenterm_rh::build_pack_dir("fn entry() { fleet.protocol.info(); 7 }", &dir)
            .expect("build");
        let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_bridge = Arc::clone(&calls);
        let value = call_pack_entry_with_host(
            &native,
            Some(Box::new(move |operation_id, params| {
                calls_for_bridge
                    .lock()
                    .expect("calls")
                    .push((operation_id.to_owned(), params.to_owned()));
                Ok("{\"operation_id\":\"protocol.info\"}".to_owned())
            })),
            crate::script_rh_run::RhRunContext::default(),
        )
        .expect("entry");
        assert_eq!(value, 7);
        let recorded = calls.lock().expect("calls");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "protocol.info");
        clear_fleet_bridge();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_pack_runs_import_script_via_compat_delegating() {
        let dir =
            std::env::temp_dir().join(format!("agenterm-rh-host-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = r#"import "scripts/rh/lib/build_identity" as build_identity;
fn entry() { 42 }"#;
        agenterm_rh::build_pack_dir(source, &dir).expect("build");
        let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
        let value =
            call_pack_entry_with_host(&native, None, crate::script_rh_run::RhRunContext::default())
                .expect("entry");
        assert_eq!(value, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rh_self_contained_entry_returns_entry_value() {
        use rhai::{Dynamic, Engine, Scope};
        let engine = Engine::new();
        let source = "fn entry() { 42 }";
        let ast = engine
            .compile_into_self_contained(&Scope::new(), source)
            .expect("compile");
        let mut scope = Scope::new();
        let value = engine
            .call_fn::<Dynamic>(&mut scope, &ast, "entry", ())
            .expect("call entry");
        assert_eq!(value.as_int().ok(), Some(42));
    }

    #[test]
    fn rh_self_contained_entry_sees_args() {
        use rhai::{Dynamic, Engine, Scope};
        let engine = Engine::new();
        let mut scope = Scope::new();
        scope.push_dynamic(
            "args",
            rhai::serde::to_dynamic(vec!["alpha".to_owned(), "beta".to_owned()]).expect("args"),
        );
        let source = "fn entry() { args.len() }";
        let ast = engine
            .compile_into_self_contained(&scope, source)
            .expect("compile");
        let value = engine
            .call_fn::<Dynamic>(&mut scope, &ast, "entry", ())
            .expect("call entry");
        assert_eq!(value.as_int().ok(), Some(2));
    }

    #[test]
    fn native_pack_honors_run_context_arguments() {
        let dir =
            std::env::temp_dir().join(format!("agenterm-rh-host-args-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = "fn entry() { let first = args[0]; args.len() + first.len }";
        agenterm_rh::build_pack_dir(source, &dir).expect("build");
        let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
        let module = agenterm_rh::RhNativeModule::load(&native).expect("load");
        assert!(
            module.host_api_version() >= agenterm_rh::RH_HOST_API_VERSION,
            "pack host api {}",
            module.host_api_version()
        );
        let value = call_pack_entry_with_host(
            &native,
            None,
            crate::script_rh_run::RhRunContext {
                arguments: Some(serde_json::json!(["αβ", "beta"])),
                project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
                ..Default::default()
            },
        )
        .expect("entry");
        assert_eq!(value, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_pack_runs_stdlib_via_host_eval() {
        let dir = std::env::temp_dir().join(format!("agenterm-rh-host-std-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tmp = std::env::temp_dir();
        let source = format!(
            "fn entry() {{ if std::fs::exists(`{}`) {{ 42 }} else {{ 0 }} }}",
            tmp.display()
        );
        agenterm_rh::build_pack_dir(&source, &dir).expect("build");
        let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
        let value =
            call_pack_entry_with_host(&native, None, crate::script_rh_run::RhRunContext::default())
                .expect("entry");
        assert_eq!(value, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
