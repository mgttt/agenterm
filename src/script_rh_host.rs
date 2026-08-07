use std::path::Path;

use agenterm_rh::{RH_HOST_API_VERSION, RhError, RhNativeModule};

/// Fleet bridge injected into rh host eval: (operation_id, params_json) → result JSON.
pub type FleetBridgeFn = Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

#[derive(Debug)]
pub(crate) enum RhHostEntryValue {
    Unit,
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
        HOST_RUN_SCRIPT_RESULT.with(|slot| {
            slot.borrow_mut().take();
        });
        if let Some(bridge) = fleet_bridge {
            set_fleet_bridge(bridge);
        }
        let module = RhNativeModule::load(native_path)?;
        register_native_module(&module)?;
        clear_host_error();
        let entry_value = module.call_entry();
        let host_value = HOST_RUN_SCRIPT_RESULT.with(|slot| slot.borrow_mut().take());
        if let Some(error) = take_host_error() {
            return Err(error);
        }
        Ok(RhPackEntryResult {
            entry_value,
            host_value,
        })
    })
}

pub fn register_native_module(module: &RhNativeModule) -> Result<(), RhError> {
    let api = module.host_api_version();
    if api >= RH_HOST_API_VERSION {
        module.register_host_v9(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
            Some(host_utility_call),
        )
    } else if api >= 7 {
        module.register_host_v7(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
            Some(host_fs_read_call),
        )
    } else if api >= 6 {
        module.register_host_v6(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
            Some(host_arg_call),
        )
    } else if api >= 5 {
        module.register_host_v5(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
            Some(host_std_fs_exists_call),
            Some(host_args_len_call),
        )
    } else if api >= 4 {
        module.register_host_v4(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
            Some(host_std_fs_exists_call),
        )
    } else if api >= 3 {
        module.register_host_v3(
            host_fleet_call,
            Some(host_eval_call),
            Some(host_run_script_call),
        )
    } else if api >= 2 {
        module.register_host_v2(host_fleet_call, Some(host_eval_call))
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
            if let Some(capture) = crate::script_rh_run::current_run_context()
                .and_then(|context| context.output_capture)
            {
                capture.push_line(&input);
            }
            0
        }
        _ => {
            record_host_error("rh_utility", &format!("unknown operation {operation}"));
            -5
        }
    }
}

fn host_process_request(
    request: &str,
) -> Result<(String, Vec<String>, u64, Option<String>), String> {
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
    Ok((program, arguments, timeout_ms, stdout_path))
}

fn host_process_status(request: &str) -> Result<i32, String> {
    let (program, arguments, timeout_ms, _) = host_process_request(request)?;
    let code = crate::script_process::command_status(&program, &arguments, timeout_ms)?;
    i32::try_from(code).map_err(|_| format!("process_exit_code_overflow: {code}"))
}

