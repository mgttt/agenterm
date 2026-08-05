//! stderr fallback for hosts without a native blocking alert surface.

pub(crate) fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}
