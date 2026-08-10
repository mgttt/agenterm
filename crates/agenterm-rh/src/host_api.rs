//! C ABI between rh native packs and the embedding host (worker, gateway, CC).

pub const RH_HOST_API_VERSION: u32 = 13;
pub const RH_CODEGEN_REVISION: u32 = 89;

/// First-class host API module root registered on the Engine and accepted by AOT emit.
pub const RH_HOST_API_ROOT: &str = "rh";
/// Host API submodule suffix after `rh::` or legacy `rhai::` (e.g. `json`, `task`).
pub fn host_api_module(namespace: &str) -> Option<&'static str> {
    let rest = namespace
        .strip_prefix(RH_HOST_API_ROOT)
        .and_then(|rest| rest.strip_prefix("::"))?;
    match rest {
        "json" => Some("json"),
        "task" => Some("task"),
        "crypto" => Some("crypto"),
        "runtime" => Some("runtime"),
        "bytes" => Some("bytes"),
        "http" => Some("http"),
        "image" => Some("image"),
        "clipboard" => Some("clipboard"),
        "hash" => Some("hash"),
        _ => None,
    }
}
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

pub type RhHostStdFsExistsCall = extern "C" fn(path: *const u8, path_len: u32) -> i32;
pub type RhHostArgsLenCall = extern "C" fn() -> i64;
pub type RhHostArgCall = extern "C" fn(index: u32, out_buf: *mut u8, out_cap: u32) -> i32;
pub type RhHostFsReadCall =
    extern "C" fn(path: *const u8, path_len: u32, out_buf: *mut u8, out_cap: u32) -> i32;
