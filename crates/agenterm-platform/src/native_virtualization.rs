//! Current-host native virtualization availability without VM lifecycle policy.

pub use crate::contract::native_virtualization::{
    NativeVirtualizationBackend, NativeVirtualizationFacts, VirtualizationProbeState,
};

#[must_use]
pub fn probe() -> NativeVirtualizationFacts {
    crate::selected::native_virtualization::probe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_host_probe_uses_the_selected_backend() {
        assert_eq!(
            crate::capability_status(crate::Capability::NativeVirtualization),
            crate::CapabilityStatus::Available
        );
        let facts = probe();
        eprintln!("native virtualization probe: {facts:?}");
        #[cfg(windows)]
        assert_eq!(
            facts.backend(),
            NativeVirtualizationBackend::WindowsHypervisorPlatform
        );
        #[cfg(target_os = "linux")]
        assert_eq!(facts.backend(), NativeVirtualizationBackend::Kvm);
        #[cfg(target_os = "macos")]
        assert_eq!(
            facts.backend(),
            NativeVirtualizationBackend::HypervisorFramework
        );
        assert!(!facts.state().as_str().is_empty());
        if facts.is_available() {
            assert_eq!(facts.state(), VirtualizationProbeState::Available);
        }
    }
}
