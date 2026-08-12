//! Windows named-pipe adapter for the target-neutral local IPC facade.

use std::{
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt as _, OsStringExt as _},
        io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle as _, OwnedHandle, RawHandle},
    },
    ptr,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, InitializeAcl,
        InitializeSecurityDescriptor, SECURITY_ATTRIBUTES, SetSecurityDescriptorDacl,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, GetTempPathW, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
            WaitNamedPipeW,
        },
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

use crate::{
    contract::ipc_transport::{
        IpcTransportError, IpcTransportErrorCode, TransportResult, timeout_error, transport_io,
    },
    ipc::{IpcEndpoint, TrustedUserIdentity},
};

pub(crate) const NATIVE_TRANSPORT_NAME: &str = "named_pipe";
const PIPE_BUFFER_BYTES: u32 = 256 * 1024;
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
const PIPE_NAME_MAX_UTF16: usize = 256;

pub(crate) fn native_runtime_directory() -> std::path::PathBuf {
    const INITIAL_UNITS: usize = 261;
    const MAX_UNITS: usize = 32_768;

    let mut buffer = vec![0u16; INITIAL_UNITS];
    loop {
        let length = unsafe {
            // SAFETY: buffer is writable for its advertised length and remains
            // alive for the call. GetTempPathW writes at most that capacity.
            GetTempPathW(buffer.len() as u32, buffer.as_mut_ptr())
        } as usize;
        if length == 0 || length >= MAX_UNITS {
            return std::path::PathBuf::from(".");
        }
        if length < buffer.len() {
            return std::path::PathBuf::from(OsString::from_wide(&buffer[..length]));
        }
        buffer.resize(length.saturating_add(1), 0);
    }
}

#[cfg(test)]
mod runtime_directory_tests {
    #[test]
    fn native_temp_directory_is_absolute_and_nonempty() {
        let path = super::native_runtime_directory();
        assert!(path.is_absolute(), "native temp path: {path:?}");
        assert!(!path.as_os_str().is_empty());
    }
}

pub(crate) fn trusted_user_identity() -> io::Result<TrustedUserIdentity> {
    let identity = crate::user_identity::current_user_identity()?;
    Ok(TrustedUserIdentity {
        kind: identity.stable_kind(),
        bytes: identity.stable_bytes(),
    })
}

pub(crate) struct NativeListener {
    name: Vec<u16>,
    endpoint: IpcEndpoint,
    security: PipeSecurity,
    pending: Option<OwnedHandle>,
}

impl NativeListener {
    pub(crate) fn bind(endpoint: &IpcEndpoint) -> TransportResult<Self> {
        let IpcEndpoint::NamedPipe(name) = endpoint else {
            return Err(crate::contract::ipc_transport::unsupported(
                endpoint,
                "this host native IPC adapter accepts only Windows named pipes",
            ));
        };
        let name = validated_pipe_name(name, endpoint)?;
        let mut security = PipeSecurity::for_current_user(endpoint)?;
        let pending = create_pipe(&name, true, &mut security, endpoint)?;
        Ok(Self {
            name,
            endpoint: endpoint.clone(),
            security,
            pending: Some(pending),
        })
    }

    pub(crate) fn accept(&mut self, timeout: Duration) -> TransportResult<NativeStream> {
        let pipe = match self.pending.take() {
            Some(pipe) => pipe,
            None => create_pipe(&self.name, false, &mut self.security, &self.endpoint)?,
        };
        match connect_pipe_instance(&pipe, timeout, &self.endpoint) {
            Ok(()) => {
                self.pending = Some(create_pipe(
                    &self.name,
                    false,
                    &mut self.security,
                    &self.endpoint,
                )?);
                Ok(NativeStream {
                    handle: pipe,
                    timeout,
                })
            }
            Err(error) => {
                unsafe {
                    DisconnectNamedPipe(pipe.as_raw_handle());
                }
                self.pending = Some(pipe);
                Err(error)
            }
        }
    }
}

pub(crate) struct NativeStream {
    handle: OwnedHandle,
    timeout: Duration,
}

impl AsRawHandle for NativeStream {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl AsHandle for NativeStream {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl AsRawHandle for crate::ipc::NativeStream {
    fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

impl AsHandle for crate::ipc::NativeStream {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

pub trait NativeStreamExt: Sized {
    fn from_owned_handle(handle: OwnedHandle, timeout: Duration) -> Self;
    fn into_owned_handle(self) -> OwnedHandle;
}

impl NativeStreamExt for crate::ipc::NativeStream {
    fn from_owned_handle(handle: OwnedHandle, timeout: Duration) -> Self {
        Self(NativeStream::from_owned_handle(handle, timeout))
    }

