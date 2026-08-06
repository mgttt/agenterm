pub mod check;
pub mod compile;
pub mod error;
pub mod load;
pub mod manifest;
pub mod pack;
pub mod qualify;
pub mod subset;
pub mod transpile;

pub use check::check;
pub use compile::{compile_native, compile_native_for_target, hash_bytes, hash_file, CompileOutput};
pub use error::RhError;
pub use load::{load_and_call_entry, verify_native_hash, RhNativeModule};
pub use manifest::RhPackManifest;
pub use pack::{build_pack_dir, PackBuildOutput, RhPack};
pub use qualify::{qualify_pack_dir, write_receipt, RhQualificationReceipt};
pub use transpile::{transpile, transpile_cdylib};

pub const RH_VERSION: &str = env!("CARGO_PKG_VERSION");
