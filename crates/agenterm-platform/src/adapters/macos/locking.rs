#[path = "../unix/locking.rs"]
mod unix;

pub use unix::{PathLock, SlotPermit};
