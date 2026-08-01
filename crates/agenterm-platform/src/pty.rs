//! PTY facade projection; native handles remain adapter-private to selection.

pub use crate::contract::pty::{
    InvalidProcessId, NativeInputOwnership, NativeTerminalKey, ProcessId, PtyError, PtyResult,
    TerminalSize,
};
pub use crate::selected::pty::{ChildCommand, PtyChild, PtyMaster, login_shell_argument};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_shell_argument_is_selected_by_the_platform_adapter() {
        #[cfg(windows)]
        {
            assert_eq!(login_shell_argument(std::path::Path::new("bash"), 0), None);
        }
        #[cfg(unix)]
        {
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/zsh"), 0),
                Some("-l")
            );
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/zsh"), 1),
                None
            );
            assert_eq!(
                login_shell_argument(std::path::Path::new("/bin/custom-shell"), 0),
                None
            );
        }
    }
}
