//! Product-neutral facts about the current host's native virtualization ABI.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum NativeVirtualizationBackend {
    WindowsHypervisorPlatform,
    Kvm,
    HypervisorFramework,
}

impl NativeVirtualizationBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsHypervisorPlatform => "whpx",
            Self::Kvm => "kvm",
            Self::HypervisorFramework => "hvf",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum VirtualizationProbeState {
    Available,
    Unavailable,
    AccessDenied,
    Incompatible,
    Failed,
}

impl VirtualizationProbeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::AccessDenied => "access-denied",
            Self::Incompatible => "incompatible",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeVirtualizationFacts {
    backend: NativeVirtualizationBackend,
    state: VirtualizationProbeState,
    api_version: Option<u32>,
    native_code: Option<i64>,
}

impl NativeVirtualizationFacts {
    #[must_use]
    pub const fn available(backend: NativeVirtualizationBackend, api_version: Option<u32>) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::Available,
            api_version,
            native_code: None,
        }
    }

    #[must_use]
    pub const fn unavailable(backend: NativeVirtualizationBackend) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::Unavailable,
            api_version: None,
            native_code: None,
        }
    }

    #[must_use]
    pub const fn unavailable_with_code(
        backend: NativeVirtualizationBackend,
        native_code: i64,
    ) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::Unavailable,
            api_version: None,
            native_code: Some(native_code),
        }
    }

    #[must_use]
    pub const fn access_denied(backend: NativeVirtualizationBackend, native_code: i64) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::AccessDenied,
            api_version: None,
            native_code: Some(native_code),
        }
    }

    #[must_use]
    pub const fn incompatible(backend: NativeVirtualizationBackend, api_version: u32) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::Incompatible,
            api_version: Some(api_version),
            native_code: None,
        }
    }

    #[must_use]
    pub const fn failed(backend: NativeVirtualizationBackend, native_code: i64) -> Self {
        Self {
            backend,
            state: VirtualizationProbeState::Failed,
            api_version: None,
            native_code: Some(native_code),
        }
    }

    #[must_use]
    pub const fn backend(self) -> NativeVirtualizationBackend {
        self.backend
    }

    #[must_use]
    pub const fn state(self) -> VirtualizationProbeState {
        self.state
    }

    #[must_use]
    pub const fn api_version(self) -> Option<u32> {
        self.api_version
    }

    #[must_use]
    pub const fn native_code(self) -> Option<i64> {
        self.native_code
    }

    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self.state, VirtualizationProbeState::Available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_preserve_state_specific_evidence() {
        let backend = NativeVirtualizationBackend::Kvm;
        let available = NativeVirtualizationFacts::available(backend, Some(12));
        assert!(available.is_available());
        assert_eq!(available.api_version(), Some(12));
        assert_eq!(available.native_code(), None);

        let denied = NativeVirtualizationFacts::access_denied(backend, 13);
        assert_eq!(denied.state(), VirtualizationProbeState::AccessDenied);
        assert_eq!(denied.api_version(), None);
        assert_eq!(denied.native_code(), Some(13));

        let missing = NativeVirtualizationFacts::unavailable_with_code(backend, 2);
        assert_eq!(missing.state(), VirtualizationProbeState::Unavailable);
        assert_eq!(missing.native_code(), Some(2));

        let incompatible = NativeVirtualizationFacts::incompatible(backend, 11);
        assert_eq!(incompatible.state(), VirtualizationProbeState::Incompatible);
        assert_eq!(incompatible.api_version(), Some(11));
        assert_eq!(incompatible.native_code(), None);
    }

    #[test]
    fn stable_names_cover_every_declared_backend_and_state() {
        assert_eq!(
            NativeVirtualizationBackend::WindowsHypervisorPlatform.as_str(),
            "whpx"
        );
        assert_eq!(NativeVirtualizationBackend::Kvm.as_str(), "kvm");
        assert_eq!(
            NativeVirtualizationBackend::HypervisorFramework.as_str(),
            "hvf"
        );
        assert_eq!(VirtualizationProbeState::Available.as_str(), "available");
        assert_eq!(
            VirtualizationProbeState::AccessDenied.as_str(),
            "access-denied"
        );
        assert_eq!(
            VirtualizationProbeState::Incompatible.as_str(),
            "incompatible"
        );
    }
}
