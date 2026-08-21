pub mod api_validate;
pub(crate) mod backend;
pub mod bundle;
pub mod caller_inventory;
pub mod check;
pub mod check_many;
#[cfg(feature = "compile")]
pub mod compile;
pub mod corpus;
pub mod engine;
pub mod error;
pub mod evidence;
pub mod expr_print;
pub mod fleet;
pub mod fleet_subset;
pub mod host;
pub mod host_api;
pub mod host_std;
pub(crate) mod interp;
pub(crate) mod ir;
pub mod lang_error;
#[cfg(feature = "compile")]
pub mod load;
pub(crate) mod lower;
#[cfg(feature = "compile")]
pub mod manifest;
#[cfg(feature = "compile")]
pub mod pack;
pub mod project_import;
#[cfg(feature = "compile")]
pub mod qualify;
pub mod shipped_surfaces;
pub mod subset;
pub mod transpile;
pub mod value;

pub use backend::{CancelHandle, Scope};
pub use bundle::bundle_project_source;
pub use caller_inventory::{
    CallerHit, CallerInventoryOptions, CallerInventoryReport, scan_caller_inventory,
};
pub use check::check;
pub use check::{RH_MAX_EXPR_DEPTH, check_with_project_validation};
pub use check_many::{
    CheckManyManifest, CheckManyOptions, CheckManyReport, ParsedCheckManyCli, parse_check_many_cli,
    read_manifest, run_check_many,
};
#[cfg(feature = "compile")]
pub use compile::{
    CompileOutput, compile_native, compile_native_for_target, hash_bytes, hash_file,
};
pub use corpus::{
    CorpusScanOptions, CorpusScanReport, extract_task_entries, scan_relative_files,
    scan_rh_directory, scan_task_manifest,
};
pub use engine::{Compiled, Engine, Options};
pub use error::RhError;
pub use evidence::static_evidence_declarations;
pub use host::{Host, NullHost, ProcessRequest};
pub use host_api::{
    RH_CODEGEN_REVISION, RH_HOST_API_ROOT, RH_HOST_API_VERSION, RH_HOST_FLEET_OUT_CAP,
    RH_HOST_FS_READ_CAP, RH_HOST_OUT_CAP, RH_HOST_UTILITY_EXISTS_CASE_EXACT, RH_HOST_UTILITY_FAIL,
    RH_HOST_UTILITY_PRINT, RH_HOST_UTILITY_PROCESS_STATUS, RH_HOST_UTILITY_PROCESS_STDOUT_FILE,
    RhHostArgCall, RhHostArgsLenCall, RhHostFleetCall, RhHostFsReadCall, RhHostJsonCall,
    RhHostStdFsExistsCall, RhHostUtilityCall, emit_host_runtime, host_api_module,
};
pub use host_std::StdHost;
pub use lang_error::Error;
#[cfg(feature = "compile")]
pub use load::{RhNativeModule, load_and_call_entry, verify_native_hash};
#[cfg(feature = "compile")]
pub use manifest::RhPackManifest;
#[cfg(feature = "compile")]
pub use pack::{PackBuildOutput, RhPack, build_pack_dir};
#[cfg(feature = "compile")]
pub use qualify::{RhQualificationReceipt, qualify_pack_dir, write_receipt};
pub use transpile::{
    CdylibExecutionMode, CdylibTranspileOutput, transpile, transpile_cdylib,
    transpile_cdylib_with_mode, transpile_cdylib_with_project,
};
pub use value::{HostObject, Value, exit_from_int};

pub const RH_VERSION: &str = env!("CARGO_PKG_VERSION");
