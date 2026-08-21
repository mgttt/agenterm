//! The Language-1 `Host` trait (`plan/design-rh-standalone-product.md` D13).
//!
//! **Every method has a default that fails closed.** That is the whole point:
//! Rust cannot "omit" a required method, so a sandbox embedder that wants no
//! capabilities implements nothing and gets `Error::Unsupported` for all of
//! them. This mirrors `LuaHostFunctions`' `Option<Arc<dyn Fn…>>` precedent
//! without growing the trait every time AgenTerm adds a `fleet.*` name.

use crate::lang_error::Error;
use crate::value::Value;

/// A host that a Language-1 program can call out to.
///
/// `Send` is required so `Engine` can be `Send` (D17): there is no
/// thread-local host in the product crate.
pub trait Host: Send {
    /// `print(...)`.
    fn print(&mut self, text: &str) -> Result<(), Error> {
        let _ = text;
        Err(Error::unsupported("print"))
    }

    /// `args.len`.
    fn args_len(&self) -> Result<i64, Error> {
        Err(Error::unsupported("args.len"))
    }

    /// `args[index]`.
    fn arg(&self, index: u32) -> Result<String, Error> {
        let _ = index;
        Err(Error::unsupported("args"))
    }

    /// Every other Language-1 surface, keyed by its **script spelling**
    /// (`std::fs::exists`, `rh::json::parse`, `Command.start`). AgenTerm
    /// additionally routes its dot-form fleet names (`fleet.tabs.list`) here.
    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, Error> {
        let _ = args;
        Err(Error::unsupported_name(name))
    }
}

/// A process spawn request, with the caps the live host enforces.
///
/// Caps are from `src/script_rh_host.rs` `host_process_request` (lines
/// 300-386), not folklore.
#[derive(Clone, Debug, Default)]
pub struct ProcessRequest {
    pub program: String,
    /// Cap 256 entries; each cap 4096 bytes.
    pub args: Vec<String>,
    pub timeout_ms: u64,
    /// Cap 4096 bytes.
    pub stdout_path: Option<String>,
    /// Cap 4096 bytes.
    pub current_dir: Option<String>,
    /// Cap 256 entries; name cap 256 bytes, value cap 4096 bytes.
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub env_clear: bool,
}

/// The caps `StdHost` applies to a [`ProcessRequest`].
pub mod caps {
    pub const MAX_ARGS: usize = 256;
    pub const MAX_ARG_BYTES: usize = 4096;
    pub const MAX_ENV: usize = 256;
    pub const MAX_ENV_NAME_BYTES: usize = 256;
    pub const MAX_ENV_VALUE_BYTES: usize = 4096;
    pub const MAX_PATH_BYTES: usize = 4096;
}

impl ProcessRequest {
    /// Enforce the live caps. Returns the same `process_*_too_*` shapes the
    /// AOT host reports, so a script sees one vocabulary either way.
    pub fn validate(&self) -> Result<(), Error> {
        if self.args.len() > caps::MAX_ARGS {
            return Err(Error::Host(format!(
                "process_arguments_too_many: maximum is {}",
                caps::MAX_ARGS
            )));
        }
        for arg in &self.args {
            if arg.len() > caps::MAX_ARG_BYTES {
                return Err(Error::Host(format!(
                    "process_argument_too_large: maximum is {} bytes",
                    caps::MAX_ARG_BYTES
                )));
            }
        }
        if self.env.len() > caps::MAX_ENV {
            return Err(Error::Host(format!(
                "process_env_too_many: maximum is {}",
                caps::MAX_ENV
            )));
        }
        for (name, value) in &self.env {
            if name.len() > caps::MAX_ENV_NAME_BYTES {
                return Err(Error::Host(format!(
                    "process_env_name_too_large: maximum is {} bytes",
                    caps::MAX_ENV_NAME_BYTES
                )));
            }
            if value.len() > caps::MAX_ENV_VALUE_BYTES {
                return Err(Error::Host(format!(
                    "process_env_value_too_large: maximum is {} bytes",
                    caps::MAX_ENV_VALUE_BYTES
                )));
            }
        }
        for path in [&self.stdout_path, &self.current_dir].into_iter().flatten() {
            if path.len() > caps::MAX_PATH_BYTES {
                return Err(Error::Host(format!(
                    "process_path_too_large: maximum is {} bytes",
                    caps::MAX_PATH_BYTES
                )));
            }
        }
        Ok(())
    }
}

/// A host that provides nothing. Every call fails closed.
///
/// This is the default host for `Engine::new()` in PR-A1; `StdHost` arrives in
/// PR-A3 and is what actually implements the Language-1 allowlist.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullHost;

impl Host for NullHost {}

#[cfg(test)]
mod tests {
    use super::{Host, NullHost};
    use crate::lang_error::Error;

    #[test]
    fn null_host_fails_closed_on_every_method() {
        let mut host = NullHost;
        assert!(matches!(
            host.print("x"),
            Err(Error::Unsupported { feature }) if feature == "print"
        ));
        assert!(matches!(host.args_len(), Err(Error::Unsupported { .. })));
        assert!(matches!(host.arg(0), Err(Error::Unsupported { .. })));
        assert!(matches!(
            host.call("std::fs::exists", &[]),
            Err(Error::Unsupported { .. })
        ));
    }

    /// A host implementing only `print` still compiles: the other three
    /// capabilities are genuinely omittable, which is what D13 promises.
    #[test]
    fn a_host_may_implement_only_what_it_offers() {
        struct PrintOnly(Vec<String>);
        impl Host for PrintOnly {
            fn print(&mut self, text: &str) -> Result<(), Error> {
                self.0.push(text.to_owned());
                Ok(())
            }
        }
        let mut host = PrintOnly(Vec::new());
        host.print("hello").expect("print");
        assert_eq!(host.0, vec!["hello".to_owned()]);
        assert!(matches!(host.args_len(), Err(Error::Unsupported { .. })));
    }

    #[test]
    fn host_is_object_safe_and_send() {
        fn assert_send<T: Send + ?Sized>() {}
        assert_send::<dyn Host>();
        let _boxed: Box<dyn Host> = Box::new(NullHost);
    }
}