fn host_process_stdout_file(request: &str) -> Result<i32, String> {
    let (program, arguments, timeout_ms, stdout_path) = host_process_request(request)?;
    let stdout_path =
        stdout_path.ok_or_else(|| "process_stdout_path_type: expected string".to_owned())?;
    let code = crate::script_process::run_command_stdout_file(
        &program,
        &arguments,
        timeout_ms,
        &stdout_path,
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

extern "C" fn host_run_script_call(
    source: *const u8,
    source_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32 {
    if source.is_null() || out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let source = match unsafe { read_utf8(source, source_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    let response = host_run_script_source(&source);
    if let Err(error) = &response {
        record_host_error("rh_host_run_script", error);
    }
    let response = response.map_err(|_| -5);
    if let Ok(json) = &response
        && let Some(value) = decode_host_entry_value(json)
    {
        HOST_RUN_SCRIPT_RESULT.with(|slot| {
            *slot.borrow_mut() = Some(value);
        });
    }
    write_response(response, out_buf, out_cap)
}

fn decode_host_entry_value(json: &str) -> Option<RhHostEntryValue> {
    let encoded: serde_json::Value = serde_json::from_str(json).ok()?;
    match encoded.get("kind").and_then(serde_json::Value::as_str)? {
        "unit" => Some(RhHostEntryValue::Unit),
        "int" | "bool" | "str" | "json" => {
            encoded.get("value").cloned().map(RhHostEntryValue::Value)
        }
        _ => None,
    }
}

fn host_run_script_source(source: &str) -> Result<String, String> {
    use rhai::{Dynamic, Engine, Scope};

    let context = crate::script_rh_run::current_run_context().unwrap_or_default();
    let project_root = context
        .project_root
        .clone()
        .or_else(|| {
            std::env::var("AGENTERM_WORKSPACE_PATH")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "project root unavailable".to_owned())?;
    let budgets = context.budgets.clone().unwrap_or_default();
    let mut engine = Engine::new();
    crate::script_runtime::configure_engine(&mut engine, &budgets);
    capture_compat_print(&mut engine, &context);
    let resolver = crate::script_project::ProjectModuleResolver::new(&project_root)
        .map_err(|error| error.to_string())?;
    engine.set_module_resolver(resolver);
    let mut scope = Scope::new();
    if let Some(arguments) = context.arguments {
        let dynamic = rhai::serde::to_dynamic(&arguments).map_err(|error| error.to_string())?;
        scope.push_dynamic("args", dynamic);
    }
    let ast = engine
        .compile_into_self_contained(&scope, source)
        .map_err(|error| error.to_string())?;
    let has_entry = ast.iter_functions().any(|meta| meta.name == "entry");
    let value = if has_entry {
        engine
            .call_fn::<Dynamic>(&mut scope, &ast, "entry", ())
            .map_err(|error| error.to_string())?
    } else {
        engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
            .map_err(|error| error.to_string())?
    };
    encode_host_result(&value)
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

extern "C" fn host_eval_call(
    snippet: *const u8,
    snippet_len: u32,
    scope_json: *const u8,
    scope_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32 {
    if snippet.is_null() || scope_json.is_null() || out_buf.is_null() || out_cap == 0 {
        return -1;
    }
    let snippet = match unsafe { read_utf8(snippet, snippet_len) } {
        Ok(value) => value,
        Err(()) => return -2,
    };
    let scope_json = match unsafe { read_utf8(scope_json, scope_json_len) } {
        Ok(value) => value,
        Err(()) => return -3,
    };
    let response = host_eval_snippet(&snippet, &scope_json);
    if let Err(error) = &response {
        record_host_error("rh_host_eval", error);
    }
    let response = response.map_err(|_| -5);
    write_response(response, out_buf, out_cap)
}

fn clear_host_error() {
    HOST_ERROR.with(|slot| {
        slot.borrow_mut().take();
    });
}

fn record_host_error(code: &'static str, detail: &str) {
    HOST_ERROR.with(|slot| {
        let mut error = slot.borrow_mut();
        if error.is_none() {
            *error = Some(RhError::Compile(format!("{code}: {detail}")));
        }
    });
}

fn take_host_error() -> Option<RhError> {
    HOST_ERROR.with(|slot| slot.borrow_mut().take())
}

fn host_eval_snippet(snippet: &str, scope_json: &str) -> Result<String, String> {
    use rhai::{Dynamic, Engine, Scope};

    let context = crate::script_rh_run::current_run_context().unwrap_or_default();
    let scope_value: serde_json::Value =
        serde_json::from_str(scope_json).unwrap_or_else(|_| serde_json::json!({}));
    let mut engine = Engine::new();
    let budgets = context.budgets.clone().unwrap_or_default();
    crate::script_runtime::configure_engine(&mut engine, &budgets);
    capture_compat_print(&mut engine, &context);
    if let Some(project_root) = context
        .project_root
        .or_else(|| {
            std::env::var("AGENTERM_WORKSPACE_PATH")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .or_else(|| std::env::current_dir().ok())
    {
        if let Ok(resolver) = crate::script_project::ProjectModuleResolver::new(&project_root) {
            engine.set_module_resolver(resolver);
        }
    }
    let mut scope = Scope::new();
    if let Some(arguments) = context.arguments {
        let dynamic = rhai::serde::to_dynamic(&arguments).map_err(|error| error.to_string())?;
        scope.push_dynamic("args", dynamic);
    }
    if let Some(vars) = scope_value.get("vars").and_then(|value| value.as_object()) {
        for (name, binding) in vars {
            if let Some(value) = binding.get("value") {
                if let Some(number) = value.as_i64() {
                    scope.push(name.as_str(), number);
                    continue;
                }
                if let Some(flag) = value.as_bool() {
                    scope.push(name.as_str(), flag);
                    continue;
                }
                if let Some(text) = value.as_str() {
                    scope.push(name.as_str(), text.to_owned());
                }
            }
        }
    }
    let result = engine
        .eval_with_scope::<Dynamic>(&mut scope, snippet)
        .map_err(|error| error.to_string())?;
    encode_host_result(&result)
}

fn capture_compat_print(engine: &mut rhai::Engine, context: &crate::script_rh_run::RhRunContext) {
    let capture = context.output_capture.clone();
    engine.on_print(move |text| {
        if let Some(capture) = &capture {
            capture.push_line(text);
        }
    });
}

fn encode_host_result(value: &rhai::Dynamic) -> Result<String, String> {
    if value.is_unit() {
        return Ok("{\"kind\":\"unit\"}".to_owned());
    }
    if let Ok(number) = value.as_int() {
        return Ok(format!("{{\"kind\":\"int\",\"value\":{number}}}"));
    }
    if let Ok(flag) = value.as_bool() {
        return Ok(format!("{{\"kind\":\"bool\",\"value\":{flag}}}"));
    }
    if let Ok(text) = value.clone().into_string() {
        return Ok(format!(
            "{{\"kind\":\"str\",\"value\":{}}}",
            serde_json::to_string(&text).map_err(|error| error.to_string())?
        ));
    }
    if let Ok(json) = rhai::serde::from_dynamic::<serde_json::Value>(value) {
        return Ok(format!("{{\"kind\":\"json\",\"value\":{json}}}"));
    }
    Err("unsupported host eval result type".to_owned())
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
    static HOST_RUN_SCRIPT_RESULT: std::cell::RefCell<Option<RhHostEntryValue>> =
        const { std::cell::RefCell::new(None) };
    static HOST_ERROR: std::cell::RefCell<Option<RhError>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::{call_pack_entry_with_host, clear_fleet_bridge, host_eval_snippet};
    use std::sync::{Arc, Mutex};

    #[test]
    fn host_eval_runs_std_fs_exists() {
        let dir = std::env::temp_dir();
        let snippet = format!("std::fs::exists(`{}`)", dir.display());
        let json = host_eval_snippet(&snippet, "{}").expect("eval");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["kind"], "bool");
        assert!(value["value"].as_bool().unwrap());
    }

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
        let source = r#"import "scripts/rhai/lib/build_identity" as build_identity;
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
    fn rhai_self_contained_entry_returns_entry_value() {
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
    fn rhai_self_contained_entry_sees_args() {
        use rhai::{Dynamic, Engine, Scope};
        let engine = Engine::new();
        let mut scope = Scope::new();
        scope.push_dynamic(
            "args",
            rhai::serde::to_dynamic(&vec!["alpha".to_owned(), "beta".to_owned()]).expect("args"),
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
    fn host_run_script_call_ffi_honors_args() {
        let context = crate::script_rh_run::RhRunContext {
            arguments: Some(serde_json::json!(["alpha", "beta"])),
            project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            ..Default::default()
        };
        let source = "fn entry() { args.len() }";
        crate::script_rh_run::with_run_context(context, || {
            let mut out = vec![0u8; 256];
            let wrote = super::host_run_script_call(
                source.as_ptr(),
                source.len() as u32,
                out.as_mut_ptr(),
                out.len() as u32,
            );
            assert!(wrote > 0, "host_run_script_call failed with {wrote}");
            let json = std::str::from_utf8(&out[..wrote as usize]).expect("utf8");
            assert_eq!(json, "{\"kind\":\"int\",\"value\":2}");
        });
    }

    #[test]
    fn host_eval_snippet_honors_args() {
        let context = crate::script_rh_run::RhRunContext {
            arguments: Some(serde_json::json!(["alpha", "beta"])),
            ..Default::default()
        };
        let json = crate::script_rh_run::with_run_context(context, || {
            super::host_eval_snippet("args.len()", "{}").expect("eval")
        });
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["kind"], "int");
        assert_eq!(value["value"], 2);
    }

    #[test]
    fn host_run_script_source_honors_args() {
        let context = crate::script_rh_run::RhRunContext {
            arguments: Some(serde_json::json!(["alpha", "beta"])),
            project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            ..Default::default()
        };
        let source = "fn entry() { args.len() }";
        let err = crate::script_rh_run::with_run_context(context, || {
            super::host_run_script_source(source).map_err(|error| error.to_string())
        });
        assert_eq!(err.as_deref(), Ok("{\"kind\":\"int\",\"value\":2}"));
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
