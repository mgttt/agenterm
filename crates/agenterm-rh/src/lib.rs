pub mod check;
pub mod error;
pub mod subset;
pub mod transpile;

pub use check::{check, compile_native};
pub use error::RhError;
pub use transpile::transpile;

pub const RH_VERSION: &str = env!("CARGO_PKG_VERSION");
