//! C ABI between rh native packs and the embedding host (worker, gateway, CC).

pub const RH_HOST_API_VERSION: u32 = 9;
pub const RH_CODEGEN_REVISION: u32 = 7;
pub const RH_HOST_OUT_CAP: u32 = 65536;
pub const RH_HOST_FS_READ_CAP: u32 = 1024 * 1024;
pub const RH_HOST_UTILITY_FAIL: u32 = 1;
pub const RH_HOST_UTILITY_EXISTS_CASE_EXACT: u32 = 2;
pub const RH_HOST_UTILITY_PROCESS_STATUS: u32 = 3;

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

pub type RhHostRunScriptCall =
    extern "C" fn(source: *const u8, source_len: u32, out_buf: *mut u8, out_cap: u32) -> i32;

pub type RhHostStdFsExistsCall = extern "C" fn(path: *const u8, path_len: u32) -> i32;
pub type RhHostArgsLenCall = extern "C" fn() -> i64;
pub type RhHostArgCall = extern "C" fn(index: u32, out_buf: *mut u8, out_cap: u32) -> i32;
pub type RhHostFsReadCall =
    extern "C" fn(path: *const u8, path_len: u32, out_buf: *mut u8, out_cap: u32) -> i32;
pub type RhHostUtilityCall = extern "C" fn(operation: u32, input: *const u8, input_len: u32) -> i32;

pub fn rust_raw_string_literal(source: &str) -> String {
    let mut hashes = 0_usize;
    loop {
        let delimiter = "#".repeat(hashes);
        let terminator = format!("\"{delimiter}");
        if !source.contains(&terminator) {
            return format!("r{delimiter}\"{source}\"{delimiter}");
        }
        hashes += 1;
    }
}

