use std::collections::HashMap;
use std::ffi::c_void;

mod error;
mod eval;
mod hosts;
mod native;
mod parse;
mod sym;
mod value;

pub use error::DynError;
pub use eval::REPEAT_MAX;
pub use hosts::{
    ALL_CELLS, CU_ADJACENT_PROBE_CATALOG, CuAdjacentProbeCell, HostArch, HostCell, HostOs,
    LAYER3_CANDIDATES, LINUX_AARCH64, LINUX_ATSPI_EXISTENCE_LIBS, LINUX_X86_64, MACOS_AARCH64,
    MACOS_X86_64, PLATFORM_CANDIDATES, ProbeFact, SecondaryProbe, SizeProbe, SystemProbe,
    SystemProbeStatus, WINDOWS_AARCH64, WINDOWS_X86_64, cell, cu_adjacent_probe, live_cell,
};
pub use sym::Symbol;
pub use value::Value;

/// Maximum number of distinct bindings retained by one [`Dyn`] environment.
pub const MAX_BINDINGS: usize = 4_096;

/// In-process live-native evaluation environment.
pub struct Dyn {
    interner: sym::Interner,
    pub(crate) bindings: HashMap<String, Value>,
    pub(crate) libs: native::LibraryCache,
}

impl Dyn {
    pub fn new() -> Self {
        Self {
            interner: sym::Interner::new(),
            bindings: HashMap::new(),
            libs: native::LibraryCache::new(),
        }
    }

    /// Intern `name` into a stable [`Symbol`].
    pub fn intern(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    /// Bind `name` to an existing native pointer/handle (for example a `winsize` buffer).
    pub fn bind(&mut self, name: &str, ptr: *mut c_void) -> Result<(), DynError> {
        if name.is_empty() {
            return Err(DynError::InvalidBindingName);
        }
        self.ensure_binding_capacity(name)?;
        self.bindings
            .insert(name.to_owned(), Value::Ptr(ptr as usize));
        Ok(())
    }

    /// Reject a new binding when the environment is full, while always permitting replacement.
    pub(crate) fn ensure_binding_capacity(&self, name: &str) -> Result<(), DynError> {
        if self.bindings.contains_key(name) || self.bindings.len() < MAX_BINDINGS {
            return Ok(());
        }
        Err(DynError::StateLimit {
            resource: "bindings",
            limit: MAX_BINDINGS,
        })
    }

    /// Evaluate S-expression `source` in this environment.
    pub fn eval(&mut self, source: &str) -> Result<Value, DynError> {
        let expr = parse::parse(source)?;
        eval::eval_expr(self, &expr)
    }
}

impl Default for Dyn {
    fn default() -> Self {
        Self::new()
    }
}
