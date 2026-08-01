//! Current-user named shared memory with native RAII ownership.

pub use crate::contract::shared_memory::{SharedMemoryError, SharedMemoryErrorKind};

/// One read/write view of a named shared-memory object.
///
/// Keep the creator alive while peers still need to discover the name. POSIX
/// removes that name when the creator is dropped; Windows retains it until the
/// last native handle closes. Existing views remain valid in both cases.
/// Product protocols remain responsible for synchronization and the byte layout.
pub struct SharedMemory {
    inner: crate::selected::shared_memory::SharedMemory,
    name: String,
    len: usize,
}

impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedMemory")
            .field("name", &self.name)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl SharedMemory {
    /// Create a new mapping, failing if the portable name already exists.
    pub fn create(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        validate(name, len)?;
        crate::selected::shared_memory::SharedMemory::create(name, len).map(|inner| Self {
            inner,
            name: name.to_owned(),
            len,
        })
    }

    /// Open an existing mapping using its agreed byte length.
    pub fn open(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        validate(name, len)?;
        crate::selected::shared_memory::SharedMemory::open(name, len).map(|inner| Self {
            inner,
            name: name.to_owned(),
            len,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Return the first byte of this process's mapped view.
    pub fn as_ptr(&self) -> *const u8 {
        self.inner.as_ptr()
    }

    /// Return the first byte of this process's writable mapped view.
    ///
    /// Cross-process synchronization is the caller's protocol responsibility.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.inner.as_mut_ptr()
    }
}

fn validate(name: &str, len: usize) -> Result<(), SharedMemoryError> {
    if name.is_empty()
        || name.len() > 120
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(SharedMemoryError::new(
            SharedMemoryErrorKind::InvalidName,
            "name must be 1..=120 ASCII letters, digits, '.', '_' or '-'",
        ));
    }
    if len == 0 || len > isize::MAX as usize {
        return Err(SharedMemoryError::new(
            SharedMemoryErrorKind::InvalidLength,
            "length must be between 1 and isize::MAX bytes",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name(label: &str) -> String {
        format!("agenterm-platform-{label}-{}", std::process::id())
    }

    #[test]
    fn named_views_share_bytes_and_creation_is_exclusive() {
        let name = unique_name("views");
        let mut first = SharedMemory::create(&name, 4096).expect("create mapping");
        let second = SharedMemory::open(&name, 4096).expect("open mapping");
        assert_eq!(first.name(), name);
        assert_eq!(first.len(), 4096);
        assert!(!first.is_empty());
        let duplicate = SharedMemory::create(&name, 4096).expect_err("exclusive creation");
        assert_eq!(duplicate.kind(), SharedMemoryErrorKind::AlreadyExists);
        // SAFETY: both live views cover 4096 bytes; no concurrent writer exists.
        unsafe { first.as_mut_ptr().cast::<u64>().write(0x1234_5678) };
        assert_eq!(unsafe { second.as_ptr().cast::<u64>().read() }, 0x1234_5678);
    }

    #[test]
    fn rejects_nonportable_names_and_lengths() {
        for name in ["", "with/slash", "with\\slash", "with space"] {
            assert_eq!(
                SharedMemory::create(name, 1).unwrap_err().kind(),
                SharedMemoryErrorKind::InvalidName
            );
        }
        assert_eq!(
            SharedMemory::create("valid-name", 0).unwrap_err().kind(),
            SharedMemoryErrorKind::InvalidLength
        );
    }
}
