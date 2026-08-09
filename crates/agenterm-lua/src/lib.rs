//! LuaJIT scripting backend for AgenTerm.
//!
//! Wraps mlua + LuaJIT to provide `eval` and `check` operations,
//! with host function injection for fleet/args/print callbacks.

mod stdlib;
pub mod check;
pub mod cache;
pub mod check_many;
pub mod cli;
pub mod compile;
pub mod corpus_scan;
pub mod manifest;
pub mod pack;
pub mod qualify;

// `main.rs` is compiled as a `[[bin]]` of the root `agenterm` package (see
// the root Cargo.toml), not as a target of this crate — it can only reach
// `agenterm-script-common` through a re-export here, since the root
// package's own `[dependencies]` doesn't list it directly.
pub use agenterm_script_common::cli::{find_flag_value, has_flag, positional, require_flag_value};

use std::sync::Arc;
use std::sync::OnceLock;

use mlua::{Lua, Value};

/// Cached fleet.lua source for auto-injection.
static FLEET_SOURCE: OnceLock<String> = OnceLock::new();

fn fleet_source() -> &'static str {
    FLEET_SOURCE.get_or_init(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("scripts")
            .join("lua")
            .join("lib")
            .join("fleet.lua");
        std::fs::read_to_string(&path).unwrap_or_default()
    })
}

/// Type alias for fleet call host function.
pub type FleetCallFn = Arc<dyn Fn(String, String) -> Result<String, String> + Send + Sync>;
/// Type alias for arg host function.
pub type ArgFn = Arc<dyn Fn(i64) -> Result<String, String> + Send + Sync>;

/// A configured Lua engine with optional host functions.
pub struct LuaEngine {
    lua: Lua,
}

/// Host functions injectable into Lua scripts as `__host.*` globals.
#[derive(Default)]
pub struct LuaHostFunctions {
    /// Called as `__host.fleet_call(operation_id, params_json) -> result_json`.
    pub fleet_call: Option<FleetCallFn>,
    /// Called as `__host.args_len() -> int`.
    pub args_len: Option<Arc<dyn Fn() -> i64 + Send + Sync>>,
    /// Called as `__host.arg(index) -> string`.
    pub arg: Option<ArgFn>,
    /// Called as `__host.print(text)` — captured into output buffer.
    pub print: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

#[derive(Debug)]
pub enum LuaError {
    Parse(String),
    Runtime(String),
    Host(String),
    Engine(String),
}

impl std::fmt::Display for LuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "lua_parse: {e}"),
            Self::Runtime(e) => write!(f, "lua_runtime: {e}"),
            Self::Host(e) => write!(f, "lua_host: {e}"),
            Self::Engine(e) => write!(f, "lua_engine: {e}"),
        }
    }
}

impl std::error::Error for LuaError {}

/// Result of a Lua `eval` call.
#[derive(Debug)]
pub struct LuaEvalResult {
    /// Exit code (i64-compatible value).
    pub value: i64,
    /// Captured print output.
    pub stdout: String,
}

impl LuaEngine {
    /// Create a new LuaJIT engine.
    pub fn new() -> Result<Self, LuaError> {
        let lua = Lua::new();
        // Sandbox: remove dangerous globals. Keep `load` for syntax checking.
        lua.globals()
            .set("loadfile", mlua::Value::Nil)
            .map_err(|e| LuaError::Engine(e.to_string()))?;
        lua.globals()
            .set("dofile", mlua::Value::Nil)
            .map_err(|e| LuaError::Engine(e.to_string()))?;
        // Inject std library.
        stdlib::inject(&lua).map_err(|e| LuaError::Engine(e.to_string()))?;
        Ok(Self { lua })
    }

    /// Check Lua source for syntax errors.
    pub fn check(&self, source: &str) -> Result<(), LuaError> {
        // Load only — don't execute. This catches parse errors.
        self.lua
            .load(source)
            .into_function()
            .map_err(|e| LuaError::Parse(e.to_string()))?;
        Ok(())
    }

