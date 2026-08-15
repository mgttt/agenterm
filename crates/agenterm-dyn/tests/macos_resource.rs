//! Darwin resource-ownership catalogue guards.
//!
//! `mach_host_self` creates a Mach send right.  The generic `dlcall` door has
//! no ownership-aware release operation, so the catalogue must retain the
//! honest placeholder instead of treating allocation as a harmless probe.

#![cfg(target_os = "macos")]

use agenterm_dyn::{SystemProbeStatus, live_cell};

#[test]
fn mach_host_self_is_catalogued_but_never_callable_without_a_release_owner() {
    let probe = live_cell()
        .expect("macOS host cell")
        .system_probes
        .iter()
        .find(|probe| probe.name == "mach_host_self")
        .expect("Mach host probe is catalogued");

    assert!(matches!(probe.status, SystemProbeStatus::Placeholder));
}
