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
pub use hosts::{
    ALL_CELLS, HostCell, LINUX_AARCH64, LINUX_X86_64, MACOS_AARCH64, MACOS_X86_64,
    PLATFORM_CANDIDATES, SecondaryProbe, SizeProbe, WINDOWS_AARCH64, WINDOWS_X86_64, cell,
    live_cell,
};
pub use sym::Symbol;
pub use value::Value;

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
        self.bindings
            .insert(name.to_owned(), Value::Ptr(ptr as usize));
        Ok(())
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
