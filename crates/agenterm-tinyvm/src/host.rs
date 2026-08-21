//! Platform-neutral host contract for optional system capabilities.
//!
//! WebAssembly imports are the public guest boundary. This module sits one
//! layer below an adapter such as WASI Preview 1: it owns guest descriptor
//! numbers, virtual preopens and capability checks while a platform backend
//! owns native handles and OS calls. The VM engine never sees an OS descriptor
//! or physical path.

use alloc::string::String;
use alloc::vec::Vec;

pub type HostResult<T> = Result<T, HostError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    AllocationFailed,
    BadHandle,
    Invalid,
    InvalidPath,
    Io,
    NotCapable,
    NotSupported,
    Overflow,
    ProcessTooLarge,
    TooManyDescriptors,
    TooManyPreopens,
}

/// Backend-owned opaque handle. It is never exposed to a guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostHandle(u32);

impl HostHandle {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Guest-visible descriptor number, suitable for an i32 Wasm ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestFd(u32);

impl GuestFd {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HostClock {
    Realtime,
    Monotonic,
    ProcessCpu,
    ThreadCpu,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    Start,
    Current,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Unknown,
    BlockDevice,
    CharacterDevice,
    Directory,
    RegularFile,
    Socket,
    SymbolicLink,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub file_type: FileType,
    pub size: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenOptions {
    pub create: bool,
    pub directory: bool,
    pub read: bool,
    pub truncate: bool,
    pub write: bool,
}

impl OpenOptions {
    pub const fn read_only() -> Self {
        Self {
            create: false,
            directory: false,
            read: true,
            truncate: false,
            write: false,
        }
    }

