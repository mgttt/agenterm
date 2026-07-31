//! Compatibility projection for OS runtime defaults.

/// Returns the native default interactive shell without exposing an OS branch
/// to terminal product logic.
pub(crate) fn default_terminal_shell() -> String {
    crate::platform::services::runtime::default_terminal_shell()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_is_nonempty() {
        assert!(!default_terminal_shell().is_empty());
    }
}
