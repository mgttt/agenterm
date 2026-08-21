//! Optional capability-based host backend for `std` embeddings.
//!
//! The embedding explicitly opens ambient directories once. All guest path
//! operations then use `cap-std` directory capabilities and never reconstruct
//! an ambient path. This works for desktop hosts and for app-container paths
//! supplied by an iOS embedding.

use alloc::vec::Vec;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, FileType as CapFileType, OpenOptions as CapOpenOptions};
use core::time::Duration;
use std::io::{Error as IoError, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{
    FileStat, FileType, HostBackend, HostClock, HostError, HostHandle, HostResult, OpenOptions,
    SeekWhence,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StdHostLimits {
    pub max_handles: u32,
}

impl Default for StdHostLimits {
    fn default() -> Self {
        Self { max_handles: 128 }
    }
}

enum Resource {
    Directory(Dir),
    File(File),
}

/// Capability-based backend for the platform-neutral [`HostBackend`] trait.
pub struct StdHostBackend {
    limits: StdHostLimits,
    resources: Vec<Option<Resource>>,
    monotonic_origin: Instant,
    exit_code: Option<u32>,
}

impl StdHostBackend {
    pub fn new(limits: StdHostLimits) -> Self {
        Self {
            limits,
            resources: Vec::new(),
            monotonic_origin: Instant::now(),
            exit_code: None,
        }
    }

    /// Opens one host-chosen ambient directory for later virtual preopening.
    /// Guest operations never receive or reconstruct this ambient path.
    pub fn open_ambient_preopen(&mut self, path: impl AsRef<Path>) -> HostResult<HostHandle> {
        let directory = Dir::open_ambient_dir(path, ambient_authority()).map_err(io_error)?;
        self.insert(Resource::Directory(directory))
    }

    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    pub fn take_exit_code(&mut self) -> Option<u32> {
        self.exit_code.take()
    }

    fn insert(&mut self, resource: Resource) -> HostResult<HostHandle> {
        if let Some((index, slot)) = self
            .resources
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(resource);
            return u32::try_from(index)
                .map(HostHandle::new)
                .map_err(|_| HostError::Overflow);
        }
        if self.resources.len() >= self.limits.max_handles as usize {
            return Err(HostError::TooManyDescriptors);
        }
        self.resources
            .try_reserve_exact(1)
            .map_err(|_| HostError::AllocationFailed)?;
        let index = u32::try_from(self.resources.len()).map_err(|_| HostError::Overflow)?;
        self.resources.push(Some(resource));
        Ok(HostHandle::new(index))
    }

    fn resource(&self, handle: HostHandle) -> HostResult<&Resource> {
        self.resources
            .get(handle.raw() as usize)
            .and_then(Option::as_ref)
            .ok_or(HostError::BadHandle)
    }

    fn resource_mut(&mut self, handle: HostHandle) -> HostResult<&mut Resource> {
        self.resources
            .get_mut(handle.raw() as usize)
            .and_then(Option::as_mut)
            .ok_or(HostError::BadHandle)
    }
}

impl Default for StdHostBackend {
    fn default() -> Self {
        Self::new(StdHostLimits::default())
    }
}

impl HostBackend for StdHostBackend {
    fn clock_now(&mut self, clock: HostClock) -> HostResult<u64> {
        let duration = match clock {
            HostClock::Realtime => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| HostError::Io)?,
            HostClock::Monotonic => self.monotonic_origin.elapsed(),
            HostClock::ProcessCpu | HostClock::ThreadCpu => {
                return Err(HostError::NotSupported);
            }
        };
        u64::try_from(duration.as_nanos()).map_err(|_| HostError::Overflow)
    }

    fn sleep(&mut self, duration_nanoseconds: u64) -> HostResult<()> {
        std::thread::sleep(Duration::from_nanos(duration_nanoseconds));
        Ok(())
    }

    fn random_fill(&mut self, output: &mut [u8]) -> HostResult<()> {
        getrandom::fill(output).map_err(|_| HostError::Io)
    }

    fn fd_read(&mut self, handle: HostHandle, output: &mut [u8]) -> HostResult<usize> {
        match self.resource_mut(handle)? {
            Resource::File(file) => file.read(output).map_err(io_error),
            Resource::Directory(_) => Err(HostError::IsDirectory),
        }
    }

    fn fd_write(&mut self, handle: HostHandle, input: &[u8]) -> HostResult<usize> {
        match self.resource_mut(handle)? {
            Resource::File(file) => file.write(input).map_err(io_error),
            Resource::Directory(_) => Err(HostError::IsDirectory),
        }
    }

    fn fd_seek(&mut self, handle: HostHandle, offset: i64, whence: SeekWhence) -> HostResult<u64> {
        let position = match whence {
            SeekWhence::Start => {
                SeekFrom::Start(u64::try_from(offset).map_err(|_| HostError::Invalid)?)
            }
            SeekWhence::Current => SeekFrom::Current(offset),
            SeekWhence::End => SeekFrom::End(offset),
        };
        match self.resource_mut(handle)? {
            Resource::File(file) => file.seek(position).map_err(io_error),
            Resource::Directory(_) => Err(HostError::IsDirectory),
        }
    }

    fn fd_close(&mut self, handle: HostHandle) -> HostResult<()> {
        self.resources
            .get_mut(handle.raw() as usize)
            .and_then(Option::take)
            .map(|_| ())
            .ok_or(HostError::BadHandle)
    }

    fn fd_stat(&mut self, handle: HostHandle) -> HostResult<FileStat> {
        let metadata = match self.resource(handle)? {
            Resource::Directory(directory) => directory.dir_metadata().map_err(io_error)?,
            Resource::File(file) => file.metadata().map_err(io_error)?,
        };
        Ok(FileStat {
            file_type: file_type(metadata.file_type()),
            size: metadata.len(),
        })
    }

    fn path_open(
        &mut self,
        directory: HostHandle,
        path: &str,
        options: OpenOptions,
    ) -> HostResult<HostHandle> {
        let resource = match self.resource(directory)? {
            Resource::Directory(directory) => {
                if options.directory {
                    if options.create || options.read || options.truncate || options.write {
                        return Err(HostError::NotSupported);
                    }
                    Resource::Directory(directory.open_dir(path).map_err(io_error)?)
                } else {
                    let mut open = CapOpenOptions::new();
                    open.read(options.read)
                        .write(options.write)
                        .create(options.create)
                        .truncate(options.truncate);
                    Resource::File(directory.open_with(path, &open).map_err(io_error)?)
                }
            }
            Resource::File(_) => return Err(HostError::NotDirectory),
        };
        self.insert(resource)
    }

    fn path_unlink(&mut self, directory: HostHandle, path: &str) -> HostResult<()> {
        match self.resource(directory)? {
            Resource::Directory(directory) => directory.remove_file(path).map_err(io_error),
            Resource::File(_) => Err(HostError::NotDirectory),
        }
    }

    fn exit(&mut self, code: u32) -> HostResult<()> {
        self.exit_code = Some(code);
        Ok(())
    }
}

fn file_type(file_type: CapFileType) -> FileType {
    if file_type.is_dir() {
        FileType::Directory
    } else if file_type.is_file() {
        FileType::RegularFile
    } else if file_type.is_symlink() {
        FileType::SymbolicLink
    } else {
        FileType::Unknown
    }
}

fn io_error(error: IoError) -> HostError {
    match error.kind() {
        ErrorKind::NotFound => HostError::NotFound,
        ErrorKind::PermissionDenied => HostError::PermissionDenied,
        ErrorKind::AlreadyExists => HostError::AlreadyExists,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => HostError::Invalid,
        ErrorKind::IsADirectory => HostError::IsDirectory,
        ErrorKind::NotADirectory => HostError::NotDirectory,
        ErrorKind::Unsupported => HostError::NotSupported,
        _ => HostError::Io,
    }
}