    fn into_owned_handle(self) -> OwnedHandle {
        self.0.into_owned_handle()
    }
}

impl NativeStream {
    pub(crate) fn from_owned_handle(handle: OwnedHandle, timeout: Duration) -> Self {
        Self { handle, timeout }
    }

    pub(crate) fn into_owned_handle(self) -> OwnedHandle {
        self.handle
    }

    pub(crate) fn connect(endpoint: &IpcEndpoint, timeout: Duration) -> TransportResult<Self> {
        let IpcEndpoint::NamedPipe(name) = endpoint else {
            return Err(crate::contract::ipc_transport::unsupported(
                endpoint,
                "this host native IPC adapter accepts only Windows named pipes",
            ));
        };
        let name = validated_pipe_name(name, endpoint)?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timeout_error(
                    IpcTransportErrorCode::ConnectTimeout,
                    endpoint,
                ));
            }
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Self {
                    handle: unsafe { OwnedHandle::from_raw_handle(handle) },
                    timeout,
                });
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(transport_io(endpoint, error));
            }
            let waited = unsafe { WaitNamedPipeW(name.as_ptr(), duration_ms(remaining)) };
            if waited == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                    return Err(transport_io(endpoint, error));
                }
            }
        }
    }

    pub(crate) fn set_io_timeout(&mut self, timeout: Duration) -> TransportResult<()> {
        self.timeout = timeout;
        Ok(())
    }

    fn raw_handle(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }
}

impl Read for NativeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let handle = self.raw_handle();
        match overlapped_io(handle, self.timeout, |overlapped| unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                length,
                ptr::null_mut(),
                overlapped,
            )
        }) {
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            result => result.map(|count| count as usize),
        }
    }
}

impl Write for NativeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = buffer.len().min(u32::MAX as usize) as u32;
        let handle = self.raw_handle();
        overlapped_io(handle, self.timeout, |overlapped| unsafe {
            WriteFile(handle, buffer.as_ptr(), length, ptr::null_mut(), overlapped)
        })
        .map(|count| count as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn connect_pipe_instance(
    pipe: &OwnedHandle,
    timeout: Duration,
    endpoint: &IpcEndpoint,
) -> TransportResult<()> {
    let raw = pipe.as_raw_handle();
    let event = Event::new().map_err(|error| transport_io(endpoint, error))?;
    let mut overlapped = OVERLAPPED {
        hEvent: event.handle,
        ..Default::default()
    };
    let connected = unsafe { ConnectNamedPipe(raw, &mut overlapped) };
    if connected == 0 {
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|value| value as u32) {
            Some(ERROR_PIPE_CONNECTED) => {}
            Some(ERROR_IO_PENDING) => {
                wait_overlapped(raw, &mut overlapped, timeout).map_err(|error| {
                    let code = if error.kind() == io::ErrorKind::TimedOut {
                        IpcTransportErrorCode::AcceptTimeout
                    } else {
                        IpcTransportErrorCode::Io
                    };
                    IpcTransportError::new(code, endpoint.to_string(), error)
                })?;
            }
            _ => return Err(transport_io(endpoint, error)),
        }
    }
    Ok(())
}

fn create_pipe(
    name: &[u16],
    first: bool,
    security: &mut PipeSecurity,
    endpoint: &IpcEndpoint,
) -> TransportResult<OwnedHandle> {
    let attributes = security.attributes();
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        let raw_error = error.raw_os_error().map(|value| value as u32);
        let code = if first
            && matches!(raw_error, Some(value) if value == ERROR_ACCESS_DENIED || value == ERROR_PIPE_BUSY)
        {
            IpcTransportErrorCode::EndpointInUse
        } else {
            IpcTransportErrorCode::Io
        };
        return Err(IpcTransportError::new(code, endpoint.to_string(), error));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn overlapped_io(
    handle: HANDLE,
    timeout: Duration,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<u32> {
    let event = Event::new()?;
    let mut overlapped = OVERLAPPED {
        hEvent: event.handle,
        ..Default::default()
    };
    if start(&mut overlapped) != 0 {
        let mut transferred = 0;
        if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(transferred);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
        return Err(error);
    }
    wait_overlapped(handle, &mut overlapped, timeout)
}

fn wait_overlapped(
    handle: HANDLE,
    overlapped: &mut OVERLAPPED,
    timeout: Duration,
) -> io::Result<u32> {
    match unsafe { WaitForSingleObject(overlapped.hEvent, duration_ms(timeout)) } {
        WAIT_OBJECT_0 => {
            let mut transferred = 0;
            if unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 0) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(transferred)
            }
        }
        WAIT_TIMEOUT => {
            unsafe {
                CancelIoEx(handle, overlapped);
            }
            let mut ignored = 0;
            unsafe {
                GetOverlappedResult(handle, overlapped, &mut ignored, 1);
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "named-pipe operation timed out",
            ))
        }
        _ => Err(io::Error::last_os_error()),
    }
}

