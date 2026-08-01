//! Reusable operating-system contracts and capability facades.
//!
//! Product policy stays in the embedding crate. This crate exposes only
//! platform-neutral types and selected native mechanisms.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
}

pub const fn platform_kind() -> PlatformKind {
    selected::platform_kind()
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Capability {
    Hardware,
    CacheHierarchy,
    NativeVirtualization,
    ProcessorTopology,
    ProcessorAffinity,
    HostMemory,
    Storage,
    Entropy,
    ConsoleInterrupt,
    UserIdentity,
    ProcessControl,
    ProcessObservation,
    ProcessReference,
    ProcessSecurity,
    ProcessImage,
    ProcessMetrics,
    ProcessSpawn,
    SharedMemory,
    Process,
    FilesystemConventions,
    FilesystemEntry,
    FilesystemOpen,
    FilesystemCleanup,
    FilesystemPublish,
    FilesystemUsage,
    FileIdentity,
    Filesystem,
    Locking,
    Ipc,
    Pty,
    Window,
    Input,
    Ime,
    Activation,
    Clipboard,
    Screenshot,
    Font,
    WebView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CapabilityStatus {
    Available,
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

pub fn capability_status(capability: Capability) -> CapabilityStatus {
    let (enabled, implemented) = match capability {
        Capability::Hardware => (cfg!(feature = "hardware"), true),
        Capability::CacheHierarchy => (cfg!(feature = "cache-hierarchy"), true),
        Capability::NativeVirtualization => (cfg!(feature = "virtualization-probe"), true),
        Capability::ProcessorTopology => (cfg!(feature = "processor-topology"), true),
        Capability::ProcessorAffinity => (cfg!(feature = "processor-affinity"), true),
        Capability::HostMemory => (cfg!(feature = "host-memory"), true),
        Capability::Storage => (cfg!(feature = "storage"), true),
        Capability::Entropy => (cfg!(feature = "entropy"), true),
        Capability::ConsoleInterrupt => (cfg!(feature = "console-interrupt"), true),
        Capability::UserIdentity => (cfg!(feature = "user-identity"), true),
        Capability::ProcessControl => (cfg!(feature = "process-control"), true),
        Capability::ProcessObservation => (cfg!(feature = "process-observation"), true),
        Capability::ProcessReference => (cfg!(feature = "process-reference"), true),
        Capability::ProcessSecurity => (cfg!(feature = "process-security"), true),
        Capability::ProcessImage => (cfg!(feature = "process-image"), true),
        Capability::ProcessMetrics => (cfg!(feature = "process-metrics"), true),
        Capability::ProcessSpawn => (cfg!(feature = "process-spawn"), true),
        Capability::SharedMemory => (cfg!(feature = "shared-memory"), true),
        Capability::Process => (cfg!(feature = "process"), true),
        Capability::FilesystemConventions => (cfg!(feature = "filesystem-conventions"), true),
        Capability::FilesystemEntry => (cfg!(feature = "filesystem-entry"), true),
        Capability::FilesystemOpen => (cfg!(feature = "filesystem-open"), true),
        Capability::FilesystemCleanup => (cfg!(feature = "filesystem-cleanup"), true),
        Capability::FilesystemPublish => (cfg!(feature = "filesystem-publish"), true),
        Capability::FilesystemUsage => (cfg!(feature = "filesystem-usage"), true),
        Capability::FileIdentity => (cfg!(feature = "file-identity"), true),
        Capability::Filesystem => (cfg!(feature = "filesystem"), true),
        Capability::Locking => (cfg!(feature = "locking"), true),
        Capability::Ipc => (cfg!(feature = "ipc"), true),
        Capability::Pty => (cfg!(feature = "pty"), true),
        Capability::Window => (cfg!(feature = "window"), true),
        Capability::Input => (cfg!(feature = "input"), true),
        Capability::Ime => (cfg!(feature = "ime"), true),
        Capability::Activation => (cfg!(feature = "activation"), true),
        Capability::Clipboard => (cfg!(feature = "clipboard"), true),
        Capability::Screenshot => (cfg!(feature = "screenshot"), true),
        Capability::Font => (cfg!(feature = "font"), true),
        Capability::WebView => (cfg!(feature = "webview"), true),
    };
    if enabled && implemented {
        CapabilityStatus::Available
    } else if enabled {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("capability-not-yet-implemented"),
        }
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("feature-disabled"),
        }
    }
}

pub mod contract;

#[cfg(feature = "activation")]
pub mod activation;

#[cfg(feature = "clipboard")]
pub mod clipboard;

#[cfg(feature = "console-interrupt")]
pub mod console_interrupt;

#[cfg(all(feature = "window", feature = "input"))]
pub mod control_window;

#[cfg(feature = "font")]
pub mod font;

#[cfg(feature = "hardware")]
pub mod hardware;

#[cfg(feature = "cache-hierarchy")]
pub mod cache_hierarchy;

#[cfg(feature = "virtualization-probe")]
pub mod native_virtualization;

#[cfg(feature = "processor-topology")]
pub mod processor_topology;

#[cfg(feature = "processor-affinity")]
pub mod processor_affinity;

#[cfg(feature = "host-memory")]
pub mod host_memory;

#[cfg(feature = "storage")]
pub mod storage;

#[cfg(feature = "entropy")]
pub mod entropy;

#[cfg(feature = "user-identity")]
pub mod user_identity;

#[cfg(any(feature = "filesystem-conventions", feature = "filesystem"))]
pub mod filesystem;

#[cfg(feature = "filesystem-entry")]
pub mod filesystem_entry;

#[cfg(feature = "filesystem-open")]
pub mod filesystem_open;

#[cfg(feature = "filesystem-cleanup")]
pub mod filesystem_cleanup;

#[cfg(feature = "filesystem-publish")]
pub mod filesystem_publish;

#[cfg(feature = "filesystem-usage")]
pub mod filesystem_usage;

#[cfg(feature = "file-identity")]
pub mod file_identity;

#[cfg(feature = "locking")]
pub mod locking;

#[cfg(feature = "ipc")]
pub mod ipc;

#[cfg(feature = "input")]
pub mod input;

#[cfg(feature = "ime")]
pub mod ime;

#[cfg(feature = "window")]
pub mod window;

#[cfg(all(feature = "window", feature = "input", feature = "ime"))]
pub mod window_host;

#[cfg(feature = "webview")]
pub mod webview;

#[cfg(feature = "pty")]
pub mod pty;

#[cfg(feature = "process")]
pub mod process;

#[cfg(feature = "process-control")]
pub mod process_control;

#[cfg(feature = "process-image")]
pub mod process_image;

#[cfg(feature = "process-metrics")]
pub mod process_metrics;
#[cfg(feature = "process-observation")]
pub mod process_observation;

#[cfg(feature = "process-reference")]
pub mod process_reference;

#[cfg(feature = "process-security")]
pub mod process_security;

#[cfg(feature = "process-spawn")]
pub mod process_spawn;

#[cfg(feature = "shared-memory")]
pub mod shared_memory;

#[cfg(feature = "window")]
pub mod process_window;

#[cfg(feature = "process")]
pub mod runtime;

#[cfg(feature = "screenshot")]
pub mod screenshot;

mod selected;

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_capabilities_are_explicit() {
        #[cfg(not(feature = "ipc"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Ipc),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
        #[cfg(not(feature = "console-interrupt"))]
        assert_eq!(
            crate::capability_status(crate::Capability::ConsoleInterrupt),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-conventions")]
    #[test]
    fn filesystem_conventions_do_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemConventions),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-cleanup")]
    #[test]
    fn filesystem_cleanup_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemCleanup),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-entry")]
    #[test]
    fn filesystem_entry_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemEntry),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-open")]
    #[test]
    fn filesystem_open_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemOpen),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-publish")]
    #[test]
    fn filesystem_publish_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemPublish),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "filesystem-usage")]
    #[test]
    fn filesystem_usage_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FilesystemUsage),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "file-identity")]
    #[test]
    fn file_identity_does_not_claim_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::FileIdentity),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "host-memory")]
    #[test]
    fn host_memory_does_not_claim_processor_hardware() {
        assert_eq!(
            crate::capability_status(crate::Capability::HostMemory),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "hardware"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Hardware),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "cache-hierarchy")]
    #[test]
    fn cache_hierarchy_does_not_claim_processor_features_or_topology() {
        assert_eq!(
            crate::capability_status(crate::Capability::CacheHierarchy),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "hardware"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Hardware),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
        #[cfg(not(feature = "processor-topology"))]
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessorTopology),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "processor-affinity")]
    #[test]
    fn processor_affinity_does_not_claim_hardware_or_topology() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessorAffinity),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "hardware"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Hardware),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
        #[cfg(not(feature = "processor-topology"))]
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessorTopology),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "process-control")]
    #[test]
    fn process_control_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessControl),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "process-observation")]
    #[test]
    fn process_observation_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessObservation),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "process-reference")]
    #[test]
    fn process_reference_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessReference),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "process-security")]
    #[test]
    fn process_security_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessSecurity),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "process-spawn")]
    #[test]
    fn process_spawn_does_not_claim_full_process() {
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessSpawn),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Process),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "user-identity")]
    #[test]
    fn user_identity_does_not_claim_filesystem_or_ipc() {
        assert_eq!(
            crate::capability_status(crate::Capability::UserIdentity),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
        #[cfg(not(feature = "ipc"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Ipc),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "hardware")]
    #[test]
    fn hardware_does_not_enable_native_services() {
        assert_eq!(
            crate::capability_status(crate::Capability::Hardware),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "process-control"))]
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessControl),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[cfg(feature = "entropy")]
    #[test]
    fn entropy_does_not_enable_product_services() {
        assert_eq!(
            crate::capability_status(crate::Capability::Entropy),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
        #[cfg(not(feature = "process-control"))]
        assert_eq!(
            crate::capability_status(crate::Capability::ProcessControl),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }
}