    pub const fn read_write() -> Self {
        Self {
            create: false,
            directory: false,
            read: true,
            truncate: false,
            write: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DescriptorRights(u16);

impl DescriptorRights {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const SEEK: Self = Self(1 << 2);
    pub const STAT: Self = Self(1 << 3);
    pub const PATH_OPEN: Self = Self(1 << 4);
    pub const PATH_UNLINK: Self = Self(1 << 5);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HostLimits {
    pub max_descriptors: u32,
    pub max_preopens: u32,
    pub max_process_entries: u32,
    pub max_process_bytes: u32,
}

impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_descriptors: 64,
            max_preopens: 16,
            max_process_entries: 64,
            max_process_bytes: 16 * 1024,
        }
    }
}

/// Native mechanism implemented by Unix, Windows, iOS or an embedding.
///
/// Paths are always relative to a previously registered directory handle.
/// An implementation must never accept or return a guest descriptor here.
pub trait HostBackend {
    fn clock_now(&mut self, clock: HostClock) -> HostResult<u64>;
    fn sleep(&mut self, duration_nanoseconds: u64) -> HostResult<()>;
    fn random_fill(&mut self, output: &mut [u8]) -> HostResult<()>;
    fn fd_read(&mut self, handle: HostHandle, output: &mut [u8]) -> HostResult<usize>;
    fn fd_write(&mut self, handle: HostHandle, input: &[u8]) -> HostResult<usize>;
    fn fd_seek(&mut self, handle: HostHandle, offset: i64, whence: SeekWhence) -> HostResult<u64>;
    fn fd_close(&mut self, handle: HostHandle) -> HostResult<()>;
    fn fd_stat(&mut self, handle: HostHandle) -> HostResult<FileStat>;
    fn path_open(
        &mut self,
        directory: HostHandle,
        path: &str,
        options: OpenOptions,
    ) -> HostResult<HostHandle>;
    fn path_unlink(&mut self, directory: HostHandle, path: &str) -> HostResult<()>;
    fn exit(&mut self, code: u32) -> HostResult<()>;
}

struct Descriptor {
    handle: HostHandle,
    rights: DescriptorRights,
    preopen_name: Option<String>,
}

/// Bounded guest descriptor and preopen owner over one platform backend.
pub struct HostContext<B> {
    backend: B,
    limits: HostLimits,
    descriptors: Vec<Option<Descriptor>>,
    preopen_count: u32,
    args: Vec<String>,
    environ: Vec<String>,
}

impl<B: HostBackend> HostContext<B> {
    pub fn new(backend: B, limits: HostLimits) -> Self {
        Self {
            backend,
            limits,
            descriptors: Vec::new(),
            preopen_count: 0,
            args: Vec::new(),
            environ: Vec::new(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn into_backend(self) -> B {
        self.backend
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn environ(&self) -> &[String] {
        &self.environ
    }

    /// Replaces process strings only after the complete bounded input passes.
    pub fn set_process_values(
        &mut self,
        args: Vec<String>,
        environ: Vec<String>,
    ) -> HostResult<()> {
        let entries = args
            .len()
            .checked_add(environ.len())
            .ok_or(HostError::Overflow)?;
        if entries > self.limits.max_process_entries as usize {
            return Err(HostError::ProcessTooLarge);
        }
        let mut bytes = 0usize;
        for value in args.iter().chain(environ.iter()) {
            if value.as_bytes().contains(&0) {
                return Err(HostError::Invalid);
            }
            bytes = bytes
                .checked_add(value.len().checked_add(1).ok_or(HostError::Overflow)?)
                .ok_or(HostError::Overflow)?;
        }
        if bytes > self.limits.max_process_bytes as usize {
            return Err(HostError::ProcessTooLarge);
        }
        self.args = args;
        self.environ = environ;
        Ok(())
    }

    pub fn register_descriptor(
        &mut self,
        handle: HostHandle,
        rights: DescriptorRights,
    ) -> HostResult<GuestFd> {
        self.insert_descriptor(Descriptor {
            handle,
            rights,
            preopen_name: None,
        })
    }

    pub fn register_preopen(
        &mut self,
        handle: HostHandle,
        virtual_root: String,
        rights: DescriptorRights,
    ) -> HostResult<GuestFd> {
        if !is_virtual_root(&virtual_root) {
            return Err(HostError::InvalidPath);
        }
        if self.preopen_count >= self.limits.max_preopens {
            return Err(HostError::TooManyPreopens);
        }
        let descriptor = Descriptor {
            handle,
            rights,
            preopen_name: Some(virtual_root),
        };
        let fd = self.insert_descriptor(descriptor)?;
        self.preopen_count += 1;
        Ok(fd)
    }

    pub fn preopen_name(&self, fd: GuestFd) -> HostResult<Option<&str>> {
        Ok(self.descriptor(fd)?.preopen_name.as_deref())
    }

    pub fn clock_now(&mut self, clock: HostClock) -> HostResult<u64> {
        self.backend.clock_now(clock)
    }

    pub fn sleep(&mut self, duration_nanoseconds: u64) -> HostResult<()> {
        self.backend.sleep(duration_nanoseconds)
    }

    pub fn random_fill(&mut self, output: &mut [u8]) -> HostResult<()> {
        self.backend.random_fill(output)
    }

    pub fn fd_read(&mut self, fd: GuestFd, output: &mut [u8]) -> HostResult<usize> {
        let handle = self.handle_with(fd, DescriptorRights::READ)?;
        self.backend.fd_read(handle, output)
    }

    pub fn fd_write(&mut self, fd: GuestFd, input: &[u8]) -> HostResult<usize> {
        let handle = self.handle_with(fd, DescriptorRights::WRITE)?;
        self.backend.fd_write(handle, input)
    }

    pub fn fd_seek(&mut self, fd: GuestFd, offset: i64, whence: SeekWhence) -> HostResult<u64> {
        let handle = self.handle_with(fd, DescriptorRights::SEEK)?;
        self.backend.fd_seek(handle, offset, whence)
    }

    pub fn fd_stat(&mut self, fd: GuestFd) -> HostResult<FileStat> {
        let handle = self.handle_with(fd, DescriptorRights::STAT)?;
        self.backend.fd_stat(handle)
    }

    pub fn fd_close(&mut self, fd: GuestFd) -> HostResult<()> {
        let index = fd.raw() as usize;
        let descriptor = self
            .descriptors
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(HostError::BadHandle)?;
        if descriptor.preopen_name.is_some() {
            self.preopen_count -= 1;
        }
        self.backend.fd_close(descriptor.handle)
    }

    pub fn path_open(
        &mut self,
        preopen: GuestFd,
        path: &str,
        options: OpenOptions,
        rights: DescriptorRights,
    ) -> HostResult<GuestFd> {
        if !is_relative_guest_path(path) {
            return Err(HostError::InvalidPath);
        }
        if options.read != rights.contains(DescriptorRights::READ)
            || options.write != rights.contains(DescriptorRights::WRITE)
            || ((options.create || options.truncate) && !options.write)
            || (options.directory && options.truncate)
        {
            return Err(HostError::Invalid);
        }
        let descriptor = self.descriptor(preopen)?;
        if descriptor.preopen_name.is_none()
            || !descriptor.rights.contains(DescriptorRights::PATH_OPEN)
            || !descriptor.rights.contains(rights)
        {
            return Err(HostError::NotCapable);
        }
        let handle = descriptor.handle;
        self.reserve_descriptor_slot()?;
        let opened = self.backend.path_open(handle, path, options)?;
        match self.register_descriptor(opened, rights) {
            Ok(fd) => Ok(fd),
            Err(error) => {
                let _ = self.backend.fd_close(opened);
                Err(error)
            }
        }
    }

    pub fn path_unlink(&mut self, preopen: GuestFd, path: &str) -> HostResult<()> {
        if !is_relative_guest_path(path) {
            return Err(HostError::InvalidPath);
        }
        let handle = self.handle_with(preopen, DescriptorRights::PATH_UNLINK)?;
        if self.descriptor(preopen)?.preopen_name.is_none() {
            return Err(HostError::NotCapable);
        }
        self.backend.path_unlink(handle, path)
    }

    pub fn exit(&mut self, code: u32) -> HostResult<()> {
        self.backend.exit(code)
    }

    fn insert_descriptor(&mut self, descriptor: Descriptor) -> HostResult<GuestFd> {
        if let Some((index, slot)) = self
            .descriptors
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(descriptor);
            return Ok(GuestFd(index as u32));
        }
        self.reserve_descriptor_slot()?;
        let index = u32::try_from(self.descriptors.len()).map_err(|_| HostError::Overflow)?;
        self.descriptors.push(Some(descriptor));
        Ok(GuestFd(index))
    }

    fn reserve_descriptor_slot(&mut self) -> HostResult<()> {
        if self.descriptors.iter().any(Option::is_none) {
            return Ok(());
        }
        if self.descriptors.len() >= self.limits.max_descriptors as usize {
            return Err(HostError::TooManyDescriptors);
        }
        self.descriptors
            .try_reserve_exact(1)
            .map_err(|_| HostError::AllocationFailed)
    }

    fn descriptor(&self, fd: GuestFd) -> HostResult<&Descriptor> {
        self.descriptors
            .get(fd.raw() as usize)
            .and_then(Option::as_ref)
            .ok_or(HostError::BadHandle)
    }

    fn handle_with(&self, fd: GuestFd, required: DescriptorRights) -> HostResult<HostHandle> {
        let descriptor = self.descriptor(fd)?;
        if !descriptor.rights.contains(required) {
            return Err(HostError::NotCapable);
        }
        Ok(descriptor.handle)
    }
}

fn is_virtual_root(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    path == "/" || is_component_path(&path[1..])
}

fn is_relative_guest_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && is_component_path(path)
}

fn is_component_path(path: &str) -> bool {
    !path.ends_with('/')
        && !path.as_bytes().contains(&0)
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}
