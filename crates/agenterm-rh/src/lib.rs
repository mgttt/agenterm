pub mod caller_inventory;
pub mod check;
pub mod check_many;
pub mod compile;
pub mod corpus;
pub mod error;
pub mod expr_print;
pub mod fleet;
pub mod host_api;
pub mod load;
pub mod manifest;
pub mod pack;
pub mod qualify;
pub mod subset;
pub mod transpile;

pub use caller_inventory::{
    scan_caller_inventory, CallerHit, CallerInventoryOptions, CallerInventoryReport,
};
pub use check::check;
pub use check_many::{
    parse_check_many_cli, read_manifest, run_check_many, CheckManyManifest, CheckManyOptions,
    CheckManyReport, ParsedCheckManyCli,
};
pub use corpus::{
    extract_task_entries, scan_rhai_directory, scan_relative_files, scan_task_manifest,
    CorpusScanOptions, CorpusScanReport,
};
pub use compile::{
    CompileOutput, compile_native, compile_native_for_target, hash_bytes, hash_file,
};
pub use error::RhError;
pub use host_api::{
    RH_HOST_API_VERSION, RH_HOST_FLEET_OUT_CAP, RH_HOST_OUT_CAP, RhHostEvalCall, RhHostFleetCall,
    emit_host_runtime,
};
pub use load::{RhNativeModule, load_and_call_entry, verify_native_hash};
pub use manifest::RhPackManifest;
pub use pack::{PackBuildOutput, RhPack, build_pack_dir};
pub use qualify::{RhQualificationReceipt, qualify_pack_dir, write_receipt};
pub use transpile::{transpile, transpile_cdylib};

pub const RH_VERSION: &str = env!("CARGO_PKG_VERSION");