pub type RhHostUtilityCall = extern "C" fn(operation: u32, input: *const u8, input_len: u32) -> i32;
pub type RhHostJsonCall = extern "C" fn(
    operation: *const u8,
    operation_len: u32,
    input_json: *const u8,
    input_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32;

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
         type RhHostStdFsExistsCall = extern \"C\" fn(*const u8, u32) -> i32;\n\
         type RhHostArgsLenCall = extern \"C\" fn() -> i64;\n\n\
         type RhHostArgCall = extern \"C\" fn(u32, *mut u8, u32) -> i32;\n\n\
         type RhHostFsReadCall = extern \"C\" fn(*const u8, u32, *mut u8, u32) -> i32;\n\n\
         type RhHostUtilityCall = extern \"C\" fn(u32, *const u8, u32) -> i32;\n\n\
         type RhHostJsonCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\n\
         static mut RH_HOST_FLEET_CALL: Option<RhHostFleetCall> = None;\n\
         static mut RH_HOST_STD_FS_EXISTS_CALL: Option<RhHostStdFsExistsCall> = None;\n\
         static mut RH_HOST_ARGS_LEN_CALL: Option<RhHostArgsLenCall> = None;\n\
         static mut RH_HOST_ARG_CALL: Option<RhHostArgCall> = None;\n\
         static mut RH_HOST_FS_READ_CALL: Option<RhHostFsReadCall> = None;\n\
         static mut RH_HOST_UTILITY_CALL: Option<RhHostUtilityCall> = None;\n\
         static mut RH_HOST_JSON_CALL: Option<RhHostJsonCall> = None;\n\
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
         pub extern \"C\" fn rh_register_host_v11(\n\
             fleet_call: RhHostFleetCall,\n\
             std_fs_exists_call: RhHostStdFsExistsCall,\n\
             args_len_call: RhHostArgsLenCall,\n\
             arg_call: RhHostArgCall,\n\
             fs_read_call: RhHostFsReadCall,\n\
             utility_call: RhHostUtilityCall,\n\
             json_call: RhHostJsonCall,\n\
         ) {\n\
             unsafe {\n\
                 RH_HOST_FLEET_CALL = Some(fleet_call);\n\
                 RH_HOST_STD_FS_EXISTS_CALL = Some(std_fs_exists_call);\n\
                 RH_HOST_ARGS_LEN_CALL = Some(args_len_call);\n\
                 RH_HOST_ARG_CALL = Some(arg_call);\n\
                 RH_HOST_FS_READ_CALL = Some(fs_read_call);\n\
                 RH_HOST_UTILITY_CALL = Some(utility_call);\n\
                 RH_HOST_JSON_CALL = Some(json_call);\n\
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
         fn rh_host_json_call(operation: &str, input: &serde_json::Value) -> serde_json::Value {\n\
             let Some(call) = (unsafe { RH_HOST_JSON_CALL }) else {\n\
                 let _ = rh_fail(\"host_json_call_unavailable\");\n\
                 return serde_json::Value::Null;\n\
             };\n\
             let input_json = rh_json_stringify(input);\n\
             let mut scratch = vec![0u8; 65536usize];\n\
             let wrote = call(\n\
                 operation.as_ptr(),\n\
                 operation.len() as u32,\n\
                 input_json.as_ptr(),\n\
                 input_json.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             if wrote <= 0 {\n\
                 let _ = rh_fail(&format!(\"host_json_call_failed: {operation}: {wrote}\"));\n\
                 return serde_json::Value::Null;\n\
             }\n\
             serde_json::from_slice(&scratch[..(wrote as usize).min(scratch.len())])\n\
                 .unwrap_or(serde_json::Value::Null)\n\
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
         fn rh_path_join2(base: String, child: String) -> String {\n\
             std::path::Path::new(base.as_str()).join(child.as_str()).to_string_lossy().into_owned()\n\
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
         fn rh_path_is_absolute(path: &str) -> INT {\n\
             i64::from(std::path::Path::new(path).is_absolute())\n\
         }\n\n\
         fn rh_path_file_name(path: &str) -> String {\n\
             std::path::Path::new(path)\n\
                 .file_name()\n\
                 .map(|name| name.to_string_lossy().into_owned())\n\
                 .unwrap_or_default()\n\
         }\n\n\
         fn rh_path_parent(path: &str) -> String {\n\
             std::path::Path::new(path)\n\
                 .parent()\n\
                 .map(|parent| parent.to_string_lossy().into_owned())\n\
                 .unwrap_or_default()\n\
         }\n\n\
         fn rh_process_id() -> INT {\n\
             std::process::id() as INT\n\
         }\n\n\
         fn rh_process_kill(pid: INT) -> INT {\n\
             if pid <= 0 {\n\
                 return rh_fail(\"process_kill_invalid_pid\");\n\
             }\n\
             #[cfg(unix)]\n\
             {\n\
                 extern \"C\" {\n\
                     fn kill(pid: i32, sig: i32) -> i32;\n\
                 }\n\
                 const SIGTERM: i32 = 15;\n\
                 let pid = match i32::try_from(pid) {\n\
                     Ok(value) if value > 0 => value,\n\
                     _ => return rh_fail(\"process_kill_invalid_pid\"),\n\
                 };\n\
                 if unsafe { kill(pid, SIGTERM) } == 0 {\n\
                     0\n\
                 } else {\n\
                     rh_fail(\"process_kill\")\n\
                 }\n\
             }\n\
             #[cfg(windows)]\n\
             {\n\
                 match std::process::Command::new(\"taskkill\")\n\
                     .args([\"/PID\", &pid.to_string(), \"/F\"])\n\
                     .status()\n\
                 {\n\
                     Ok(status) if status.success() => 0,\n\
                     _ => rh_fail(\"process_kill\"),\n\
                 }\n\
             }\n\
             #[cfg(not(any(unix, windows)))]\n\
             {\n\
                 let result = rh_host_json_call(\n\
                     \"process.kill\",\n\
                     &serde_json::json!({ \"pid\": pid }),\n\
                 );\n\
                 if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                     0\n\
                 } else {\n\
                     rh_fail(\"process_kill\")\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_system_time_now_unix_millis() -> INT {\n\
             match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {\n\
                 Ok(duration) => match i64::try_from(duration.as_millis()) {\n\
                     Ok(millis) => millis,\n\
                     Err(_) => {\n\
                         let _ = rh_fail(\"system_time_overflow: milliseconds exceed Rh integer\");\n\
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
         fn rh_env_parse_int(name: &str) -> INT {\n\
             match std::env::var(name) {\n\
                 Ok(value) => value.parse::<INT>().unwrap_or(-1),\n\
                 Err(_) => -1,\n\
             }\n\
         }\n\n\
         fn rh_string_parse_int(value: &str) -> INT {\n\
             match value.trim().parse::<INT>() {\n\
                 Ok(number) => number,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"string_parse_int: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_string_argv(value: &serde_json::Value) -> Vec<String> {\n\
             rh_json_array_items(value, &[])\n\
                 .into_iter()\n\
                 .map(|item| rh_json_as_str(&item))\n\
                 .collect()\n\
         }\n\n\
         fn rh_env_current_dir() -> String {\n\
             match std::env::current_dir() {\n\
                 Ok(path) => path.to_string_lossy().into_owned(),\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"environment_current_dir: {error}\"));\n\
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
         fn rh_hash_fnv1a64(text: &str) -> String {\n\
             let mut hash = 0xcbf29ce484222325_u64;\n\
             for byte in text.bytes() {\n\
                 hash ^= u64::from(byte);\n\
                 hash = hash.wrapping_mul(0x100000001b3);\n\
             }\n\
             format!(\"fnv1a64:{hash:016x}\")\n\
         }\n\n\
         fn rh_append_sync(path: &str, text: &str) -> INT {\n\
             use std::io::Write;\n\
             match std::fs::OpenOptions::new()\n\
                 .append(true)\n\
                 .create(true)\n\
                 .open(path)\n\
             {\n\
                 Ok(mut output) => match output.write_all(text.as_bytes()) {\n\
                     Ok(()) => 0,\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"runtime_append_sync: {path}: {error}\"));\n\
                         0\n\
                     }\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"runtime_append_sync: {path}: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_string_index_of(haystack: &str, needle: &str) -> INT {\n\
             match haystack.find(needle) {\n\
                 Some(index) => index as INT,\n\
                 None => -1,\n\
             }\n\
         }\n\n\
         fn rh_string_sub_string(value: &str, start: INT, len: Option<INT>) -> String {\n\
             if start < 0 {\n\
                 return String::new();\n\
             }\n\
             let start = start as usize;\n\
             let chars: Vec<char> = value.chars().collect();\n\
             if start >= chars.len() {\n\
                 return String::new();\n\
             }\n\
             match len {\n\
                 Some(len) if len < 0 => String::new(),\n\
                 Some(len) => chars.iter().skip(start).take(len as usize).collect(),\n\
                 None => chars.iter().skip(start).collect(),\n\
             }\n\
         }\n\n\
         #[cfg(windows)]\n\
         fn rh_replace_file(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {\n\
             use std::os::windows::ffi::OsStrExt;\n\
             use std::time::Duration;\n\
             use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};\n\
             use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};\n\
             let source = std::fs::canonicalize(source)?;\n\
             let parent = destination.parent().ok_or_else(|| std::io::Error::other(\"destination parent required\"))?;\n\
             let destination = std::fs::canonicalize(parent)?\n\
                 .join(destination.file_name().ok_or_else(|| std::io::Error::other(\"destination name required\"))?);\n\
             let source = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();\n\
             let destination = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();\n\
             const ATTEMPTS: usize = 32;\n\
             for attempt in 0..ATTEMPTS {\n\
                 if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } != 0 {\n\
                     return Ok(());\n\
                 }\n\
                 let error = std::io::Error::last_os_error();\n\
                 let retryable = matches!(error.raw_os_error(), Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_SHARING_VIOLATION as i32 || code == ERROR_LOCK_VIOLATION as i32);\n\
                 if !retryable || attempt + 1 == ATTEMPTS {\n\
                     return Err(error);\n\
                 }\n\
                 std::thread::sleep(Duration::from_millis(2));\n\
             }\n\
             unreachable!(\"bounded replacement loop always returns\")\n\
         }\n\n\
         #[cfg(not(windows))]\n\
         fn rh_replace_file(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {\n\
             std::fs::rename(source, destination)\n\
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
             if let Err(error) = rh_replace_file(&temporary, &destination) {\n\
                 let _ = std::fs::remove_file(&cleanup_path);\n\
                 let _ = rh_fail(&format!(\"runtime_atomic_write_promote: {path}: {error}\"));\n\
                 return 0;\n\
             }\n\
             0\n\
         }\n\n\
         #[derive(Clone, Copy)]\n\
         struct RhSystemTime {\n\
             unix_millis: INT,\n\
         }\n\n\
         fn rh_system_time_rfc3339(time: &RhSystemTime) -> String {\n\
             let millis = time.unix_millis;\n\
             if millis < 0 {\n\
                 let _ = rh_fail(\"system_time_before_unix_epoch\");\n\
                 return String::new();\n\
             }\n\
             let seconds = millis / 1_000;\n\
             let subsec_millis = (millis % 1_000) as u32;\n\
             let days = seconds / 86_400;\n\
             let day_seconds = seconds % 86_400;\n\
             let (year, month, day) = rh_civil_date_from_unix_days(days);\n\
             let hour = day_seconds / 3_600;\n\
             let minute = (day_seconds % 3_600) / 60;\n\
             let second = day_seconds % 60;\n\
             format!(\n\
                 \"{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{subsec_millis:03}Z\"\n\
             )\n\
         }\n\n\
         fn rh_system_time_from_metadata(metadata: &std::fs::Metadata) -> RhSystemTime {\n\
             match metadata.modified() {\n\
                 Ok(time) => match time.duration_since(std::time::UNIX_EPOCH) {\n\
                     Ok(duration) => match i64::try_from(duration.as_millis()) {\n\
                         Ok(millis) => RhSystemTime { unix_millis: millis },\n\
                         Err(_) => {\n\
                             let _ = rh_fail(\"system_time_overflow: milliseconds exceed Rh integer\");\n\
                             RhSystemTime { unix_millis: 0 }\n\
                         }\n\
                     },\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"system_time_before_unix_epoch: {error}\"));\n\
                         RhSystemTime { unix_millis: 0 }\n\
                     }\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"filesystem_modified_unavailable: {error}\"));\n\
                     RhSystemTime { unix_millis: 0 }\n\
                 }\n\
             }\n\
         }\n\n\
         #[derive(Clone, Copy)]\n\
         struct RhMetadata {\n\
             is_file: INT,\n\
             is_dir: INT,\n\
             is_symlink: INT,\n\
             is_reparse_point: INT,\n\
             len: INT,\n\
             modified: RhSystemTime,\n\
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
                     modified: rh_system_time_from_metadata(&metadata),\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_symlink_metadata: {error}\"));\n\
                     RhMetadata {\n\
                         is_file: 0,\n\
                         is_dir: 0,\n\
                         is_symlink: 0,\n\
                         is_reparse_point: 0,\n\
                         len: 0,\n\
                         modified: RhSystemTime { unix_millis: 0 },\n\
                     }\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_metadata(path: &str) -> RhMetadata {\n\
             match std::fs::metadata(path) {\n\
                 Ok(metadata) => RhMetadata {\n\
                     is_file: metadata.is_file() as INT,\n\
                     is_dir: metadata.is_dir() as INT,\n\
                     is_symlink: metadata.file_type().is_symlink() as INT,\n\
                     is_reparse_point: rh_metadata_is_reparse_point(&metadata) as INT,\n\
                     len: metadata.len() as INT,\n\
                     modified: rh_system_time_from_metadata(&metadata),\n\
                 },\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_metadata: {error}\"));\n\
                     RhMetadata {\n\
                         is_file: 0,\n\
                         is_dir: 0,\n\
                         is_symlink: 0,\n\
                         is_reparse_point: 0,\n\
                         len: 0,\n\
                         modified: RhSystemTime { unix_millis: 0 },\n\
                     }\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_std_fs_write(path: &str, contents: &str) -> INT {\n\
             match std::fs::write(path, contents.as_bytes()) {\n\
                 Ok(()) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_write: {error}\"));\n\
                     0\n\
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
         fn rh_remove_dir_all(path: &str) -> INT {\n\
             match std::fs::remove_dir_all(path) {\n\
                 Ok(()) => 0,\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"fs_remove_dir_all: {error}\"));\n\
                     0\n\
                 }\n\
             }\n\
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
         #[derive(Clone)]\n\
         struct RhCommand {\n\
             program: String,\n\
             args: Vec<String>,\n\
             // Ordered env program: `Some` sets, `None` removes. A single\n\
             // sequential list mirrors the interpreter's last-write-wins\n\
             // map (ScriptCommand.environment) -- split set/remove\n\
             // containers replayed remove-first re-set variables that a\n\
             // script removed and never set again (native-ipc-smoke's\n\
             // AGENTERM_SETTINGS_PATH hygiene).\n\
             env: Vec<(String, Option<String>)>,\n\
             timeout_ms: INT,\n\
             capture_limit: usize,\n\
             current_dir: Option<String>,\n\
             stdin_text: Option<String>,\n\
             stdout_file: Option<String>,\n\
             stderr_file: Option<String>,\n\
             stderr_inherit: bool,\n\
         }\n\n\
         #[derive(Clone)]\n\
         struct RhOutput {\n\
             success: INT,\n\
             exit_code: INT,\n\
             stdout: String,\n\
             stderr: String,\n\
         }\n\n\
         struct RhChild {\n\
             inner: std::rc::Rc<std::cell::RefCell<RhChildInner>>,\n\
         }\n\n\
         struct RhChildInner {\n\
             child: Option<std::process::Child>,\n\
             pid: INT,\n\
             state: String,\n\
             capture_limit: usize,\n\
         }\n\n\
        struct RhStream {\n\
            receiver: std::sync::mpsc::Receiver<Vec<u8>>,\n\
        }\n\n\
        struct RhBytes {\n\
            bytes: Vec<u8>,\n\
        }\n\n\
         impl Clone for RhChild {\n\
             fn clone(&self) -> Self {\n\
                 Self {\n\
                     inner: self.inner.clone(),\n\
                 }\n\
             }\n\
         }\n\n\
         impl RhChild {\n\
             fn new(pid: INT, child: Option<std::process::Child>, capture_limit: usize) -> Self {\n\
                 Self {\n\
                     inner: std::rc::Rc::new(std::cell::RefCell::new(RhChildInner {\n\
                         child,\n\
                         pid,\n\
                         state: String::from(\"running\"),\n\
                         capture_limit,\n\
                     })),\n\
                 }\n\
             }\n\n\
             fn exited(pid: INT, capture_limit: usize) -> Self {\n\
                 Self {\n\
                     inner: std::rc::Rc::new(std::cell::RefCell::new(RhChildInner {\n\
                         child: None,\n\
                         pid,\n\
                         state: String::from(\"exited\"),\n\
                         capture_limit,\n\
                     })),\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_child_share(child: &mut RhChild) -> RhChild {\n\
             child.clone()\n\
         }\n\n\
         fn rh_command_new(program: &str) -> RhCommand {\n\
             RhCommand {\n\
                 program: program.to_owned(),\n\
                 args: Vec::new(),\n\
                 env: Vec::new(),\n\
                 timeout_ms: 2_000,\n\
                 capture_limit: 64 * 1024,\n\
                 current_dir: None,\n\
                 stdin_text: None,\n\
                 stdout_file: None,\n\
                 stderr_file: None,\n\
                 stderr_inherit: false,\n\
             }\n\
         }\n\n\
         fn rh_command_new_owned(program: String) -> RhCommand {\n\
             rh_command_new(program.as_str())\n\
         }\n\n\
         fn rh_command_args(command: &mut RhCommand, args: &[String]) {\n\
             command.args.extend(args.iter().cloned());\n\
         }\n\n\
         fn rh_command_arg(command: &mut RhCommand, arg: &str) {\n\
             command.args.push(arg.to_owned());\n\
         }\n\n\
         fn rh_command_env(command: &mut RhCommand, name: &str, value: &str) {\n\
             command.env.push((name.to_owned(), Some(value.to_owned())));\n\
         }\n\n\
         fn rh_command_env_remove(command: &mut RhCommand, name: &str) {\n\
             command.env.push((name.to_owned(), None));\n\
         }\n\n\
         fn rh_command_timeout_ms(command: &mut RhCommand, timeout_ms: INT) {\n\
             if timeout_ms > 0 {\n\
                 command.timeout_ms = timeout_ms.min(3_600_000);\n\
             }\n\
         }\n\n\
         fn rh_command_capture_limit(command: &mut RhCommand, limit: INT) {\n\
             if limit > 0 {\n\
                 if let Ok(limit) = usize::try_from(limit.min(262_144)) {\n\
                     command.capture_limit = limit.max(1);\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_command_current_dir(command: &mut RhCommand, path: &str) {\n\
             command.current_dir = Some(path.to_owned());\n\
         }\n\n\
         fn rh_command_stdin_text(command: &mut RhCommand, text: &str) {\n\
             command.stdin_text = Some(text.to_owned());\n\
         }\n\n\
         fn rh_command_write_stdin(child: &mut std::process::Child, stdin_text: &Option<String>) {\n\
             if let Some(text) = stdin_text {\n\
                 if let Some(mut stdin) = child.stdin.take() {\n\
                     use std::io::Write;\n\
                     let _ = stdin.write_all(text.as_bytes());\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_command_stdout_file(command: &mut RhCommand, path: &str) {\n\
             command.stdout_file = Some(path.to_owned());\n\
         }\n\n\
         fn rh_command_stderr_file(command: &mut RhCommand, path: &str) {\n\
             command.stderr_file = Some(path.to_owned());\n\
         }\n\n\
         fn rh_command_stderr_inherit(command: &mut RhCommand) {\n\
             command.stderr_inherit = true;\n\
         }\n\n\
         fn rh_command_build(command: &RhCommand) -> std::process::Command {\n\
             let mut process = std::process::Command::new(&command.program);\n\
             process.args(&command.args);\n\
             for (name, value) in &command.env {\n\
                 match value {\n\
                     Some(value) => { process.env(name, value); }\n\
                     None => { process.env_remove(name); }\n\
                 }\n\
             }\n\
             if let Some(dir) = &command.current_dir {\n\
                 process.current_dir(dir);\n\
             }\n\
             if command.stdin_text.is_some() {\n\
                 process.stdin(std::process::Stdio::piped());\n\
             } else {\n\
                 process.stdin(std::process::Stdio::null());\n\
             }\n\
             if let Some(path) = &command.stdout_file {\n\
                 match std::fs::File::create(path) {\n\
                     Ok(file) => { process.stdout(std::process::Stdio::from(file)); }\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"process_stdout_file: {error}\"));\n\
                         process.stdout(std::process::Stdio::null());\n\
                     }\n\
                 }\n\
             } else {\n\
                 process.stdout(std::process::Stdio::piped());\n\
             }\n\
             if command.stderr_inherit {\n\
                 // Live streaming: the child writes the caller's own stderr\n\
                 // (valid through the GUI-subsystem console-worker chain\n\
                 // because the worker attaches the caller's console and owns\n\
                 // real std handles). Nothing is captured on this path.\n\
                 process.stderr(std::process::Stdio::inherit());\n\
             } else if let Some(path) = &command.stderr_file {\n\
                 match std::fs::File::create(path) {\n\
                     Ok(file) => { process.stderr(std::process::Stdio::from(file)); }\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"process_stderr_file: {error}\"));\n\
                         process.stderr(std::process::Stdio::null());\n\
                     }\n\
                 }\n\
             } else {\n\
                 process.stderr(std::process::Stdio::piped());\n\
             }\n\
             process\n\
         }\n\n\
         fn rh_read_pipe_limited(mut reader: impl std::io::Read, limit: usize) -> (String, bool) {\n\
             let mut bytes = Vec::new();\n\
             let mut chunk = [0_u8; 8192];\n\
             let mut truncated = false;\n\
             loop {\n\
                 match reader.read(&mut chunk) {\n\
                     Ok(0) => break,\n\
                     Ok(count) => {\n\
                         let room = limit.saturating_sub(bytes.len());\n\
                         let take = count.min(room);\n\
                         bytes.extend_from_slice(&chunk[..take]);\n\
                         if take < count {\n\
                             truncated = true;\n\
                         }\n\
                     }\n\
                     Err(_) => break,\n\
                 }\n\
             }\n\
             (String::from_utf8(bytes).unwrap_or_default(), truncated)\n\
         }\n\n\
         fn rh_start_pipe_reader<R>(\n\
             reader: Option<R>,\n\
             limit: usize,\n\
         ) -> Option<std::thread::JoinHandle<(String, bool)>>\n\
         where\n\
             R: std::io::Read + Send + 'static,\n\
         {\n\
             reader.map(|pipe| {\n\
                 std::thread::spawn(move || rh_read_pipe_limited(pipe, limit))\n\
             })\n\
         }\n\n\
         fn rh_finish_pipe_reader(\n\
             reader: Option<std::thread::JoinHandle<(String, bool)>>,\n\
         ) -> (String, bool) {\n\
             reader\n\
                 .and_then(|reader| reader.join().ok())\n\
                 .unwrap_or_else(|| (String::new(), false))\n\
         }\n\n\
         fn rh_finish_process_output(\n\
             mut child: std::process::Child,\n\
             timeout: std::time::Duration,\n\
             capture_limit: usize,\n\
             program: &str,\n\
             args_preview: &str,\n\
         ) -> RhOutput {\n\
             // Drain both pipes while the child is alive. Waiting first can\n\
             // deadlock as soon as either stream fills the OS pipe buffer.\n\
             // Readers continue discarding after the capture limit so a\n\
             // deliberately bounded result never blocks an unbounded child.\n\
             let mut stdout_reader =\n\
                 rh_start_pipe_reader(child.stdout.take(), capture_limit);\n\
             let mut stderr_reader =\n\
                 rh_start_pipe_reader(child.stderr.take(), capture_limit);\n\
             let deadline = std::time::Instant::now() + timeout;\n\
             let status = loop {\n\
                 match child.try_wait() {\n\
                     Ok(Some(status)) => break status,\n\
                     Ok(None) => {\n\
                         if std::time::Instant::now() >= deadline {\n\
                             let _ = child.kill();\n\
                             let _ = child.wait();\n\
                             // Name the culprit: a bare label made CI\n\
                             // timeouts undiagnosable. Args identify which\n\
                             // task a staged worker copy was running.\n\
                             let _ = rh_fail(&format!(\n\
                                 \"process_timeout: {program} {:?} after {}ms\",\n\
                                 args_preview,\n\
                                 timeout.as_millis()\n\
                             ));\n\
                             return RhOutput {\n\
                                 success: 0,\n\
                                 exit_code: -1,\n\
                                 stdout: String::new(),\n\
                                 stderr: String::new(),\n\
                             };\n\
                         }\n\
                         std::thread::sleep(std::time::Duration::from_millis(10));\n\
                     }\n\
                     Err(error) => {\n\
                         let _ = child.kill();\n\
                         let _ = child.wait();\n\
                         let _ = rh_fail(&format!(\"process_wait: {error}\"));\n\
                         return RhOutput {\n\
                             success: 0,\n\
                             exit_code: -1,\n\
                             stdout: String::new(),\n\
                             stderr: String::new(),\n\
                         };\n\
                     }\n\
                 }\n\
             };\n\
             let (stdout, _) = rh_finish_pipe_reader(stdout_reader.take());\n\
             let (stderr, _) = rh_finish_pipe_reader(stderr_reader.take());\n\
             RhOutput {\n\
                 success: i64::from(status.success()),\n\
                 exit_code: status.code().unwrap_or(-1) as INT,\n\
                 stdout,\n\
                 stderr,\n\
             }\n\
         }\n\n\
         fn rh_command_output(command: &mut RhCommand) -> RhOutput {\n\
             let timeout =\n\
                 std::time::Duration::from_millis(command.timeout_ms.max(1).max(0) as u64);\n\
             let capture_limit = command.capture_limit;\n\
             let stdin_text = command.stdin_text.clone();\n\
             let mut process = rh_command_build(command);\n\
             match process.spawn() {\n\
                 Ok(mut child) => {\n\
                     rh_command_write_stdin(&mut child, &stdin_text);\n\
                     let args_preview = command\n\
                         .args\n\
                         .iter()\n\
                         .take(8)\n\
                         .cloned()\n\
                         .collect::<Vec<_>>()\n\
                         .join(\" \");\n\
                     rh_finish_process_output(\n\
                         child,\n\
                         timeout,\n\
                         capture_limit,\n\
                         &command.program,\n\
                         &args_preview,\n\
                     )\n\
                 }\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"process_spawn: {error}\"));\n\
                     RhOutput {\n\
                         success: 0,\n\
                         exit_code: -1,\n\
                         stdout: String::new(),\n\
                         stderr: String::new(),\n\
                     }\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_command_start(command: &mut RhCommand) -> RhChild {\n\
             let stdin_text = command.stdin_text.clone();\n\
             let mut process = rh_command_build(command);\n\
             match process.spawn() {\n\
                 Ok(mut child) => {\n\
                     rh_command_write_stdin(&mut child, &stdin_text);\n\
                     RhChild::new(child.id() as INT, Some(child), command.capture_limit)\n\
                 }\n\
                 Err(error) => {\n\
                     let _ = rh_fail(&format!(\"process_spawn: {error}\"));\n\
                     RhChild::exited(0, command.capture_limit)\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_output_stdout_text(output: &RhOutput) -> String {\n\
             output.stdout.clone()\n\
         }\n\n\
         fn rh_output_stderr_text(output: &RhOutput) -> String {\n\
             output.stderr.clone()\n\
         }\n\n\
         fn rh_output_require_success(output: &RhOutput, message: &str) -> INT {\n\
             if output.success != 0 {\n\
                 return 0;\n\
             }\n\
             rh_fail(message)\n\
         }\n\n\
         fn rh_child_state(child: &mut RhChild) -> String {\n\
             let mut inner = child.inner.borrow_mut();\n\
             if inner.state == \"exited\" {\n\
                 return inner.state.clone();\n\
             }\n\
             if let Some(process) = inner.child.as_mut() {\n\
                 match process.try_wait() {\n\
                     Ok(Some(_)) => inner.state = String::from(\"exited\"),\n\
                     Ok(None) => {}\n\
                     Err(error) => {\n\
                         let _ = rh_fail(&format!(\"process_try_wait: {error}\"));\n\
                         inner.state = String::from(\"exited\");\n\
                     }\n\
                 }\n\
             } else {\n\
                 inner.state = String::from(\"exited\");\n\
             }\n\
             inner.state.clone()\n\
         }\n\n\
         fn rh_child_platform_facts(child: &mut RhChild) -> serde_json::Value {\n\
             let pid = child.inner.borrow().pid;\n\
             rh_host_json_call(\"process.platform_facts\", &serde_json::json!({ \"pid\": pid }))\n\
         }\n\n\
        fn rh_child_stdout(child: &mut RhChild) -> RhStream {\n\
            let pipe = child\n\
                .inner\n\
                .borrow_mut()\n\
                .child\n\
                .as_mut()\n\
                .and_then(|process| process.stdout.take());\n\
            let (sender, receiver) = std::sync::mpsc::channel();\n\
            if let Some(mut pipe) = pipe {\n\
                std::thread::spawn(move || {\n\
                    use std::io::Read as _;\n\
                    let mut buffer = [0_u8; 8192];\n\
                    loop {\n\
                        match pipe.read(&mut buffer) {\n\
                            Ok(0) | Err(_) => break,\n\
                            Ok(count) => {\n\
                                if sender.send(buffer[..count].to_vec()).is_err() {\n\
                                    break;\n\
                                }\n\
                            }\n\
                        }\n\
                    }\n\
                });\n\
            }\n\
            RhStream { receiver }\n\
        }\n\n\
        fn rh_child_stderr(child: &mut RhChild) -> RhStream {\n\
            let pipe = child\n\
                .inner\n\
                .borrow_mut()\n\
                .child\n\
                .as_mut()\n\
                .and_then(|process| process.stderr.take());\n\
            let (sender, receiver) = std::sync::mpsc::channel();\n\
            if let Some(mut pipe) = pipe {\n\
                std::thread::spawn(move || {\n\
                    use std::io::Read as _;\n\
                    let mut buffer = [0_u8; 8192];\n\
                    loop {\n\
                        match pipe.read(&mut buffer) {\n\
                            Ok(0) | Err(_) => break,\n\
                            Ok(count) => {\n\
                                if sender.send(buffer[..count].to_vec()).is_err() {\n\
                                    break;\n\
                                }\n\
                            }\n\
                        }\n\
                    }\n\
                });\n\
            }\n\
            RhStream { receiver }\n\
        }\n\n\
        fn rh_stream_read(stream: &mut RhStream, limit: INT, timeout_ms: INT) -> RhBytes {\n\
            let timeout = std::time::Duration::from_millis(timeout_ms.max(0) as u64);\n\
            let mut bytes = stream.receiver.recv_timeout(timeout).unwrap_or_default();\n\
            bytes.truncate(usize::try_from(limit.max(0)).unwrap_or(0));\n\
            RhBytes { bytes }\n\
        }\n\n\
        fn rh_bytes_to_text(bytes: &RhBytes) -> String {\n\
            String::from_utf8_lossy(&bytes.bytes).into_owned()\n\
        }\n\n\
        fn rh_bytes_len(bytes: &RhBytes) -> INT {\n\
            bytes.bytes.len() as INT\n\
        }\n\n\
        fn rh_bytes_from_text(text: &str) -> RhBytes {\n\
            RhBytes {\n\
                bytes: text.as_bytes().to_vec(),\n\
            }\n\
        }\n\n\
        fn rh_bytes_from_array(values: &[u8]) -> RhBytes {\n\
            RhBytes {\n\
                bytes: values.to_vec(),\n\
            }\n\
        }\n\n\
        fn rh_bytes_append(target: &mut RhBytes, other: &RhBytes) {\n\
            target.bytes.extend_from_slice(&other.bytes);\n\
        }\n\n\
        fn rh_child_window_key(child: &mut RhChild, key: &str) -> INT {\n\
            let pid = child.inner.borrow().pid;\n\
            let result = rh_host_json_call(\n\
                \"process.window_key\",\n\
                &serde_json::json!({ \"pid\": pid, \"key\": key }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"process_window_key\")\n\
            }\n\
        }\n\n\
        #[derive(Clone, Copy)]\n\
        struct RhWindowRect {\n\
            left: INT,\n\
            top: INT,\n\
            right: INT,\n\
            bottom: INT,\n\
        }\n\n\
        #[derive(Clone)]\n\
        struct RhWindowControl {\n\
            child: RhChild,\n\
            id: INT,\n\
        }\n\n\
        fn rh_child_pid(child: &RhChild) -> INT {\n\
            child.inner.borrow().pid\n\
        }\n\n\
        fn rh_window_rect_from_json(value: &serde_json::Value) -> RhWindowRect {\n\
            RhWindowRect {\n\
                left: value.get(\"left\").and_then(serde_json::Value::as_i64).unwrap_or(0),\n\
                top: value.get(\"top\").and_then(serde_json::Value::as_i64).unwrap_or(0),\n\
                right: value.get(\"right\").and_then(serde_json::Value::as_i64).unwrap_or(0),\n\
                bottom: value.get(\"bottom\").and_then(serde_json::Value::as_i64).unwrap_or(0),\n\
            }\n\
        }\n\n\
        fn rh_child_window_control(child: &mut RhChild, id: INT) -> RhWindowControl {\n\
            let pid = rh_child_pid(child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_control\",\n\
                &serde_json::json!({ \"pid\": pid, \"id\": id }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) != Some(true) {\n\
                let _ = rh_fail(\"process_window_control\");\n\
            }\n\
            RhWindowControl {\n\
                child: child.clone(),\n\
                id,\n\
            }\n\
        }\n\n\
        fn rh_window_control_visible(control: &mut RhWindowControl) -> INT {\n\
            let pid = rh_child_pid(&control.child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_control_visible\",\n\
                &serde_json::json!({ \"pid\": pid, \"id\": control.id }),\n\
            );\n\
            if let Some(visible) = result.get(\"visible\").and_then(serde_json::Value::as_bool) {\n\
                i64::from(visible)\n\
            } else {\n\
                rh_fail(\"process_window_control_visible\")\n\
            }\n\
        }\n\n\
        fn rh_window_control_text(control: &mut RhWindowControl) -> String {\n\
            let pid = rh_child_pid(&control.child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_control_text\",\n\
                &serde_json::json!({ \"pid\": pid, \"id\": control.id }),\n\
            );\n\
            match result.get(\"text\").and_then(serde_json::Value::as_str) {\n\
                Some(text) => text.to_owned(),\n\
                None => {\n\
                    let _ = rh_fail(\"process_window_control_text\");\n\
                    String::new()\n\
                }\n\
            }\n\
        }\n\n\
        fn rh_window_control_set_text(control: &mut RhWindowControl, text: &str) -> INT {\n\
            let pid = rh_child_pid(&control.child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_control_set_text\",\n\
                &serde_json::json!({ \"pid\": pid, \"id\": control.id, \"text\": text }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"process_window_control_set_text\")\n\
            }\n\
        }\n\n\
        fn rh_window_control_click(control: &mut RhWindowControl) -> INT {\n\
            let pid = rh_child_pid(&control.child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_control_click\",\n\
                &serde_json::json!({ \"pid\": pid, \"id\": control.id }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"process_window_control_click\")\n\
            }\n\
        }\n\n\
        fn rh_child_window_message(\n\
            child: &mut RhChild,\n\
            message: INT,\n\
            wparam: INT,\n\
            lparam: INT,\n\
        ) -> INT {\n\
            let pid = rh_child_pid(child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_message\",\n\
                &serde_json::json!({\n\
                    \"pid\": pid,\n\
                    \"message\": message,\n\
                    \"wparam\": wparam,\n\
                    \"lparam\": lparam,\n\
                }),\n\
            );\n\
            result\n\
                .get(\"value\")\n\
                .and_then(serde_json::Value::as_i64)\n\
                .unwrap_or_else(|| rh_fail(\"process_window_message\"))\n\
        }\n\n\
        fn rh_child_window_pointer(child: &mut RhChild, action: &str, x: INT, y: INT) -> INT {\n\
            let pid = rh_child_pid(child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_pointer\",\n\
                &serde_json::json!({ \"pid\": pid, \"action\": action, \"x\": x, \"y\": y }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"process_window_pointer\")\n\
            }\n\
        }\n\n\
        fn rh_child_window_resize(child: &mut RhChild, width: INT, height: INT) -> INT {\n\
            let pid = rh_child_pid(child);\n\
            let result = rh_host_json_call(\n\
                \"process.window_resize\",\n\
                &serde_json::json!({ \"pid\": pid, \"width\": width, \"height\": height }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"process_window_resize\")\n\
            }\n\
        }\n\n\
        fn rh_child_window_rect(child: &mut RhChild, client: bool) -> RhWindowRect {\n\
            let pid = rh_child_pid(child);\n\
            let result = rh_host_json_call(\n\
                if client { \"process.window_client_rect\" } else { \"process.window_rect\" },\n\
                &serde_json::json!({ \"pid\": pid }),\n\
            );\n\
            if result.get(\"left\").is_some() {\n\
                rh_window_rect_from_json(&result)\n\
            } else {\n\
                let _ = rh_fail(if client {\n\
                    \"process_window_client_rect\"\n\
                } else {\n\
                    \"process_window_rect\"\n\
                });\n\
                RhWindowRect {\n\
                    left: 0,\n\
                    top: 0,\n\
                    right: 0,\n\
                    bottom: 0,\n\
                }\n\
            }\n\
        }\n\n\
        fn rh_clipboard_get_text() -> String {\n\
            let result = rh_host_json_call(\n\
                \"clipboard.get_text\",\n\
                &serde_json::json!({}),\n\
            );\n\
            match result.get(\"text\").and_then(serde_json::Value::as_str) {\n\
                Some(text) => text.to_owned(),\n\
                None => {\n\
                    let _ = rh_fail(\"clipboard_get_text\");\n\
                    String::new()\n\
                }\n\
            }\n\
        }\n\n\
        fn rh_clipboard_set_text(text: &str) -> INT {\n\
            let result = rh_host_json_call(\n\
                \"clipboard.set_text\",\n\
                &serde_json::json!({ \"text\": text }),\n\
            );\n\
            if result.get(\"ok\").and_then(serde_json::Value::as_bool) == Some(true) {\n\
                0\n\
            } else {\n\
                rh_fail(\"clipboard_set_text\")\n\
            }\n\
        }\n\n\
         fn rh_child_kill(child: &mut RhChild) -> INT {\n\
             let mut inner = child.inner.borrow_mut();\n\
             if let Some(process) = inner.child.as_mut() {\n\
                 if let Err(error) = process.kill() {\n\
                     let _ = rh_fail(&format!(\"process_kill: {error}\"));\n\
                 }\n\
             }\n\
             0\n\
         }\n\n\
         fn rh_child_wait_with_output(child: &mut RhChild, timeout_ms: INT) -> RhOutput {\n\
             let timeout = std::time::Duration::from_millis(timeout_ms.max(1).max(0) as u64);\n\
             let mut inner = child.inner.borrow_mut();\n\
             inner.state = String::from(\"exited\");\n\
             let capture_limit = inner.capture_limit;\n\
             let Some(process) = inner.child.take() else {\n\
                 return RhOutput {\n\
                     success: 0,\n\
                     exit_code: -1,\n\
                     stdout: String::new(),\n\
                     stderr: String::new(),\n\
                 };\n\
             };\n\
             rh_finish_process_output(process, timeout, capture_limit, \"child\", \"\")\n\
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
         fn rh_process_apply_options(request: &mut serde_json::Value, options: Option<&serde_json::Value>) {\n\
             let Some(options) = options else {\n\
                 return;\n\
             };\n\
             if let Some(current_dir) = options.get(\"current_dir\").and_then(|value| value.as_str()) {\n\
                 request[\"current_dir\"] = serde_json::Value::String(current_dir.to_owned());\n\
             }\n\
             if let Some(env) = options.get(\"env\").and_then(|value| value.as_object()) {\n\
                 request[\"env\"] = serde_json::Value::Object(env.clone());\n\
             }\n\
             if let Some(removes) = options.get(\"env_remove\").and_then(|value| value.as_array()) {\n\
                 request[\"env_remove\"] = serde_json::Value::Array(removes.clone());\n\
             }\n\
         }\n\n\
         fn rh_process_status(program: &str, args: &[String], timeout_ms: INT, options: Option<&serde_json::Value>) -> INT {\n\
             let mut request = serde_json::json!({\n\
                 \"program\": program,\n\
                 \"args\": args,\n\
                 \"timeout_ms\": timeout_ms,\n\
             });\n\
             rh_process_apply_options(&mut request, options);\n\
             rh_utility(3, &request.to_string())\n\
         }\n\n\
         fn rh_process_stdout_file(\n\
             program: &str,\n\
             args: &[String],\n\
             timeout_ms: INT,\n\
             stdout_path: &str,\n\
             options: Option<&serde_json::Value>,\n\
         ) -> INT {\n\
             let mut request = serde_json::json!({\n\
                 \"program\": program,\n\
                 \"args\": args,\n\
                 \"timeout_ms\": timeout_ms,\n\
                 \"stdout_path\": stdout_path,\n\
             });\n\
             rh_process_apply_options(&mut request, options);\n\
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
         fn rh_json_stringify(value: &serde_json::Value) -> String {\n\
             match serde_json::to_string(value) {\n\
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
         fn rh_json_array_push_path(\n\
             target: &mut serde_json::Value,\n\
             path: &[&str],\n\
             item: serde_json::Value,\n\
         ) -> INT {\n\
             match rh_json_path_mut(target, path) {\n\
                 Some(node) => rh_json_array_push(node, item),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_array_push_path: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_array_insert(\n\
             target: &mut serde_json::Value,\n\
             index: INT,\n\
             item: serde_json::Value,\n\
         ) -> INT {\n\
             let Some(items) = target.as_array_mut() else {\n\
                 let _ = rh_fail(\"json_array_insert_target\");\n\
                 return 0;\n\
             };\n\
             let Ok(index) = usize::try_from(index) else {\n\
                 let _ = rh_fail(\"json_array_insert_index\");\n\
                 return 0;\n\
             };\n\
             if index > items.len() {\n\
                 let _ = rh_fail(\"json_array_insert_index\");\n\
                 return 0;\n\
             }\n\
             items.insert(index, item);\n\
             0\n\
         }\n\n\
         fn rh_json_remove(target: &mut serde_json::Value, key: &str) -> INT {\n\
             match target.as_object_mut() {\n\
                 Some(object) => {\n\
                     object.remove(key);\n\
                     0\n\
                 }\n\
                 None => {\n\
                     let _ = rh_fail(\"json_remove_target\");\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_get_path_key(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
             key: &str,\n\
         ) -> serde_json::Value {\n\
             match rh_json_path(value, path).and_then(|node| node.get(key)) {\n\
                 Some(item) => item.clone(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_get_path_key: {}.{}\", path.join(\".\"), key));\n\
                     serde_json::Value::Null\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_int_path_key_field(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
             key: &str,\n\
             field: &str,\n\
         ) -> INT {\n\
             match rh_json_get_path_key(value, path, key).get(field) {\n\
                 Some(node) => rh_json_as_i64(node),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\n\
                         \"json_int_path_key_field: {}.{key}.{field}\",\n\
                         path.join(\".\")\n\
                     ));\n\
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
         fn rh_json_get_path_index(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
             index: INT,\n\
         ) -> serde_json::Value {\n\
             match rh_json_path(value, path) {\n\
                 Some(array) => rh_json_array_get(array, index),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_path: {}\", path.join(\".\")));\n\
                     serde_json::Value::Null\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_string_path_index(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
             index: INT,\n\
         ) -> String {\n\
             rh_json_as_str(&rh_json_get_path_index(value, path, index))\n\
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
         fn rh_string_list_set(items: &mut Vec<String>, index: INT, value: &str) -> INT {\n\
             match usize::try_from(index) {\n\
                 Ok(index) if index < items.len() => {\n\
                     items[index] = value.to_owned();\n\
                     0\n\
                 }\n\
                 _ => {\n\
                     let _ = rh_fail(&format!(\"string_list_index: {index}\"));\n\
                     0\n\
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
         fn rh_json_intish(value: &serde_json::Value) -> Option<INT> {\n\
             if let Some(number) = value.as_i64() {\n\
                 return Some(number as INT);\n\
             }\n\
             if let Some(number) = value.as_f64() {\n\
                 if number.is_finite() && number >= INT::MIN as f64 && number <= INT::MAX as f64 {\n\
                     return Some(number as INT);\n\
                 }\n\
             }\n\
             // INT-only packs treat JSON booleans as 0/1 so `require(doc.flag)` works.\n\
             if let Some(flag) = value.as_bool() {\n\
                 return Some(i64::from(flag));\n\
             }\n\
             None\n\
         }\n\n\
         fn rh_json_int_path(value: &serde_json::Value, path: &[&str]) -> INT {\n\
             // Absent/null reads 0 -- the interpreter's is-absent idiom\n\
             // (`0 + row.maybe.field`), which live smokes rely on for\n\
             // optional fields like cleanup_receipt. A PRESENT non-numeric\n\
             // value is still a type error and fails closed.\n\
             match rh_json_path(value, path) {\n\
                 None | Some(serde_json::Value::Null) => 0,\n\
                 Some(node) => match rh_json_intish(node) {\n\
                     Some(value) => value,\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_integer_path: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
             }\n\
         }\n\n\
         fn rh_json_array_len(value: &serde_json::Value, path: &[&str]) -> INT {\n\
             // `.len` mirrors the interpreter: array length for arrays,\n\
             // char count for JSON strings (native-ipc-smoke checks\n\
             // `scope_id.len == 39` on a json-sourced string). Everything\n\
             // else stays fail-closed.\n\
             match rh_json_path(value, path) {\n\
                 Some(serde_json::Value::Array(items)) => items.len() as INT,\n\
                 Some(serde_json::Value::String(text)) => text.chars().count() as INT,\n\
                 _ => {\n\
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
         fn rh_json_object_keys(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
         ) -> Vec<String> {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_object) {\n\
                 Some(map) => map.keys().cloned().collect(),\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_object_path: {}\", path.join(\".\")));\n\
                     Vec::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_object_keys_len(value: &serde_json::Value, path: &[&str]) -> INT {\n\
             match rh_json_path(value, path).and_then(serde_json::Value::as_object) {\n\
                 Some(map) => map.len() as INT,\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_object_path: {}\", path.join(\".\")));\n\
                     0\n\
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
             match rh_json_intish(value) {\n\
                 Some(value) => value,\n\
                 None => {\n\
                     let _ = rh_fail(\"json_integer_value\");\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_scalar_string(value: &serde_json::Value) -> Option<String> {\n\
             // Scalars stringify exactly as the interpreter's `\"\" + value`\n\
             // does. The AOT lane used to hard-fail on numbers and bools\n\
             // here -- an interpreter/native semantic divergence that broke\n\
             // every gate whose message concatenated a JSON number (budget\n\
             // gates, qualification timing). Structural values (null, array,\n\
             // object) still fail closed: those in a string slot are a logic\n\
             // error, not a formatting choice.\n\
             match value {\n\
                 serde_json::Value::String(text) => Some(text.clone()),\n\
                 serde_json::Value::Number(number) => Some(number.to_string()),\n\
                 serde_json::Value::Bool(flag) => Some(flag.to_string()),\n\
                 // Null reads as the empty string: `(\"\" + doc.field) == \"\"`\n\
                 // is the established is-absent idiom across the gate\n\
                 // scripts (bootstrap_timing::wall_time, control-center\n\
                 // smoke), written against the interpreter behavior.\n\
                 serde_json::Value::Null => Some(String::new()),\n\
                 _ => None,\n\
             }\n\
         }\n\n\
         fn rh_json_as_str(value: &serde_json::Value) -> String {\n\
             match rh_json_scalar_string(value) {\n\
                 Some(value) => value,\n\
                 None => {\n\
                     let _ = rh_fail(\"json_string_value\");\n\
                     String::new()\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_string_path(value: &serde_json::Value, path: &[&str]) -> String {\n\
             // Absent reads \"\" -- same is-absent idiom as rh_json_int_path\n\
             // (rh_json_scalar_string already maps null to \"\"). Present\n\
             // structural values (arrays/objects) still fail closed.\n\
             match rh_json_path(value, path) {\n\
                 None => String::new(),\n\
                 Some(node) => match rh_json_scalar_string(node) {\n\
                     Some(value) => value,\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_string_path: {}\", path.join(\".\")));\n\
                         String::new()\n\
                     }\n\
                 },\n\
             }\n\
         }\n\n\
         fn rh_json_contains_path(\n\
             value: &serde_json::Value,\n\
             path: &[&str],\n\
             needle: &serde_json::Value,\n\
         ) -> INT {\n\
             match rh_json_path(value, path) {\n\
                 Some(serde_json::Value::String(text)) => match needle.as_str() {\n\
                     Some(needle) => text.contains(needle) as INT,\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_string_contains: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
                 Some(serde_json::Value::Array(items)) => {\n\
                     items.iter().any(|item| item == needle) as INT\n\
                 }\n\
                 Some(serde_json::Value::Object(items)) => match needle.as_str() {\n\
                     Some(key) => items.contains_key(key) as INT,\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_object_contains: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
                 Some(_) | None => {\n\
                     let _ = rh_fail(&format!(\"json_contains_path: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_path_mut<'a>(\n\
             value: &'a mut serde_json::Value,\n\
             path: &[&str],\n\
         ) -> Option<&'a mut serde_json::Value> {\n\
             match path.split_first() {\n\
                 None => Some(value),\n\
                 Some((segment, rest)) => {\n\
                     let next = value.get_mut(*segment)?;\n\
                     rh_json_path_mut(next, rest)\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_set_path(\n\
             target: &mut serde_json::Value,\n\
             path: &[&str],\n\
             value: serde_json::Value,\n\
         ) -> INT {\n\
             match path.split_last() {\n\
                 None => {\n\
                     *target = value;\n\
                     0\n\
                 }\n\
                 Some((field, parent_path)) => match rh_json_path_mut(target, parent_path) {\n\
                     Some(parent) => match parent.as_object_mut() {\n\
                         Some(map) => {\n\
                             map.insert((*field).to_string(), value);\n\
                             0\n\
                         }\n\
                         None => {\n\
                             let _ = rh_fail(&format!(\"json_set_path: {}\", path.join(\".\")));\n\
                             0\n\
                         }\n\
                     },\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_set_path: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
             }\n\
         }\n\n\
         fn rh_json_set_path_key(\n\
             target: &mut serde_json::Value,\n\
             path: &[&str],\n\
             key: &str,\n\
             value: serde_json::Value,\n\
         ) -> INT {\n\
             match rh_json_path_mut(target, path) {\n\
                 Some(node) => match node.as_object_mut() {\n\
                     Some(map) => {\n\
                         map.insert(key.to_string(), value);\n\
                         0\n\
                     }\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_set_path_key: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_set_path_key: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_set_path_index(\n\
             target: &mut serde_json::Value,\n\
             path: &[&str],\n\
             index: INT,\n\
             value: serde_json::Value,\n\
         ) -> INT {\n\
             match rh_json_path_mut(target, path) {\n\
                 Some(node) => match node.as_array_mut() {\n\
                     Some(items) => match usize::try_from(index) {\n\
                         Ok(index) => {\n\
                             if index <= items.len() {\n\
                                 if index == items.len() {\n\
                                     items.push(value);\n\
                                 } else {\n\
                                     items[index] = value;\n\
                                 }\n\
                                 0\n\
                             } else {\n\
                                 let _ = rh_fail(&format!(\n\
                                     \"json_set_path_index: {}[{index}]\",\n\
                                     path.join(\".\")\n\
                                 ));\n\
                                 0\n\
                             }\n\
                         }\n\
                         Err(_) => {\n\
                             let _ = rh_fail(&format!(\n\
                                 \"json_set_path_index: {}[{index}]\",\n\
                                 path.join(\".\")\n\
                             ));\n\
                             0\n\
                         }\n\
                     },\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\"json_set_path_index: {}\", path.join(\".\")));\n\
                         0\n\
                     }\n\
                 },\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\"json_set_path_index: {}\", path.join(\".\")));\n\
                     0\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_set_path_index_field(\n\
             target: &mut serde_json::Value,\n\
             path: &[&str],\n\
             index: INT,\n\
             field: &str,\n\
             value: serde_json::Value,\n\
         ) -> INT {\n\
             match rh_json_path_mut(target, path) {\n\
                 Some(node) => match node.as_array_mut() {\n\
                     Some(items) => match usize::try_from(index)\n\
                         .ok()\n\
                         .and_then(|index| items.get_mut(index))\n\
                     {\n\
                         Some(item) => match item.as_object_mut() {\n\
                             Some(map) => {\n\
                                 map.insert(field.to_string(), value);\n\
                                 0\n\
                             }\n\
                             None => {\n\
                                 let _ = rh_fail(&format!(\n\
                                     \"json_set_path_index_field: {}[{index}].{field}\",\n\
                                     path.join(\".\")\n\
                                 ));\n\
                                 0\n\
                             }\n\
                         },\n\
                         None => {\n\
                             let _ = rh_fail(&format!(\n\
                                 \"json_set_path_index_field: {}[{index}].{field}\",\n\
                                 path.join(\".\")\n\
                             ));\n\
                             0\n\
                         }\n\
                     },\n\
                     None => {\n\
                         let _ = rh_fail(&format!(\n\
                             \"json_set_path_index_field: {}[{index}].{field}\",\n\
                             path.join(\".\")\n\
                         ));\n\
                         0\n\
                     }\n\
                 },\n\
                 None => {\n\
                     let _ = rh_fail(&format!(\n\
                         \"json_set_path_index_field: {}[{index}].{field}\",\n\
                         path.join(\".\")\n\
                     ));\n\
                     0\n\
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
                     // Match Rhai `type_of(())` so optional JSON probes stay native\n\
                     // without try/catch or hard failure on missing paths.\n\
                     String::from(\"()\")\n\
                 }\n\
             }\n\
         }\n\n\
         fn rh_json_type_name_value(value: &serde_json::Value) -> String {\n\
             rh_json_type_name(value, &[])\n\
         }\n\n",
    );
}

// Back-compat alias used by older tests/docs.
pub const RH_HOST_FLEET_OUT_CAP: u32 = RH_HOST_OUT_CAP;

#[cfg(test)]
mod tests {
    #[test]
    fn generated_process_output_drains_pipes_before_waiting() {
        let mut runtime = String::new();
        super::emit_host_runtime(&mut runtime);

        let reader = runtime
            .find("rh_start_pipe_reader(child.stdout.take()")
            .expect("stdout reader starts");
        let wait = runtime
            .find("match child.try_wait()")
            .expect("child wait loop");
        assert!(reader < wait, "pipes must drain while the child is alive");
        assert!(runtime.contains("limit.saturating_sub(bytes.len())"));
        assert!(!runtime.contains("if bytes.len() >= limit"));
        assert!(
            runtime.contains("Some(serde_json::Value::Object(items)) => match needle.as_str()"),
            "JSON object contains must preserve Rhai Map key-membership semantics"
        );
        assert!(runtime.contains("items.contains_key(key) as INT"));
    }
}
