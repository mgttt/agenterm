//! Linux native platform adapter (`prd/PRD_02_20_native_platform.md`).
//!
//! Ownership: Linux agent only. Do not edit shared contracts here.
//!
//! Wiring: this module is intentionally **not** declared until primary freezes
//! `src/platform/mod.rs` and adds `pub mod linux`. Orphan files under
//! `src/platform/linux/` must not invent shared event/capability types.
//!
//! Migration slice 1 (step 2 after contract freeze):
//! - toolbar labels + stable product action identities
//! - keyboard committed text vs shortcut chord separation
//!
//! Public capability identity stays on the Linux branch. Private reuse of
//! `crate::unix_app` helpers is allowed after wiring without merging macOS
//! and Linux public identities.
//!
//! Contract revision implemented by this scaffold: **none yet** (awaiting
//! primary freeze of `src/platform/mod.rs`).

#![cfg(target_os = "linux")]

pub(crate) mod input;
pub(crate) mod toolbar;

/// Linux display backend facts (capability discovery metadata only — not auth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinuxDisplayFacts {
    pub x11: bool,
    pub wayland: bool,
    pub headless: bool,
}

impl LinuxDisplayFacts {
    /// Best-effort probe from common environment variables.
    ///
    /// A headless result must surface as a typed failure once the shared
    /// screenshot/window capabilities exist; do not claim availability.
    pub(crate) fn from_env() -> Self {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11 = std::env::var_os("DISPLAY").is_some();
        Self {
            x11,
            wayland,
            headless: !(x11 || wayland),
        }
    }
}