pub fn emit_host_runtime(out: &mut String) {
    out.push_str(
        "type RhHostFleetCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\
         type RhHostEvalCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\
         type RhHostRunScriptCall = extern \"C\" fn(*const u8, u32, *mut u8, u32) -> i32;\n\
         type RhHostStdFsExistsCall = extern \"C\" fn(*const u8, u32) -> i32;\n\
         type RhHostArgsLenCall = extern \"C\" fn() -> i64;\n\n\
         type RhHostArgCall = extern \"C\" fn(u32, *mut u8, u32) -> i32;\n\n\
         type RhHostFsReadCall = extern \"C\" fn(*const u8, u32, *mut u8, u32) -> i32;\n\n\
         type RhHostUtilityCall = extern \"C\" fn(u32, *const u8, u32) -> i32;\n\n\
         static mut RH_HOST_FLEET_CALL: Option<RhHostFleetCall> = None;\n\
         static mut RH_HOST_EVAL_CALL: Option<RhHostEvalCall> = None;\n\
         static mut RH_HOST_RUN_SCRIPT_CALL: Option<RhHostRunScriptCall> = None;\n\
         static mut RH_HOST_STD_FS_EXISTS_CALL: Option<RhHostStdFsExistsCall> = None;\n\
         static mut RH_HOST_ARGS_LEN_CALL: Option<RhHostArgsLenCall> = None;\n\
         static mut RH_HOST_ARG_CALL: Option<RhHostArgCall> = None;\n\
         static mut RH_HOST_FS_READ_CALL: Option<RhHostFsReadCall> = None;\n\
         static mut RH_HOST_UTILITY_CALL: Option<RhHostUtilityCall> = None;\n\
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
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v3(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v4(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v5(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
                 RH_HOST_ARGS_LEN_CALL = Some(args_len_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v6(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
             arg_call: RhHostArgCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
                 RH_HOST_ARGS_LEN_CALL = Some(args_len_call);\n\
                 RH_HOST_ARG_CALL = Some(arg_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v7(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
             arg_call: RhHostArgCall,\n\
             fs_read_call: RhHostFsReadCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
                 RH_HOST_ARGS_LEN_CALL = Some(args_len_call);\n\
                 RH_HOST_ARG_CALL = Some(arg_call);\n\
                 RH_HOST_FS_READ_CALL = Some(fs_read_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v8(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
             arg_call: RhHostArgCall,\n\
             fs_read_call: RhHostFsReadCall,\n\
             utility_call: RhHostUtilityCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_EVAL_CALL = Some(eval_call);\n\
                 RH_HOST_RUN_SCRIPT_CALL = Some(run_script_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
                 RH_HOST_ARGS_LEN_CALL = Some(args_len_call);\n\
                 RH_HOST_ARG_CALL = Some(arg_call);\n\
                 RH_HOST_FS_READ_CALL = Some(fs_read_call);\n\
                 RH_HOST_UTILITY_CALL = Some(utility_call);\n\
             }\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_register_host_v9(\n\
             fleet_call: RhHostFleetCall,\n\
             eval_call: RhHostEvalCall,\n\
             run_script_call: RhHostRunScriptCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
             arg_call: RhHostArgCall,\n\
             fs_read_call: RhHostFsReadCall,\n\
             utility_call: RhHostUtilityCall,\n\
         ) {\n\
             rh_register_host_v8(\n\
                 fleet_call,\n\
                 eval_call,\n\
                 run_script_call,\n\
                 std_fs_exists_call,\n\
                 args_len_call,\n\
                 arg_call,\n\
                 fs_read_call,\n\
                 utility_call,\n\
             );\n\
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
         }\n\n\
         fn rh_std_fs_exists(path: &str) -> INT {\n\
             let Some(call) = (unsafe { RH_HOST_STD_FS_EXISTS_CALL }) else {\n\
                 return -4;\n\
             };\n\
             call(path.as_ptr(), path.len() as u32) as INT\n\
         }\n\n\
         fn rh_args_len() -> INT {\n\
             let Some(call) = (unsafe { RH_HOST_ARGS_LEN_CALL }) else {\n\
                 return -4;\n\
             };\n\
             call() as INT\n\
         }\n\n\
         fn rh_arg(index: INT) -> String {\n\
             if index < 0 {\n\
                 return String::new();\n\
             }\n\
             let Some(call) = (unsafe { RH_HOST_ARG_CALL }) else {\n\
                 return String::new();\n\
             };\n\
             let mut scratch = vec![0u8; ",
    );
    out.push_str(&RH_HOST_OUT_CAP.to_string());
    out.push_str(
        "usize];\n\
             let wrote = call(index as u32, scratch.as_mut_ptr(), scratch.len() as u32);\n\
             if wrote <= 0 {\n\
                 return String::new();\n\
             }\n\
             scratch.truncate((wrote as usize).min(scratch.len()));\n\
             String::from_utf8(scratch).unwrap_or_default()\n\
         }\n\n\
         fn rh_std_fs_read_to_string(path: &str) -> String {\n\
             let Some(call) = (unsafe { RH_HOST_FS_READ_CALL }) else {\n\
                 return String::new();\n\
             };\n\
             let mut scratch = vec![0u8; ",
    );
    out.push_str(&RH_HOST_FS_READ_CAP.to_string());
    out.push_str(
        "usize];\n\
             let wrote = call(\n\
                 path.as_ptr(),\n\
                 path.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             if wrote <= 0 {\n\
                 return String::new();\n\
             }\n\
             scratch.truncate((wrote as usize).min(scratch.len()));\n\
             String::from_utf8(scratch).unwrap_or_default()\n\
         }\n\n\
         fn rh_path_join(base: &str, child: &str) -> String {\n\
             std::path::Path::new(base).join(child).to_string_lossy().into_owned()\n\
         }\n\n\
         fn rh_utility(operation: u32, input: &str) -> INT {\n\
             let Some(call) = (unsafe { RH_HOST_UTILITY_CALL }) else {\n\
                 return -4;\n\
             };\n\
             call(operation, input.as_ptr(), input.len() as u32) as INT\n\
         }\n\n\
         fn rh_fail(message: &str) -> INT {\n\
             rh_utility(1, message)\n\
         }\n\n\
         fn rh_std_fs_exists_case_exact(path: &str) -> INT {\n\
             rh_utility(2, path)\n\
         }\n\n\
         fn rh_process_status(program: &str, args: &[String], timeout_ms: INT) -> INT {\n\
             let request = serde_json::json!({\n\
                 \"program\": program,\n\
                 \"args\": args,\n\
                 \"timeout_ms\": timeout_ms,\n\
             });\n\
             rh_utility(3, &request.to_string())\n\
         }\n\n\
         fn rh_json_parse(source: &str) -> serde_json::Value {\n\
             match serde_json::from_str(source) {\n\
                 Ok(value) => value,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"json_parse: {error}\"));\n\
                     serde_json::Value::Null\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_path<'a>(\n\
             mut value: &'a serde_json::Value,\n\
             path: &[&str],\n\
         ) -> Option<&'a serde_json::Value> {\n\
             for segment in path {\n\
                 value = value.get(*segment)?;\n\
             }\n\
             Some(value)\n\
         }\n\n\
         fn rh_json_int_path(value: &serde_json::Value, path: &[&str]) -> INT {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_i64) {\n\
                 Some(value) => value as INT,\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_integer_path: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_array_len(value: &serde_json::Value, path: &[&str]) -> INT {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_array) {\n\
                 Some(items) => items.len() as INT,\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_array_path: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_array_items(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
         ) -> Vec<serde_json::Value> {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_array) {\n\
                 Some(items) => items.clone(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_array_path: {}\", path.join(\".\")));\n\
                     Vec::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_get_path(value: &serde_json::Value, path: &[&str]) -> serde_json::Value {\n\
             match rh_json_path(value, path) {\n\
                 Some(value) => value.clone(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_path: {}\", path.join(\".\")));\n\
                     serde_json::Value::Null\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_as_i64(value: &serde_json::Value) -> INT {\n\
             match value.as_i64() {\n\
                 Some(value) => value as INT,\n\
                 None => {\n\
                     let _ = rh_fail(\"json_integer_value\");\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_as_str(value: &serde_json::Value) -> String {\n\
             match value.as_str() {\n\
                 Some(value) => value.to_owned(),\n\
                 None => {\n\
                     let _ = rh_fail(\"json_string_value\");\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_string_path(value: &serde_json::Value, path: &[&str]) -> String {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_str) {\n\
                 Some(value) => value.to_owned(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_string_path: {}\", path.join(\".\")));\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_type_name(value: &serde_json::Value, path: &[&str]) -> String {\n\
             match rh_json_path(value, path) {\n\
                 Some(serde_json::Value::Array(_)) => String::from(\"array\"),\n\
                 Some(serde_json::Value::Object(_)) => String::from(\"map\"),\n\
                 Some(serde_json::Value::String(_)) => String::from(\"string\"),\n\
                 Some(serde_json::Value::Bool(_)) => String::from(\"bool\"),\n\
                 Some(serde_json::Value::Number(number)) => {\n\
                     if number.is_i64() {\n\
                         String::from(\"i64\")\n\
                     } else {\n\
                         String::from(\"f64\")\n\
                     }\n\
                 }\n\
                 Some(serde_json::Value::Null) | None => {\n\
                     if path.is_empty() {\n\
                         let _ = rh_fail(\"json_type_name\");\n\
                     } else {\n\
                         let _ = rh_fail(&format!(\"json_type_name: {}\", path.join(\".\")));\n\
                     }\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_type_name_value(value: &serde_json::Value) -> String {\n\
             rh_json_type_name(value, &[])\n\
         }\n\n\
         fn rh_host_run_script(source: &str) -> INT {\n\
             let Some(call) = (unsafe { RH_HOST_RUN_SCRIPT_CALL }) else {\n\
                 return -4;\n\
             };\n\
             let mut scratch = vec![0u8; ",
    );
    out.push_str(&RH_HOST_OUT_CAP.to_string());
    out.push_str(
        "usize];\n\
             let wrote = call(\n\
                 source.as_ptr(),\n\
                 source.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             if wrote <= 0 {\n\
                 return wrote as INT;\n\
             }\n\
             unsafe { RH_HOST_OUT_LEN = wrote as usize; }\n\
             let _ = RH_HOST_OUT.set(scratch);\n\
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
