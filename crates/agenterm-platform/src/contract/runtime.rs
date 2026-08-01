//! OS-neutral descriptors for native terminal runtime defaults.

/// A shell choice exposed by a platform-neutral terminal UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalShellDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub program: &'static str,
}
