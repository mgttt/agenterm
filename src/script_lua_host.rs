//! Lua host bridge — fleet call / args / print callbacks for Lua scripts.

use std::sync::Arc;

/// Fleet bridge injected into Lua host: (operation_id, params_json) → result JSON.
pub type LuaFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;
