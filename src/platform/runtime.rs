//! Compatibility projection for OS runtime defaults.

/// Returns the native default interactive shell without exposing an OS branch
/// to terminal product logic.
pub(crate) fn default_terminal_shell() -> String {
    crate::platform::services::runtime::default_terminal_shell()
}

/// Returns the host's primary shell choice for terminal-creation UI.
pub(crate) fn primary_terminal_shell() -> crate::platform::contract::runtime::TerminalShellDescriptor
{
    crate::platform::services::runtime::primary_terminal_shell()
}

/// Returns the `LANG` value to inject into terminal children when the GUI
/// process environment has no locale, or `None` to leave it untouched.
pub(crate) fn preferred_terminal_lang() -> Option<String> {
    crate::platform::services::runtime::preferred_terminal_lang()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_is_nonempty() {
        assert!(!default_terminal_shell().is_empty());
    }

    #[test]
    fn primary_shell_descriptor_is_complete() {
        let shell = primary_terminal_shell();
        assert!(!shell.id.is_empty());
        assert!(!shell.label.is_empty());
        assert!(!shell.program.is_empty());
    }
}
