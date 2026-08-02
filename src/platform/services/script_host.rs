//! Whether the native CLI host exposes the Script worker embedding path.

pub(crate) const fn hosted_worker_available() -> bool {
    crate::platform::hosted_script_worker_available()
}
