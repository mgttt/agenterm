#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::NamedAuditLock;
#[cfg(windows)]
pub(crate) use windows::NamedAuditLock;