    /// Evaluate Lua source from a file, returning an i64 exit code
    /// and captured print output.
    pub fn eval_file(
        &self,
        path: &std::path::Path,
        host: &LuaHostFunctions,
    ) -> Result<LuaEvalResult, LuaError> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| LuaError::Engine(format!("read_file: {e}")))?;
        self.eval(&source, host)
    }

    /// Evaluate Lua source with optional host functions, returning an i64 exit code
    /// and captured print output.
    pub fn eval(
        &self,
        source: &str,
        host: &LuaHostFunctions,
    ) -> Result<LuaEvalResult, LuaError> {
        let stdout = Arc::new(std::sync::Mutex::new(String::new()));

        // Inject __host table with fleet_call, args_len, arg, print.
        self.inject_host_table(&self.lua, host, &stdout)
            .map_err(LuaError::Host)?;

        // Prepend fleet.lua module source so scripts can use fleet.* APIs.
        let fleet_src = fleet_source();
        let fleet_src = fleet_src.trim_end().strip_suffix("return fleet")
            .unwrap_or(fleet_src)
            .trim_end();
        let full_source = format!("{fleet_src}\n{source}");

        // Execute the script as a chunk, capturing any return value.
        let result: mlua::Result<Value> = self.lua.load(&full_source).eval();

        // Extract return value.
        let value = match result {
            Ok(val) => Self::value_to_i64(val),
            Err(e) => {
                return Err(LuaError::Runtime(e.to_string()));
            }
        };

        let stdout = stdout
            .lock()
            .map_err(|e| LuaError::Host(e.to_string()))?
            .clone();

        Ok(LuaEvalResult { value, stdout })
    }

    /// Evaluate Lua source using the bytecode cache.
    /// Returns `(LuaEvalResult, cache_hit: bool)`.
    pub fn eval_cached(
        &self,
        source: &str,
        host: &LuaHostFunctions,
    ) -> Result<(LuaEvalResult, bool), LuaError> {
        let cached = crate::cache::cached_compile(source)
            .map_err(LuaError::Engine)?;
        let result = self.eval(source, host)?;
        Ok((result, cached.cache_hit))
    }

    /// Evaluate a Lua file with bytecode caching.
    /// Returns `(LuaEvalResult, cache_hit: bool)`.
    pub fn eval_file_cached(
        &self,
        path: &std::path::Path,
        host: &LuaHostFunctions,
    ) -> Result<(LuaEvalResult, bool), LuaError> {
        let source =
            std::fs::read_to_string(path).map_err(|e| LuaError::Engine(format!("read_file: {e}")))?;
        self.eval_cached(&source, host)
    }

    /// Compute SHA256 hash of Lua source.
    pub fn hash_source(source: &str) -> String {
        crate::compile::hash_source(source)
    }

    /// Evaluate Lua source with CLI arguments injected into `__host`.
    /// `args` are exposed as `__host.args_len()` and `__host.arg(n)`.
    pub fn eval_with_args(
        &self,
        source: &str,
        host: &LuaHostFunctions,
        args: &[String],
    ) -> Result<LuaEvalResult, LuaError> {
        let args_vec: Vec<String> = args.to_vec();
        let host_with_args = LuaHostFunctions {
            args_len: Some(Arc::new(move || args_vec.len() as i64)),
            arg: {
                let args = args.to_vec();
                Some(Arc::new(move |i: i64| {
                    args.get(i as usize)
                        .cloned()
                        .ok_or_else(|| format!("argument {i} is unavailable"))
                }))
            },
            fleet_call: host.fleet_call.clone(),
            print: host.print.clone(),
        };
        self.eval(source, &host_with_args)
    }

    fn inject_host_table(
        &self,
        lua: &Lua,
        host: &LuaHostFunctions,
        stdout: &Arc<std::sync::Mutex<String>>,
    ) -> Result<(), String> {
        let host_table = lua.create_table().map_err(|e| e.to_string())?;

        // Inject fleet_call — clone the Arc to satisfy 'static bound.
        if let Some(ref fleet_call) = host.fleet_call {
            let fc = Arc::clone(fleet_call);
            host_table
                .set(
                    "fleet_call",
                    lua.create_function(move |_lua, (op_id, params): (String, String)| {
                        match fc(op_id, params) {
                            Ok(result) => Ok(result),
                            Err(e) => Err(mlua::Error::runtime(format!("fleet_call: {e}"))),
                        }
                    })
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }

        // Inject args_len
        if let Some(ref args_len) = host.args_len {
            let al = Arc::clone(args_len);
            host_table
                .set(
                    "args_len",
                    lua.create_function(move |_lua, ()| Ok(al()))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }

        // Inject arg
        if let Some(ref arg) = host.arg {
            let ar = Arc::clone(arg);
            host_table
                .set(
                    "arg",
                    lua.create_function(move |_lua, index: i64| {
                        ar(index).map_err(|e| mlua::Error::runtime(format!("arg: {e}")))
                    })
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }

        // Inject print
        let print_stdout = Arc::clone(stdout);
        host_table
            .set(
                "print",
                lua.create_function(move |_lua, text: String| {
                    let mut out = print_stdout
                        .lock()
                        .map_err(|e| mlua::Error::runtime(format!("print_lock: {e}")))?;
                    out.push_str(&text);
                    out.push('\n');
                    Ok(())
                })
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;

        lua.globals()
            .set("__host", host_table)
            .map_err(|e| e.to_string())?;

        // Override native print() to capture output into the buffer.
        let print_override = Arc::clone(stdout);
        lua.globals()
            .set(
                "print",
                lua.create_function(move |_lua, text: String| {
                    let mut out = print_override
                        .lock()
                        .map_err(|e| mlua::Error::runtime(format!("print_lock: {e}")))?;
                    out.push_str(&text);
                    out.push('\n');
                    Ok(())
                })
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    fn value_to_i64(value: Value) -> i64 {
        match value {
            Value::Nil => 0,
            Value::Boolean(true) => 1,
            Value::Boolean(false) => 0,
            Value::Integer(n) => n,
            Value::Number(n) => n as i64,
            Value::String(s) => {
                let s: &str = &s.to_string_lossy();
                s.parse::<i64>().unwrap_or(0)
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_returns_i64_value() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        let result = engine.eval("return 42", &host).expect("eval");
        assert_eq!(result.value, 42);
        assert_eq!(result.stdout, "");
    }

    #[test]
    fn eval_captures_print_output() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        let result = engine.eval("print('hello') return 1", &host).expect("eval");
        assert_eq!(result.value, 1);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[test]
    fn check_rejects_syntax_error() {
        let engine = LuaEngine::new().expect("create engine");
        let err = engine.check("return !!").expect_err("syntax error");
        let msg = err.to_string();
        assert!(msg.contains("lua_parse"), "{msg}");
    }

    #[test]
    fn check_accepts_valid_source() {
        let engine = LuaEngine::new().expect("create engine");
        engine.check("return 42").expect("valid source");
    }

    #[test]
    fn eval_calls_host_args() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions {
            args_len: Some(Arc::new(|| 2)),
            arg: Some(Arc::new(|i: i64| {
                if i == 0 {
                    Ok("first".to_string())
                } else {
                    Ok("second".to_string())
                }
            })),
            ..Default::default()
        };
        let result = engine
            .eval(
                "local n = __host.args_len() local a = __host.arg(0) return n",
                &host,
            )
            .expect("eval");
        assert_eq!(result.value, 2);
    }

    #[test]
    fn eval_calls_host_fleet() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions {
            fleet_call: Some(Arc::new(|op: String, params: String| {
                if op == "test" && params == "{}" {
                    Ok("{\"ok\":true}".to_string())
                } else {
                    Err("unknown".to_string())
                }
            })),
            ..Default::default()
        };
        let result = engine
            .eval("local r = __host.fleet_call('test', '{}') return 0", &host)
            .expect("eval");
        assert_eq!(result.value, 0);
    }

    #[test]
    fn eval_fleet_module() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions {
            fleet_call: Some(Arc::new(|op: String, params: String| {
                if op == "tabs.list" && params == "{}" {
                    Ok("[{\"id\":\"tab1\",\"title\":\"Test\"}]".to_string())
                } else if op == "ui.snapshot" && params == "{}" {
                    Ok("{\"width\":1920,\"height\":1080}".to_string())
                } else {
                    Err(format!("unknown_op: {op}"))
                }
            })),
            ..Default::default()
        };

        // Load fleet.lua source and wrap it
        let fleet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("scripts")
            .join("lua")
            .join("lib")
            .join("fleet.lua");
        let fleet_source = std::fs::read_to_string(&fleet_path)
            .expect("read fleet.lua");

        // fleet.lua ends with `return fleet`; strip it so we can append test code
        let fleet_source = fleet_source.trim_end().strip_suffix("return fleet")
            .unwrap_or(&fleet_source)
            .trim_end()
            .to_string();

        let script = format!(
            "{fleet_source}\n\
             local result = fleet.tabs.list()\n\
             assert(result ~= nil, 'tabs.list returned nil')\n\
             assert(type(result) == 'table', 'expected table')\n\
             assert(#result == 1, 'expected 1 tab')\n\
             assert(result[1].id == 'tab1', 'expected tab1 id')\n\
             \n\
             local snap = fleet.ui.snapshot()\n\
             assert(snap.width == 1920, 'expected width')\n\
             \n\
             return 0"
        );

        let result = engine.eval(&script, &host).expect("fleet module eval");
        assert_eq!(result.value, 0);
    }

    #[test]
    fn eval_with_args_passes_arguments() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        let args = vec!["hello".to_string(), "world".to_string()];
        let result = engine
            .eval_with_args(
                "local n = __host.args_len() return n",
                &host,
                &args,
            )
            .expect("eval_with_args");
        assert_eq!(result.value, 2);
    }

    #[test]
    fn eval_with_args_arg_by_index() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        let args = vec!["first".to_string(), "second".to_string()];
        let result = engine
            .eval_with_args(
                "local a = __host.arg(0); local b = __host.arg(1); print(a .. ' ' .. b); return 0",
                &host,
                &args,
            )
            .expect("eval_with_args");
        assert_eq!(result.value, 0);
        assert!(result.stdout.contains("first second"), "stdout: {}", result.stdout);
    }

    #[test]
    fn fleet_auto_injected_available() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        let result = engine
            .eval("return fleet ~= nil and 1 or 0", &host)
            .expect("eval");
        assert_eq!(result.value, 1, "fleet module should be auto-injected");
    }

    #[test]
    fn fleet_auto_injected_call_no_bridge() {
        let engine = LuaEngine::new().expect("create engine");
        let host = LuaHostFunctions::default();
        // Without __host.fleet_call, calls should return nil gracefully
        let result = engine
            .eval("local r = fleet.tabs.list(); return r == nil and 1 or 0", &host)
            .expect("eval");
        assert_eq!(result.value, 1, "should return nil when no fleet bridge");
    }

    #[test]
    fn eval_build_identity_module() {
        use std::process::Command;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let repo = dir.path();

        // Set up a minimal git repo with required files
        Command::new("git")
            .args(["init", repo.to_str().unwrap()])
            .output()
            .expect("git init");
        // Configure git user for commit
        Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "config", "user.email", "test@example.com"])
            .output()
            .ok();
        Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "config", "user.name", "Test"])
            .output()
            .ok();
        std::fs::write(repo.join("Cargo.lock"), "# lock\n").expect("Cargo.lock");
        std::fs::create_dir_all(repo.join("scripts")).expect("scripts dir");
        std::fs::write(repo.join("scripts").join("artifacts.json"), "{}").expect("artifacts.json");
        Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .output()
            .expect("git add");
        Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "commit", "-m", "init"])
            .output()
            .expect("git commit");

        let engine = LuaEngine::new().expect("create engine");
        let output_path = repo.join("build-identity.bat");
        let repo_str = repo.to_str().unwrap().to_string();
        let output_str = output_path.to_str().unwrap().to_string();
        let host = LuaHostFunctions {
            args_len: Some(Arc::new(|| 3)),
            arg: Some(Arc::new(move |i: i64| {
                match i {
                    0 => Ok(repo_str.clone()),
                    1 => Ok("dev".to_string()),
                    2 => Ok(output_str.clone()),
                    _ => Err("out of range".to_string()),
                }
            })),
            ..Default::default()
        };

        // Load build_identity module, then entry point
        let lib_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().parent().unwrap()
            .join("scripts").join("lua").join("lib").join("build_identity.lua");
        let lib_source = std::fs::read_to_string(&lib_path).expect("read lib");
        // Remove `return build_identity` so the local stays in scope
        let lib_source = lib_source.trim_end()
            .strip_suffix("return build_identity")
            .unwrap_or(&lib_source)
            .trim_end()
            .to_string();

        let entry_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().parent().unwrap()
            .join("scripts").join("lua").join("build_identity.lua");
        let entry_source = std::fs::read_to_string(&entry_path).expect("read entry");

        let script = format!("{lib_source}\n{entry_source}");
        let result = engine.eval(&script, &host).expect("build_identity eval");
        assert_eq!(result.value, 0);

        // Verify output batch file exists and has expected content
        let batch = std::fs::read_to_string(repo.join("build-identity.bat")).expect("read batch");
        assert!(batch.contains("AGENTERM_BUILD_IDENTITY_VERSION=1"));
        assert!(batch.contains("AGENTERM_BUILD_GIT_COMMIT="));
        assert!(batch.contains("AGENTERM_BUILD_PROFILE=dev"));
    }
}
