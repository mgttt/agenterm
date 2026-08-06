use std::path::Path;

use libloading::{Library, Symbol};

use crate::{compile::hash_file, RhError};

pub struct RhNativeModule {
    library: Library,
}

impl RhNativeModule {
    pub fn load(path: &Path) -> Result<Self, RhError> {
        let library =
            unsafe { Library::new(path) }.map_err(|err| RhError::Compile(err.to_string()))?;
        Ok(Self { library })
    }

    pub fn call_entry(&self) -> i64 {
        let entry: Symbol<extern "C" fn() -> i64> = unsafe {
            self.library
                .get(b"rh_entry")
                .expect("rh_entry symbol missing from native pack")
        };
        entry()
    }
}

pub fn load_and_call_entry(path: &Path) -> Result<i64, RhError> {
    RhNativeModule::load(path).map(|module| module.call_entry())
}

pub fn verify_native_hash(path: &Path, expected: &str) -> Result<(), RhError> {
    let actual = hash_file(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(RhError::Compile(format!(
            "native_hash mismatch: expected {expected}, got {actual}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::load_and_call_entry;
    use crate::compile::compile_native;

    #[test]
    fn dlopen_calls_rh_entry() {
        let out = std::env::temp_dir().join(format!(
            "agenterm-rh-load-{}.{}",
            std::process::id(),
            crate::compile::native_extension()
        ));
        let _ = std::fs::remove_file(&out);
        compile_native("fn entry() { 42 }", &out).expect("compile");

        let value = load_and_call_entry(&out).expect("load");
        assert_eq!(value, 42);
    }
}
