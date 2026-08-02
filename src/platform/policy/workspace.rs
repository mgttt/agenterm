//! Product workspace layout policy shared by host adapters.
//!
//! The native mechanism lives in agenterm-platform; this table is the
//! AgenTerm-level product decision for workspace directory shape.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum WorkspaceLayoutKind {
    WindowsFlat,
    DirectoryByScope,
}

#[allow(dead_code)]
pub(crate) fn workspace_layout_kind() -> WorkspaceLayoutKind {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        WorkspaceLayoutKind::WindowsFlat
    } else {
        WorkspaceLayoutKind::DirectoryByScope
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceLayoutKind, workspace_layout_kind};

    #[test]
    fn layout_kind_matches_runtime_kind() {
        let expected = if matches!(
            agenterm_platform::platform_kind(),
            agenterm_platform::PlatformKind::Windows
        ) {
            WorkspaceLayoutKind::WindowsFlat
        } else {
            WorkspaceLayoutKind::DirectoryByScope
        };
        assert_eq!(workspace_layout_kind(), expected);
    }
}
