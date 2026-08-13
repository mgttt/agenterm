//! POSIX named shared memory for Linux and macOS.

use std::ffi::CString;

use crate::contract::shared_memory::{SharedMemoryError, SharedMemoryErrorKind};

pub(crate) struct SharedMemory {
    descriptor: libc::c_int,
    view: *mut libc::c_void,
    len: usize,
    native_name: CString,
    creator: bool,
}

/// Set `FD_CLOEXEC` after `shm_open`.
///
/// `O_CLOEXEC` cannot be passed to `shm_open` portably: Linux documents it,
/// macOS rejects the whole call with EINVAL, which is why shared memory
/// looked broken on macOS ("create mapping: Invalid argument (os error 22)")
/// until CI first ran these tests there. Setting the flag afterwards costs
/// one fcntl and behaves the same on both.
fn set_cloexec(descriptor: libc::c_int) {
    // A failure here only means the descriptor survives exec; it is not worth
    // failing an otherwise successful mapping over, and there is nothing the
    // caller could do about it.
    unsafe {
        let flags = libc::fcntl(descriptor, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

impl SharedMemory {
    pub(crate) fn create(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        let native_name = native_name(name);
        let descriptor = unsafe {
            libc::shm_open(
                native_name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(errno_error(
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                    SharedMemoryErrorKind::AlreadyExists
                } else {
                    SharedMemoryErrorKind::Create
                },
            ));
        }
        set_cloexec(descriptor);
        let length = libc::off_t::try_from(len).map_err(|source| {
            cleanup_created(descriptor, &native_name);
            SharedMemoryError::new(SharedMemoryErrorKind::InvalidLength, source.to_string())
        })?;
        if unsafe { libc::ftruncate(descriptor, length) } != 0 {
            let error = errno_error(SharedMemoryErrorKind::Resize);
            cleanup_created(descriptor, &native_name);
            return Err(error);
        }
        Self::map(descriptor, native_name, len, true)
    }

    pub(crate) fn open(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        let native_name = native_name(name);
        let descriptor = unsafe { libc::shm_open(native_name.as_ptr(), libc::O_RDWR, 0) };
        if descriptor < 0 {
            let native = std::io::Error::last_os_error();
            let kind = if native.raw_os_error() == Some(libc::ENOENT) {
                SharedMemoryErrorKind::NotFound
            } else {
                SharedMemoryErrorKind::Open
            };
            return Err(SharedMemoryError::new(kind, native.to_string()));
        }
        set_cloexec(descriptor);
        let mut status = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe { libc::fstat(descriptor, &mut status) } != 0 {
            let error = errno_error(SharedMemoryErrorKind::Inspect);
            unsafe { libc::close(descriptor) };
            return Err(error);
        }
        let actual = u64::try_from(status.st_size).unwrap_or_default();
        if actual < len as u64 {
            unsafe { libc::close(descriptor) };
            return Err(SharedMemoryError::new(
                SharedMemoryErrorKind::SizeMismatch,
                format!("mapping contains {actual} bytes but {len} were requested"),
            ));
        }
        Self::map(descriptor, native_name, len, false)
    }

    fn map(
        descriptor: libc::c_int,
        native_name: CString,
        len: usize,
        creator: bool,
    ) -> Result<Self, SharedMemoryError> {
        let view = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                descriptor,
                0,
            )
        };
        if view == libc::MAP_FAILED {
            let error = errno_error(SharedMemoryErrorKind::Map);
            unsafe { libc::close(descriptor) };
            if creator {
                unsafe { libc::shm_unlink(native_name.as_ptr()) };
            }
            return Err(error);
        }
        Ok(Self {
            descriptor,
            view,
            len,
            native_name,
            creator,
        })
    }

    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.view.cast()
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.view.cast()
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.view, self.len);
            libc::close(self.descriptor);
            if self.creator {
                libc::shm_unlink(self.native_name.as_ptr());
            }
        }
    }
}

fn native_name(name: &str) -> CString {
    CString::new(format!("/{name}")).expect("validated shared-memory name")
}

fn cleanup_created(descriptor: libc::c_int, native_name: &CString) {
    unsafe {
        libc::close(descriptor);
        libc::shm_unlink(native_name.as_ptr());
    }
}

fn errno_error(kind: SharedMemoryErrorKind) -> SharedMemoryError {
    SharedMemoryError::new(kind, std::io::Error::last_os_error().to_string())
}
