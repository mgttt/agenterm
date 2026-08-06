//! C ABI between rh native packs and the embedding host (worker, gateway, CC).

pub const RH_HOST_API_VERSION: u32 = 1;
pub const RH_HOST_FLEET_OUT_CAP: u32 = 65536;

pub type RhHostFleetCall = extern "C" fn(
    operation_id: *const u8,
    operation_id_len: u32,
    params_json: *const u8,
    params_json_len: u32,
    out_buf: *mut u8,
    out_cap: u32,
) -> i32;

pub fn emit_host_runtime(out: &mut String) {
    out.push_str(
        "type RhHostFleetCall = extern \"C\" fn(*const u8, u32, *const u8, u32, *mut u8, u32) -> i32;\n\n\
         static mut RH_HOST_FLEET_CALL: Option<RhHostFleetCall> = None;\n\
         static RH_HOST_FLEET_OUT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();\n\n\
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
         fn rh_fleet_call(operation_id: &str, params_json: &str) -> i32 {\n\
             let Some(call) = (unsafe { RH_HOST_FLEET_CALL }) else {\n\
                 return -4;\n\
             };\n\
             let buffer = RH_HOST_FLEET_OUT.get_or_init(|| vec![0u8; ",
    );
    out.push_str(&RH_HOST_FLEET_OUT_CAP.to_string());
    out.push_str(
        "usize]);\n\
             let mut scratch = buffer.clone();\n\
             let wrote = call(\n\
                 operation_id.as_ptr(),\n\
                 operation_id.len() as u32,\n\
                 params_json.as_ptr(),\n\
                 params_json.len() as u32,\n\
                 scratch.as_mut_ptr(),\n\
                 scratch.len() as u32,\n\
             );\n\
             wrote\n\
         }\n\n",
    );
}
