//! C ABI between rh native packs and the embedding host (worker, gateway, CC).

pub const RH_HOST_API_VERSION: u32 = 9;
pub const RH_CODEGEN_REVISION: u32 = 20;
pub const RH_HOST_OUT_CAP: u32 = 65536;
pub const RH_HOST_FS_READ_CAP: u32 = 1024 * 1024;
pub const RH_HOST_UTILITY_FAIL: u32 = 1;
pub const RH_HOST_UTILITY_EXISTS_CASE_EXACT: u32 = 2;
pub const RH_HOST_UTILITY_PROCESS_STATUS: u32 = 3;
pub const RH_HOST_UTILITY_PRINT: u32 = 4;
pub const RH_HOST_UTILITY_PROCESS_STDOUT_FILE: u32 = 5;

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
         fn rh_path_absolute(path: &str) -> String {\n\
             match std::path::absolute(path) {\n\
                 Ok(absolute) => absolute.to_string_lossy().into_owned(),\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"path_absolute: {error}\"));\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_system_time_now_unix_millis() -> INT {\n\
             match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {\n\
                 Ok(duration) => match i64::try_from(duration.as_millis()) {\n\
                     Ok(millis) => millis,\n\
                     Err(_) => {\n\
                         let _ = rh_fail(\"system_time_overflow: milliseconds exceed Rhai integer\");\n\
                         0\n\
                     }\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"system_time_before_unix_epoch: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_civil_date_from_unix_days(days: i64) -> (i64, i64, i64) {\n\
             let z = days + 719_468;\n\
             let era = if z >= 0 { z } else { z - 146_096 } / 146_097;\n\
             let day_of_era = z - era * 146_097;\n\
             let year_of_era =\n\
                 (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096)\n\
                     / 365;\n\
             let mut year = year_of_era + era * 400;\n\
             let day_of_year =\n\
                 day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);\n\
             let month_prime = (5 * day_of_year + 2) / 153;\n\
             let day = day_of_year - (153 * month_prime + 2) / 5 + 1;\n\
             let month = month_prime + if month_prime < 10 { 3 } else { -9 };\n\
             year += i64::from(month <= 2);\n\
             (year, month, day)\n\
         }\n\n\
         fn rh_system_time_now_rfc3339() -> String {\n\
             match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {\n\
                 Ok(duration) => {\n\
                     let Ok(seconds) = i64::try_from(duration.as_secs()) else {\n\
                         let _ = rh_fail(\"system_time_overflow: seconds exceed supported range\");\n\
                         return String::new();\n\
                     };\n\
                     let days = seconds / 86_400;\n\
                     let day_seconds = seconds % 86_400;\n\
                     let (year, month, day) = rh_civil_date_from_unix_days(days);\n\
                     let hour = day_seconds / 3_600;\n\
                     let minute = (day_seconds % 3_600) / 60;\n\
                     let second = day_seconds % 60;\n\
                     format!(\n\
                         \"{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z\",\n\
                         duration.subsec_millis()\n\
                     )\n\
                 }\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"system_time_before_unix_epoch: {error}\"));\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_env_has(name: &str) -> INT {\n\
             i64::from(std::env::var_os(name).is_some())\n\
         }\n\n\
         fn rh_env_get(name: &str) -> String {\n\
             match std::env::var(name) {\n\
                 Ok(value) => value,\n\
                 Err(_) => {\n\
                     let _ = rh_fail(&format!(\"env_get_missing: {name}\"));\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_sha256_hex(bytes: impl AsRef<[u8]>) -> String {\n\
             const HEX: &[u8; 16] = b\"0123456789abcdef\";\n\
             let bytes = bytes.as_ref();\n\
             let mut output = String::with_capacity(bytes.len() * 2);\n\
             for byte in bytes {\n\
                 output.push(HEX[usize::from(byte >> 4)] as char);\n\
                 output.push(HEX[usize::from(byte & 0x0f)] as char);\n\
             }\n\
             output\n\
         }\n\n\
         fn rh_sha256_file(path: &str) -> String {\n\
             use sha2::{Digest, Sha256};\n\
             use std::io::Read;\n\
             let mut input = match std::fs::File::open(path) {\n\
                 Ok(file) => file,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"crypto_sha256_file: {path}: {error}\"));\n\
                     return String::new();\n\
                 }\n\
             };\n\
             let mut digest = Sha256::new();\n\
             let mut buffer = [0_u8; 64 * 1024];\n\
             loop {\n\
                 let count = match input.read(&mut buffer) {\n\
                     Ok(count) => count,\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"crypto_sha256_file: {path}: {error}\"));\n\
                         return String::new();\n\
                     }\n\
                 };\n\
                 if count == 0 {\n\
                     break;\n\
                 }\n\
                 digest.update(&buffer[..count]);\n\
             }\n\
             rh_sha256_hex(digest.finalize())\n\
         }\n\n\
         fn rh_atomic_write(path: &str, value: &str) -> INT {\n\
             use std::io::Write;\n\
             use std::sync::atomic::{AtomicU64, Ordering};\n\
             static SEQ: AtomicU64 = AtomicU64::new(0);\n\
             let destination = match std::path::absolute(std::path::Path::new(path)) {\n\
                 Ok(path) => path,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"runtime_atomic_write: {path}: {error}\"));\n\
                     return 0;\n\
                 }\n\
             };\n\
             let Some(parent) = destination.parent() else {\n\
                 let _ = rh_fail(\"runtime_atomic_write_invalid_target: parent directory required\");\n\
                 return 0;\n\
             };\n\
             let Some(name) = destination.file_name() else {\n\
                 let _ = rh_fail(\"runtime_atomic_write_invalid_target: file name required\");\n\
                 return 0;\n\
             };\n\
             let sequence = SEQ.fetch_add(1, Ordering::Relaxed);\n\
             let temporary = parent.join(format!(\n\
                 \".{}.agenterm-atomic-{}-{sequence}\",\n\
                 name.to_string_lossy(),\n\
                 std::process::id()\n\
             ));\n\
             let cleanup_path = temporary.clone();\n\
             let mut output = match std::fs::OpenOptions::new()\n\
                 .write(true)\n\
                 .create_new(true)\n\
                 .open(&temporary)\n\
             {\n\
                 Ok(file) => file,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"runtime_atomic_write_create: {path}: {error}\"));\n\
                     return 0;\n\
                 }\n\
             };\n\
             if let Err(error) = output.write_all(value.as_bytes()).and_then(|_| output.sync_all()) {\n\
                 let _ = std::fs::remove_file(&cleanup_path);\n\
                 let _ = rh_fail(&format!(\"runtime_atomic_write_data: {path}: {error}\"));\n\
                 return 0;\n\
             }\n\
             drop(output);\n\
             if let Err(error) = std::fs::rename(&temporary, &destination) {\n\
                 let _ = std::fs::remove_file(&cleanup_path);\n\
                 let _ = rh_fail(&format!(\"runtime_atomic_write_promote: {path}: {error}\"));\n\
                 return 0;\n\
             }\n\
             0\n\
         }\n\n\
         #[derive(Clone, Copy)]\n\
         struct RhMetadata {\n\
             is_file: INT,\n\
             is_dir: INT,\n\
             is_symlink: INT,\n\
             is_reparse_point: INT,\n\
             len: INT,\n\
         }\n\n\
         fn rh_metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {\n\
             #[cfg(windows)]\n\
             {\n\
                 use std::os::windows::fs::MetadataExt;\n\
                 const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;\n\
                 metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0\n\
             }\n\
             #[cfg(not(windows))]\n\
             {\n\
                 metadata.file_type().is_symlink()\n\
             }\n\
         }\n\n\
         fn rh_symlink_metadata(path: &str) -> RhMetadata {\n\
             match std::fs::symlink_metadata(path) {\n\
                 Ok(metadata) => RhMetadata {\n\
                     is_file: metadata.is_file() as INT,\n\
                     is_dir: metadata.is_dir() as INT,\n\
                     is_symlink: metadata.file_type().is_symlink() as INT,\n\
                     is_reparse_point: rh_metadata_is_reparse_point(&metadata) as INT,\n\
                     len: metadata.len() as INT,\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_symlink_metadata: {error}\"));\n\
                     RhMetadata {\n\
                         is_file: 0,\n\
                         is_dir: 0,\n\
                         is_symlink: 0,\n\
                         is_reparse_point: 0,\n\
                         len: 0,\n\
                     }\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_remove_file(path: &str) -> INT {\n\
             match std::fs::remove_file(path) {\n\
                 Ok(()) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_remove_file: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_try_remove_file(path: &str) -> INT {\n\
             i64::from(std::fs::remove_file(path).is_ok())\n\
         }\n\n\
         fn rh_copy(src: &str, dst: &str) -> INT {\n\
             match std::fs::copy(src, dst) {\n\
                 Ok(_) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_copy: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_try_copy(src: &str, dst: &str) -> INT {\n\
             i64::from(std::fs::copy(src, dst).is_ok())\n\
         }\n\n\
         fn rh_create_dir_all(path: &str) -> INT {\n\
             match std::fs::create_dir_all(path) {\n\
                 Ok(()) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_create_dir_all: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_try_create_dir_all(path: &str) -> INT {\n\
             i64::from(std::fs::create_dir_all(path).is_ok())\n\
         }\n\n\
         fn rh_rename(src: &str, dst: &str) -> INT {\n\
             match std::fs::rename(src, dst) {\n\
                 Ok(()) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_rename: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_try_rename(src: &str, dst: &str) -> INT {\n\
             i64::from(std::fs::rename(src, dst).is_ok())\n\
         }\n\n\
         struct RhDirEntry {\n\
             file_name: String,\n\
             path: String,\n\
             is_file: INT,\n\
             is_dir: INT,\n\
             is_symlink: INT,\n\
         }\n\n\
         fn rh_read_dir(path: &str) -> Vec<RhDirEntry> {\n\
             match std::fs::read_dir(path) {\n\
                 Ok(entries) => entries\n\
                     .filter_map(|entry| {\n\
                         let entry = entry.ok()?;\n\
                         let file_type = entry.file_type().ok()?;\n\
                         Some(RhDirEntry {\n\
                             file_name: entry.file_name().to_string_lossy().into_owned(),\n\
                             path: entry.path().to_string_lossy().into_owned(),\n\
                             is_file: file_type.is_file() as INT,\n\
                             is_dir: file_type.is_dir() as INT,\n\
                             is_symlink: file_type.is_symlink() as INT,\n\
                         })\n\
                     })\n\
                     .collect(),\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_read_dir: {error}\"));\n\
                     Vec::new()\n\
                 }\n\
             }\n\
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
         fn rh_print(message: &str) -> INT {\n\
             rh_utility(4, message)\n\
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
         fn rh_process_stdout_file(\n\
             program: &str,\n\
             args: &[String],\n\
             timeout_ms: INT,\n\
             stdout_path: &str,\n\
         ) -> INT {\n\
             let request = serde_json::json!({\n\
                 \"program\": program,\n\
                 \"args\": args,\n\
                 \"timeout_ms\": timeout_ms,\n\
                 \"stdout_path\": stdout_path,\n\
             });\n\
             rh_utility(5, &request.to_string())\n\
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
         fn rh_json_stringify_pretty(value: &serde_json::Value) -> String {\n\
             match serde_json::to_string_pretty(value) {\n\
                 Ok(text) => text,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"json_stringify: {error}\"));\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_array_push(target: &mut serde_json::Value, item: serde_json::Value) -> INT {\n\
             match target.as_array_mut() {\n\
                 Some(items) => {\n\
                     items.push(item);\n\
                     0\n\
                 }\n\
                 None => {\n\
                     let _ = rh_fail(\"json_array_push_target\");\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_array_get(value: &serde_json::Value, index: INT) -> serde_json::Value {\n\
             match value.as_array().and_then(|items| {\n\
                 usize::try_from(index).ok().and_then(|index| items.get(index))\n\
             }) {\n\
                 Some(item) => item.clone(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_array_index: {index}\"));\n\
                     serde_json::Value::Null\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_string_split(value: &str, separator: &str) -> Vec<String> {\n\
             value.split(separator).map(str::to_owned).collect()\n\
         }\n\n\
         fn rh_string_list_get(items: &[String], index: INT) -> String {\n\
             match usize::try_from(index).ok().and_then(|index| items.get(index)) {\n\
                 Some(item) => item.clone(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"string_list_index: {index}\"));\n\
                     String::new()\n\
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
