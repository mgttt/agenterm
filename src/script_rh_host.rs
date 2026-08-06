use std::path::Path;

use agenterm_rh::{RH_HOST_API_VERSION, RhError, RhNativeModule};

pub fn broker_fleet_bridge<F>(
    call: F,
) -> Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>
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

pub fn set_fleet_bridge(bridge: Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>) {
    FLEET_BRIDGE.with(|slot| {
        *slot.borrow_mut() = Some(bridge);
    });
}

pub fn clear_fleet_bridge() {
    FLEET_BRIDGE.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

pub fn call_cached_pack_entry_with_fleet(
    bridge: Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>,
) -> Result<i64, RhError> {
    let native_path = crate::script_rh_pack::cached_native_path()
        .ok_or_else(|| RhError::Compile("AGENTERM_RH_PACK native path is unavailable".into()))?;
    call_pack_entry_with_fleet(native_path, bridge)
}

pub fn call_pack_entry_with_fleet(
    native_path: &Path,
    bridge: Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>,
) -> Result<i64, RhError> {
    set_fleet_bridge(bridge);
    let module = RhNativeModule::load(native_path)?;
    register_native_module(&module)?;
    Ok(module.call_entry())
}

pub fn register_native_module(module: &RhNativeModule) -> Result<(), RhError> {
    if module.host_api_version() < RH_HOST_API_VERSION {
        return Err(RhError::Compile(format!(
            "rh pack host api {} is older than host {}",
            module.host_api_version(),
            RH_HOST_API_VERSION
        )));
    }
    module.register_host(host_fleet_call)
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
    static FLEET_BRIDGE: std::cell::RefCell<Option<Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::{call_pack_entry_with_fleet, clear_fleet_bridge};
    use std::sync::{Arc, Mutex};

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
        let value = call_pack_entry_with_fleet(
            &native,
            Box::new(move |operation_id, params| {
                calls_for_bridge
                    .lock()
                    .expect("calls")
                    .push((operation_id.to_owned(), params.to_owned()));
                Ok("{\"operation_id\":\"protocol.info\"}".to_owned())
            }),
        )
        .expect("entry");
        assert_eq!(value, 7);
        let recorded = calls.lock().expect("calls");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "protocol.info");
        clear_fleet_bridge();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
