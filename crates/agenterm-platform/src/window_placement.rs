//! Foreign-window placement inspection facade.
//!
//! This is preflight metadata, not product authorization. The OS adapter
//! validates the transient native handle against `expected_pid`; callers own
//! authorization, action policy, actuation, and final bounds readback.

use crate::CapabilityStatus;
pub use crate::contract::window_placement::{
    PlacementRole, PlacementWindowInfo, SizeConstraints, Support, WindowPlacementError, WindowSize,
};

pub fn capability_status() -> CapabilityStatus {
    crate::selected::window_placement::capability_status()
}

pub fn inspect(
    handle: isize,
    expected_pid: u32,
) -> Result<PlacementWindowInfo, WindowPlacementError> {
    crate::selected::window_placement::inspect(handle, expected_pid)?.validate()
}
