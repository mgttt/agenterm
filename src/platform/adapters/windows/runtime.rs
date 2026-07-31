//! Windows runtime defaults.

pub(crate) fn default_terminal_shell() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned())
}
