//! The execution seam — **crate-private on purpose** (design D10 / A8).
//!
//! If this trait were public, `IrModule` and `Limits` would become frozen
//! embedder surface and a later Cranelift / copy-and-patch backend could not
//! land without a breaking change. Keeping it `pub(crate)` means the public
//! freeze is only `Engine` + `Host` + `Value` + `Engine::compile → Unsupported`.
//!
//! There is deliberately **no** `RustcPackBackend` here: rustc pack,
//! `libloading`, and codegen 107 stay on the AgenTerm side (§8).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::host::Host;
use crate::ir::IrModule;
use crate::lang_error::Error;
use crate::value::Value;

/// A handle that can stop a running program from another thread.
#[derive(Clone, Debug, Default)]
pub struct CancelHandle(Arc<AtomicBool>);

impl CancelHandle {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Named variable bindings handed to a program.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    entries: Vec<(String, Value)>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: impl Into<String>, value: Value) -> &mut Self {
        let name = name.into();
        match self.entries.iter_mut().find(|(key, _)| *key == name) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((name, value)),
        }
        self
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(name, value)| (name, value))
    }
}

/// Runtime limits applied by a backend.
#[derive(Clone, Debug)]
pub(crate) struct Limits {
    pub(crate) fuel: Option<u64>,
    pub(crate) cancel: CancelHandle,
}

pub(crate) trait Backend: Send {
    fn eval(
        &mut self,
        ir: &IrModule,
        scope: &mut Scope,
        host: &mut dyn Host,
        limits: &Limits,
    ) -> Result<Value, Error>;

    fn compile(&mut self, ir: &IrModule) -> Result<(), Error> {
        let _ = ir;
        Err(Error::unsupported("compile"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Backend, CancelHandle, Limits, Scope};

    #[test]
    fn backend_and_scope_are_send() {
        fn assert_send<T: Send + ?Sized>() {}
        assert_send::<dyn Backend>();
        assert_send::<Scope>();
        assert_send::<CancelHandle>();
    }

    #[test]
    fn cancel_handle_is_shared() {
        let handle = CancelHandle::default();
        let clone = handle.clone();
        assert!(!handle.is_cancelled());
        clone.cancel();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn scope_set_overwrites_by_name() {
        let mut scope = Scope::new();
        scope.set("x", crate::value::Value::Int(1));
        scope.set("x", crate::value::Value::Int(2));
        assert_eq!(scope.get("x"), Some(&crate::value::Value::Int(2)));
    }

    #[test]
    fn limits_are_constructible_crate_side() {
        let _ = Limits {
            fuel: Some(8),
            cancel: CancelHandle::default(),
        };
    }
}
