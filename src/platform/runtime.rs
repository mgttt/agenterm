//! OS runtime defaults exposed through the Platform Facade.

use std::env;

/// Returns the native default interactive shell without exposing an OS branch
/// to terminal product logic.
pub(crate) fn default_terminal_shell() -> String {
    #[cfg(windows)]
    {
        env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned())
    }
    #[cfg(unix)]
    {
        env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_is_nonempty() {
        assert!(!default_terminal_shell().is_empty());
    }
}
