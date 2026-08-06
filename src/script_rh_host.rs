use std::path::Path;

use agenterm_rh::{RH_HOST_API_VERSION, RhError, RhNativeModule};

/// Fleet bridge injected into rh host eval: (operation_id, params_json) → result JSON.
pub type FleetBridgeFn = Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

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
    call_pack_entry_with_fleet(native_path, bridge)
}

pub fn call_pack_entry_with_fleet(
    native_path: &Path,
    bridge: FleetBridgeFn,
) -> Result<i64, RhError> {
    set_fleet_bridge(bridge);
    let module = RhNativeModule::load(native_path)?;
    register_native_module(&module)?;
    Ok(module.call_entry())
}

pub fn call_pack_entry_with_host(
    native_path: &Path,
    fleet_bridge: Option<FleetBridgeFn>,
) -> Result<i64, RhError> {
    if let Some(bridge) = fleet_bridge {
        set_fleet_bridge(bridge);
    }
    let module = RhNativeModule::load(native_path)?;
    register_native_module(&module)?;
    Ok(module.call_entry())
}

pub fn register_native_module(module: &RhNativeModule) -> Result<(), RhError> {
    let api = module.host_api_version();
    if api >= RH_HOST_API_VERSION {
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
    let response = host_run_script_source(&source).map_err(|_| -5);
    write_response(response, out_buf, out_cap)
}

fn host_run_script_source(source: &str) -> Result<String, String> {
    use rhai::{Dynamic, Engine, Scope};

    let project_root = std::env::var("AGENTERM_WORKSPACE_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "project root unavailable".to_owned())?;
    let mut engine = Engine::new();
    crate::script_runtime::configure_engine(
        &mut engine,
        &crate::script_protocol::ScriptBudgets::default(),
    );
    let resolver = crate::script_project::ProjectModuleResolver::new(&project_root)
        .map_err(|error| error.to_string())?;
    engine.set_module_resolver(resolver);
    let mut scope = Scope::new();
    let ast = engine
        .compile_into_self_contained(&scope, source)
        .map_err(|error| error.to_string())?;
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| error.to_string())?;
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
    let response = host_eval_snippet(&snippet, &scope_json).map_err(|_| -5);
    write_response(response, out_buf, out_cap)
}

fn host_eval_snippet(snippet: &str, scope_json: &str) -> Result<String, String> {
    use rhai::{Dynamic, Engine, Scope};

    let scope_value: serde_json::Value =
        serde_json::from_str(scope_json).unwrap_or_else(|_| serde_json::json!({}));
    let mut engine = Engine::new();
    crate::script_runtime::configure_engine(
        &mut engine,
        &crate::script_protocol::ScriptBudgets::default(),
    );
    let mut scope = Scope::new();
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
        let value = call_pack_entry_with_host(&native, None).expect("entry");
        assert_eq!(value, 42);
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
        let value = call_pack_entry_with_host(&native, None).expect("entry");
        assert_eq!(value, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
