//! qjs host bridge — fleet call / args / print callbacks for qjs scripts.
//!
//! Same shape as `script_lua_host::LuaFleetBridgeFn` / `script_rh_host::FleetBridgeFn`
//! by design (see `crates/agenterm-qjs/src/host.rs` module doc for why).

use std::sync::Arc;

/// Fleet bridge injected into qjs host: (operation_id, params_json) → result JSON.
pub type QjsFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;