struct Event {
    handle: HANDLE,
}

impl Event {
    fn new() -> io::Result<Self> {
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self { handle })
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

struct PipeSecurity {
    _sid: Vec<usize>,
    acl: Vec<usize>,
    descriptor: Box<[usize; 5]>,
}

impl PipeSecurity {
    fn for_current_user(endpoint: &IpcEndpoint) -> TransportResult<Self> {
        let sid = trusted_user_identity()
            .map_err(|error| transport_io(endpoint, error))?
            .bytes;
        let mut aligned_sid = vec![0usize; sid.len().div_ceil(size_of::<usize>())];
        unsafe {
            ptr::copy_nonoverlapping(
                sid.as_ptr(),
                aligned_sid.as_mut_ptr().cast::<u8>(),
                sid.len(),
            );
        }
        let acl_bytes =
            size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid.len();
        let mut acl = vec![0usize; acl_bytes.div_ceil(size_of::<usize>())];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        let mut descriptor = Box::new([0usize; 5]);
        let descriptor_ptr = descriptor.as_mut_ptr().cast();
        if unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) }
            == 0
            || unsafe { InitializeAcl(acl_ptr, acl_bytes as u32, ACL_REVISION) } == 0
            || unsafe {
                AddAccessAllowedAce(
                    acl_ptr,
                    ACL_REVISION,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    aligned_sid.as_mut_ptr().cast(),
                )
            } == 0
            || unsafe { SetSecurityDescriptorDacl(descriptor_ptr, 1, acl_ptr, 0) } == 0
        {
            return Err(transport_io(endpoint, io::Error::last_os_error()));
        }
        Ok(Self {
            _sid: aligned_sid,
            acl,
            descriptor,
        })
    }

    fn attributes(&mut self) -> SECURITY_ATTRIBUTES {
        let _ = self.acl.len();
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.as_mut_ptr().cast(),
            bInheritHandle: 0,
        }
    }
}

fn validated_pipe_name(name: &str, endpoint: &IpcEndpoint) -> TransportResult<Vec<u16>> {
    let suffix = name.strip_prefix(r"\\.\pipe\").ok_or_else(|| {
        IpcTransportError::new(
            IpcTransportErrorCode::InvalidEndpoint,
            endpoint.to_string(),
            io::Error::new(
                io::ErrorKind::InvalidInput,
                r"named pipe must use the local \\.\pipe\ namespace",
            ),
        )
    })?;
    if suffix.is_empty()
        || suffix
            .chars()
            .any(|value| !(value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.')))
    {
        return Err(IpcTransportError::new(
            IpcTransportErrorCode::InvalidEndpoint,
            endpoint.to_string(),
            io::Error::new(io::ErrorKind::InvalidInput, "invalid named-pipe suffix"),
        ));
    }
    let encoded = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if encoded.len() > PIPE_NAME_MAX_UTF16 {
        return Err(IpcTransportError::new(
            IpcTransportErrorCode::InvalidEndpoint,
            endpoint.to_string(),
            io::Error::new(io::ErrorKind::InvalidInput, "named-pipe path is too long"),
        ));
    }
    Ok(encoded)
}

fn duration_ms(duration: Duration) -> u32 {
    duration.as_millis().clamp(1, u128::from(u32::MAX - 1)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(name: &str) -> IpcEndpoint {
        IpcEndpoint::NamedPipe(name.to_owned())
    }

    #[test]
    fn local_pipe_names_are_product_neutral() {
        for name in [
            r"\\.\pipe\agenterm-test-1",
            r"\\.\pipe\wbox.42_generation-1",
        ] {
            assert!(validated_pipe_name(name, &endpoint(name)).is_ok(), "{name}");
        }
    }

    #[test]
    fn pipe_names_reject_remote_nested_and_unsafe_namespaces() {
        for name in [
            r"\\server\pipe\agenterm-test",
            r"\\.\pipe\LOCAL\wbox-test",
            r"\\.\pipe\..\wbox-test",
            r"\\.\pipe\wbox:test",
            r"\\.\pipe\",
        ] {
            let error = validated_pipe_name(name, &endpoint(name)).unwrap_err();
            assert_eq!(error.code, IpcTransportErrorCode::InvalidEndpoint, "{name}");
        }
    }
}
