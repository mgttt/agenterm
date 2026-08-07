use std::path::Path;

use libloading::{Library, Symbol};

use crate::{
    RhError,
    compile::hash_file,
    host_api::{
        RhHostArgCall, RhHostArgsLenCall, RhHostEvalCall, RhHostFleetCall, RhHostRunScriptCall,
        RhHostStdFsExistsCall,
    },
};

pub struct RhNativeModule {
    library: Library,
}

impl RhNativeModule {
    pub fn load(path: &Path) -> Result<Self, RhError> {
        let library =
            unsafe { Library::new(path) }.map_err(|err| RhError::Compile(err.to_string()))?;
        Ok(Self { library })
    }

    pub fn register_host(&self, fleet_call: RhHostFleetCall) -> Result<(), RhError> {
        self.register_host_v2(fleet_call, None)
    }

    pub fn register_host_v2(
        &self,
        fleet_call: RhHostFleetCall,
        eval_call: Option<RhHostEvalCall>,
    ) -> Result<(), RhError> {
        self.register_host_v6(fleet_call, eval_call, None, None, None, None)
    }

    pub fn register_host_v3(
        &self,
        fleet_call: RhHostFleetCall,
        eval_call: Option<RhHostEvalCall>,
        run_script_call: Option<RhHostRunScriptCall>,
    ) -> Result<(), RhError> {
        self.register_host_v6(fleet_call, eval_call, run_script_call, None, None, None)
    }

    pub fn register_host_v4(
        &self,
        fleet_call: RhHostFleetCall,
        eval_call: Option<RhHostEvalCall>,
        run_script_call: Option<RhHostRunScriptCall>,
        std_fs_exists_call: Option<RhHostStdFsExistsCall>,
    ) -> Result<(), RhError> {
        self.register_host_v6(
            fleet_call,
            eval_call,
            run_script_call,
            std_fs_exists_call,
            None,
            None,
        )
    }

    pub fn register_host_v5(
        &self,
        fleet_call: RhHostFleetCall,
        eval_call: Option<RhHostEvalCall>,
        run_script_call: Option<RhHostRunScriptCall>,
        std_fs_exists_call: Option<RhHostStdFsExistsCall>,
        args_len_call: Option<RhHostArgsLenCall>,
    ) -> Result<(), RhError> {
        self.register_host_v6(
            fleet_call,
            eval_call,
            run_script_call,
            std_fs_exists_call,
            args_len_call,
            None,
        )
    }

    pub fn register_host_v6(
        &self,
        fleet_call: RhHostFleetCall,
        eval_call: Option<RhHostEvalCall>,
        run_script_call: Option<RhHostRunScriptCall>,
        std_fs_exists_call: Option<RhHostStdFsExistsCall>,
        args_len_call: Option<RhHostArgsLenCall>,
        arg_call: Option<RhHostArgCall>,
    ) -> Result<(), RhError> {
        unsafe {
            if let Ok(register_v6) = self.library.get::<Symbol<
                extern "C" fn(
                    RhHostFleetCall,
                    RhHostEvalCall,
                    RhHostRunScriptCall,
                    RhHostStdFsExistsCall,
                    RhHostArgsLenCall,
                    RhHostArgCall,
                ),
            >>(b"rh_register_host_v6")
            {
                register_v6(
                    fleet_call,
                    eval_call.unwrap_or(dummy_eval_call),
                    run_script_call.unwrap_or(dummy_run_script_call),
                    std_fs_exists_call.unwrap_or(dummy_std_fs_exists_call),
                    args_len_call.unwrap_or(dummy_args_len_call),
                    arg_call.unwrap_or(dummy_arg_call),
                );
                return Ok(());
            }
            if let Ok(register_v5) = self.library.get::<Symbol<
                extern "C" fn(
                    RhHostFleetCall,
                    RhHostEvalCall,
                    RhHostRunScriptCall,
                    RhHostStdFsExistsCall,
                    RhHostArgsLenCall,
                ),
            >>(b"rh_register_host_v5")
            {
                register_v5(
                    fleet_call,
                    eval_call.unwrap_or(dummy_eval_call),
                    run_script_call.unwrap_or(dummy_run_script_call),
                    std_fs_exists_call.unwrap_or(dummy_std_fs_exists_call),
                    args_len_call.unwrap_or(dummy_args_len_call),
                );
                return Ok(());
            }
            if let Ok(register_v4) = self.library.get::<Symbol<
                extern "C" fn(
                    RhHostFleetCall,
                    RhHostEvalCall,
                    RhHostRunScriptCall,
                    RhHostStdFsExistsCall,
                ),
            >>(b"rh_register_host_v4")
            {
                register_v4(
                    fleet_call,
                    eval_call.unwrap_or(dummy_eval_call),
                    run_script_call.unwrap_or(dummy_run_script_call),
                    std_fs_exists_call.unwrap_or(dummy_std_fs_exists_call),
                );
                return Ok(());
            }
            if let Ok(register_v3) = self.library.get::<Symbol<
                extern "C" fn(RhHostFleetCall, RhHostEvalCall, RhHostRunScriptCall),
            >>(b"rh_register_host_v3")
            {
                register_v3(
                    fleet_call,
                    eval_call.unwrap_or(dummy_eval_call),
                    run_script_call.unwrap_or(dummy_run_script_call),
                );
                return Ok(());
            }
            if let Ok(register_v2) = self.library.get::<Symbol<
                extern "C" fn(RhHostFleetCall, RhHostEvalCall),
            >>(b"rh_register_host_v2")
            {
                register_v2(fleet_call, eval_call.unwrap_or(dummy_eval_call));
                return Ok(());
            }
            let register = self
                .library
                .get::<Symbol<extern "C" fn(RhHostFleetCall)>>(b"rh_register_host")
                .map_err(|err| RhError::Compile(err.to_string()))?;
            register(fleet_call);
        }
        Ok(())
    }

