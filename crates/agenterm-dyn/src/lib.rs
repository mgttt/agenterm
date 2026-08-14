use std::collections::HashMap;
use std::ffi::c_void;

mod error;
mod eval;
mod native;
mod parse;
mod sym;
mod value;

pub use error::DynError;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_stable() {
        let mut env = Dyn::new();
        let a = env.intern("ioctl");
        let b = env.intern("ioctl");
        let c = env.intern("getpid");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn set_and_lookup() {
        let mut env = Dyn::new();
        let v = env.eval("(do (set x 42) x)").expect("set/get should work");
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn if_form() {
        let mut env = Dyn::new();
        let t = env.eval("(if 1 7 9)").expect("true branch");
        let f = env.eval("(if 0 7 9)").expect("false branch");
        assert_eq!(t, Value::Int(7));
        assert_eq!(f, Value::Int(9));
    }
}
