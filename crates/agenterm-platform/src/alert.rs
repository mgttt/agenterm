//! Minimal blocking user alert for launch-time failures.
//!
//! The GUI cannot show its own window when startup fails (for example when a
//! live server rejects the UI contract), so a native message box is the only
//! visible surface left. Hosts without a native box fall back to stderr.

/// Show a blocking error dialog with a single OK button.
pub fn show_error(title: &str, message: &str) {
    crate::selected::alert::show_error(title, message);
}
