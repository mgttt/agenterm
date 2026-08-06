//! C ABI between rh native packs and the embedding host (worker, gateway, CC).

pub const RH_HOST_API_VERSION: u32 = 2;
pub const RH_HOST_OUT_CAP: u32 = 65536;

pub type RhHostFleetCall = extern "C" fn(
    operation_id: *const u8,
    operation_id_len: u32,
    params_json: *const u8,
    params_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32;

pub type RhHostEvalCall = extern "C" fn(
    snippet: *const u8,
    snippet_len: u32,
    scope_json: *const u8,
    scope_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32;

pub fn emit_host_runtime(out: &mut String) {
    out.push_str(
        "type RhHostFleetCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\
         type RhHostEvalCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\n\
         static mut RH_HOST_FLEET_CALL: Option<RhHostFleetCall> = None;\n\
         static mut RH_HOST_EVAL_CALL: Option<RhHostEvalCall> = None;\n\
         static mut RH_HOST_OUT_LEN: usize = 0;\n\
         static RH_HOST_OUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_host_api_version() -> u32 {\n    ",
    );
    out.push_str(&RH_HOST_API_VERSION.to_string());
    out.push_str(
        "\n}\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host(fleet_call: RhHostFleetCall) {\n\
             unsafe { RH_HOST_FLEET_CALL = Some(fleet_call); }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v2(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
             }\n\
         }\n\n\
         fn rh_host_store(wrote: i32, scratch: Vec<u8>) -> i32 {\n\
             if wrote <= 0 {\n\
                 return wrote;\n\
             }\n\
             unsafe { RH_HOST_OUT_LEN = wrote as usize; }\n\
             let _ = RH_HOST_OUT.set(scratch);\n\
             wrote\n\
         }\n\n\
         fn rh_host_json() -> Option<serde_json::Value> {\n\
             let buffer = RH_HOST_OUT.get()?;\n\
             let len = unsafe { RH_HOST_OUT_LEN };\n\
             let slice = &buffer[..len.min(buffer.len())];\n\
             serde_json::from_slice(slice).ok()\n\
         }\n\n\
         fn rh_fleet_call(operation_id: &str, params_json: &str) -> i32 {\n\
             let Some(call) = (unsafe { RH_HOST_FLEET_CALL }) else {\n\
                 return -4;\n\
             };\n\
             let mut scratch = vec![0u8; ",
    );
    out.push_str(&RH_HOST_OUT_CAP.to_string());
    out.push_str(
        "usize];\n\
             let wrote = call(\n\
                 operation_id.as_ptr(),\n\
                 operation_id.len() as u32,\n\
                 params_json.as_ptr(),\n\
                 params_json.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             rh_host_store(wrote, scratch)\n\
         }\n\n\
         fn rh_host_eval_raw(snippet: &str, scope_json: &str) -> i32 {\n\
             let Some(call) = (unsafe { RH_HOST_EVAL_CALL }) else {\n\
                 return -4;\n\
             };\n\
             let mut scratch = vec![0u8; ",
    );
    out.push_str(&RH_HOST_OUT_CAP.to_string());
    out.push_str(
        "usize];\n\
             let wrote = call(\n\
                 snippet.as_ptr(),\n\
                 snippet.len() as u32,\n\
                 scope_json.as_ptr(),\n\
                 scope_json.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             rh_host_store(wrote, scratch)\n\
         }\n\n\
         fn rh_host_eval_int(snippet: &str, scope_json: &str) -> INT {\n\
             let wrote = rh_host_eval_raw(snippet, scope_json);\n\
             if wrote <= 0 {\n\
                 return wrote as INT;\n\
             }\n\
             if let Some(value) = rh_host_json() {\n\
                 if let Some(number) = value.get(\"value\").and_then(|v| v.as_i64()) {\n\
                     return number as INT;\n\
                 }\n\
                 if let Some(flag) = value.get(\"value\").and_then(|v| v.as_bool()) {\n\
                     return if flag { 1 } else { 0 };\n\
                 }\n\
             }\n\
             -6\n\
         }\n\n",
    );
}

// Back-compat alias used by older tests/docs.
pub const RH_HOST_FLEET_OUT_CAP: u32 = RH_HOST_OUT_CAP;