    pub fn host_api_version(&self) -> u32 {
        unsafe {
            self.library
                .get::<extern "C" fn() -> u32>(b"rh_host_api_version")
                .map(|version| version())
                .unwrap_or(0)
        }
    }

    pub fn call_entry(&self) -> i64 {
        unsafe {
            self.library
                .get::<extern "C" fn() -> i64>(b"rh_entry")
                .map(|entry| entry())
                .unwrap_or(-1)
        }
    }

    pub fn api_version(&self) -> u32 {
        unsafe {
            self.library
                .get::<extern "C" fn() -> u32>(b"rh_pack_api_version")
                .map(|version| version())
                .unwrap_or(0)
        }
    }

    pub fn cc_lines(&self) -> Vec<String> {
        unsafe {
            let Ok(count_fn) = self
                .library
                .get::<extern "C" fn() -> u32>(b"rh_cc_line_count")
            else {
                return Vec::new();
            };
            let count = count_fn();
            let Ok(len_fn) = self
                .library
                .get::<extern "C" fn(u32) -> u32>(b"rh_cc_line_len")
            else {
                return Vec::new();
            };
            let Ok(ptr_fn) = self
                .library
                .get::<extern "C" fn(u32) -> *const u8>(b"rh_cc_line_ptr")
            else {
                return Vec::new();
            };

            let mut lines = Vec::new();
            for index in 0..count {
                let length = len_fn(index) as usize;
                if length == 0 {
                    continue;
                }
                let pointer = ptr_fn(index);
                if pointer.is_null() {
                    continue;
                }
                let slice = std::slice::from_raw_parts(pointer, length);
                if let Ok(text) = std::str::from_utf8(slice) {
                    lines.push(text.to_owned());
                }
            }
            lines
        }
    }
}

extern "C" fn dummy_eval_call(
    _snippet: *const u8,
    _snippet_len: u32,
    _scope_json: *const u8,
    _scope_json_len: u32,
    _out_buf: *mut u8,
    _out_cap: u32,
) -> i32 {
    -4
}

extern "C" fn dummy_run_script_call(
    _source: *const u8,
    _source_len: u32,
    _out_buf: *mut u8,
    _out_cap: u32,
) -> i32 {
    -4
}

extern "C" fn dummy_std_fs_exists_call(_path: *const u8, _path_len: u32) -> i32 {
    -4
}

extern "C" fn dummy_args_len_call() -> i64 {
    -4
}

extern "C" fn dummy_arg_call(_index: u32, _out_buf: *mut u8, _out_cap: u32) -> i32 {
    -4
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

    #[test]
    fn dlopen_reads_cc_lines() {
        let dir = std::env::temp_dir().join(format!("agenterm-rh-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let output = crate::pack::build_pack_dir(
            "fn entry() { 1 }\nfn cc_lines() { [\"alpha\", \"beta\"] }",
            &dir,
        )
        .expect("build");
        let pack = crate::RhPack::load(&dir).expect("load");
        assert_eq!(pack.entry_value(), 1);
        assert_eq!(pack.cc_lines(), vec!["alpha".to_owned(), "beta".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = output;
    }
}
