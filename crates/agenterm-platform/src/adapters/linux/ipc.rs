//! Linux/macOS local IPC adapter. Unix-specific APIs stay behind the platform
//! facade; the product endpoint and transport contracts remain target-neutral.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::{
    env,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    contract::ipc_transport::{
        IpcTransportError, IpcTransportErrorCode, TransportResult, map_bind_error, timeout_error,
        transport_io,
    },
    ipc::{IpcEndpoint, TrustedUserIdentity},
};

pub(crate) const NATIVE_TRANSPORT_NAME: &str = "unix";

fn effective_uid() -> u32 {
    crate::user_identity::current_user_identity()
        .expect("POSIX current-user identity is infallible")
        .posix_credentials()
        .expect("Unix IPC selected a POSIX identity")
        .effective_user_id
}

pub(crate) fn trusted_user_identity() -> io::Result<TrustedUserIdentity> {
    let uid = effective_uid();
    Ok(TrustedUserIdentity {
        kind: "uid",
        bytes: uid.to_le_bytes().to_vec(),
    })
}

pub(crate) fn native_runtime_directory() -> PathBuf {
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return path;
    }
    let short_system_temp = Path::new(std::path::MAIN_SEPARATOR_STR).join("tmp");
    let base = std::fs::canonicalize(&short_system_temp)
        .or_else(|_| std::fs::canonicalize(env::temp_dir()))
        .unwrap_or(short_system_temp);
    base.join(format!("agenterm-platform-{}", effective_uid()))
}

pub(crate) struct NativeListener {
    listener: std::os::unix::net::UnixListener,
    owned_path: PathBuf,
    device: u64,
    inode: u64,
    instance_lease: UnixInstanceLease,
    endpoint: IpcEndpoint,
}

impl NativeListener {
    pub(crate) fn bind(endpoint: &IpcEndpoint) -> TransportResult<Self> {
        use std::os::unix::{
            ffi::OsStrExt as _,
            fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _},
        };

        let IpcEndpoint::UnixSocket(path) = endpoint else {
            return Err(crate::contract::ipc_transport::unsupported(
                endpoint,
                "this host native IPC adapter accepts only Unix sockets",
            ));
        };
        let path = PathBuf::from(path);
        if !path.is_absolute() || path.as_os_str().as_bytes().len() > unix_socket_path_limit() {
            return Err(IpcTransportError::new(
                IpcTransportErrorCode::InvalidEndpoint,
                endpoint.to_string(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unix socket path must be absolute and within the platform length limit",
                ),
            ));
        }
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| {
                IpcTransportError::new(
                    IpcTransportErrorCode::InvalidEndpoint,
                    endpoint.to_string(),
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Unix socket has no parent directory",
                    ),
                )
            })?;
        ensure_private_directory(parent, endpoint)?;
        let mut instance_lease = UnixInstanceLease::acquire(&path, endpoint)?;

        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.uid() != effective_uid() || !metadata.file_type().is_socket() {
                    return Err(unsafe_endpoint(
                        endpoint,
                        "existing Unix endpoint is not an owned socket",
                    ));
                }
                match connect_bounded(
                    path.to_string_lossy().as_ref(),
                    Duration::from_millis(100),
                    endpoint,
                ) {
                    Ok(_) => {
                        return Err(IpcTransportError::new(
                            IpcTransportErrorCode::EndpointInUse,
                            endpoint.to_string(),
                            io::Error::new(
                                io::ErrorKind::AddrInUse,
                                "Unix endpoint has a live listener",
                            ),
                        ));
                    }
                    Err(error)
                        if matches!(
                            error.io_kind(),
                            io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                        ) =>
                    {
                        if !instance_lease.has_predecessor_identity {
                            return Err(unsafe_endpoint(
                                endpoint,
                                "stale Unix socket has no prior PID/start lease identity",
                            ));
                        }
                        let current = std::fs::symlink_metadata(&path)
                            .map_err(|error| transport_io(endpoint, error))?;
                        if !current.file_type().is_socket()
                            || current.uid() != effective_uid()
                            || current.dev() != metadata.dev()
                            || current.ino() != metadata.ino()
                        {
                            return Err(unsafe_endpoint(
                                endpoint,
                                "stale Unix socket identity changed during recovery",
                            ));
                        }
                        std::fs::remove_file(&path)
                            .map_err(|error| transport_io(endpoint, error))?;
                    }
                    Err(error) => return Err(unsafe_endpoint(endpoint, &error.to_string())),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(transport_io(endpoint, error)),
        }

        let listener = std::os::unix::net::UnixListener::bind(&path)
            .map_err(|error| map_bind_error(endpoint, error))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| transport_io(endpoint, error))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| transport_io(endpoint, error))?;
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|error| transport_io(endpoint, error))?;
        instance_lease.retain();
        Ok(Self {
            listener,
            owned_path: path,
            device: metadata.dev(),
            inode: metadata.ino(),
            instance_lease,
            endpoint: endpoint.clone(),
        })
    }

    pub(crate) fn accept(&mut self, timeout: Duration) -> TransportResult<NativeStream> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    return NativeStream::from_stream(stream, &self.endpoint, timeout);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(timeout_error(
                            IpcTransportErrorCode::AcceptTimeout,
                            &self.endpoint,
                        ));
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(2)));
                }
                Err(error) => return Err(transport_io(&self.endpoint, error)),
            }
        }
    }
}

impl Drop for NativeListener {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
        if let Ok(metadata) = std::fs::symlink_metadata(&self.owned_path)
            && metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && metadata.uid() == effective_uid()
            && std::fs::remove_file(&self.owned_path).is_ok()
        {
            self.instance_lease.remove_on_drop = true;
        }
    }
}

pub(crate) struct NativeStream(std::os::unix::net::UnixStream, IpcEndpoint);

impl AsRawFd for NativeStream {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsFd for NativeStream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for crate::ipc::NativeStream {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsFd for crate::ipc::NativeStream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

pub trait NativeStreamExt: Sized {
    fn from_owned_fd(
        descriptor: OwnedFd,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self>;
    fn into_owned_fd(self) -> OwnedFd;
}

impl NativeStreamExt for crate::ipc::NativeStream {
    fn from_owned_fd(
        descriptor: OwnedFd,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        NativeStream::from_owned_fd(descriptor, endpoint, timeout).map(Self)
    }

    fn into_owned_fd(self) -> OwnedFd {
        self.0.into_owned_fd()
    }
}

impl NativeStream {
    pub(crate) fn from_owned_fd(
        descriptor: OwnedFd,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        Self::from_stream(descriptor.into(), endpoint, timeout)
    }

    pub(crate) fn into_owned_fd(self) -> OwnedFd {
        self.0.into()
    }

    pub(crate) fn connect(endpoint: &IpcEndpoint, timeout: Duration) -> TransportResult<Self> {
        let IpcEndpoint::UnixSocket(path) = endpoint else {
            return Err(crate::contract::ipc_transport::unsupported(
                endpoint,
                "this host native IPC adapter accepts only Unix sockets",
            ));
        };
        let stream = connect_bounded(path, timeout, endpoint)?;
        Self::from_stream(stream, endpoint, timeout)
    }

    fn from_stream(
        stream: std::os::unix::net::UnixStream,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        verify_peer_uid(&stream, endpoint)?;
        stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(timeout)))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| transport_io(endpoint, error))?;
        Ok(Self(stream, endpoint.clone()))
    }

    pub(crate) fn set_io_timeout(&mut self, timeout: Duration) -> TransportResult<()> {
        self.0
            .set_read_timeout(Some(timeout))
            .and_then(|()| self.0.set_write_timeout(Some(timeout)))
            .map_err(|error| transport_io(&self.1, error))
    }
}

impl Read for NativeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for NativeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

struct UnixInstanceLease {
    file: std::fs::File,
    has_predecessor_identity: bool,
    owned_path: PathBuf,
    device: u64,
    inode: u64,
    remove_on_drop: bool,
}

impl UnixInstanceLease {
    fn acquire(path: &Path, endpoint: &IpcEndpoint) -> TransportResult<Self> {
        use std::{
            io::{Read as _, Seek as _, SeekFrom, Write as _},
            os::{
                fd::{FromRawFd as _, OwnedFd},
                unix::ffi::OsStrExt as _,
            },
        };
        const LEASE_MAX_BYTES: u64 = 256;
        let mut lock_name = path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let c_path = std::ffi::CString::new(lock_path.as_os_str().as_bytes()).map_err(|_| {
            IpcTransportError::new(
                IpcTransportErrorCode::InvalidEndpoint,
                endpoint.to_string(),
                io::Error::new(io::ErrorKind::InvalidInput, "Unix lock path contains NUL"),
            )
        })?;
        let create_flags =
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        let mut created = true;
        let mut raw = unsafe { libc::open(c_path.as_ptr(), create_flags, 0o600) };
        if raw < 0 && io::Error::last_os_error().kind() == io::ErrorKind::AlreadyExists {
            created = false;
            raw = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
        }
        if raw < 0 {
            return Err(unsafe_endpoint(
                endpoint,
                &format!(
                    "cannot securely open Unix instance lock: {}",
                    io::Error::last_os_error()
                ),
            ));
        }
        let owned = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe { libc::fstat(raw, &mut stat) } != 0 {
            return Err(transport_io(endpoint, io::Error::last_os_error()));
        }
        if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG
            || stat.st_uid != effective_uid()
            || stat.st_nlink != 1
            || stat.st_mode & 0o077 != 0
        {
            return Err(unsafe_endpoint(
                endpoint,
                "Unix instance lock is not a private, owned regular file",
            ));
        }
        if created && unsafe { libc::fchmod(raw, 0o600) } != 0 {
            return Err(transport_io(endpoint, io::Error::last_os_error()));
        }
        if unsafe { libc::flock(raw, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            let code = if matches!(error.raw_os_error(), Some(value) if value == libc::EWOULDBLOCK || value == libc::EAGAIN)
            {
                IpcTransportErrorCode::EndpointInUse
            } else {
                IpcTransportErrorCode::Io
            };
            return Err(IpcTransportError::new(code, endpoint.to_string(), error));
        }
        let mut file = std::fs::File::from(owned);
        let has_predecessor_identity = if created {
            false
        } else {
            let metadata = file
                .metadata()
                .map_err(|error| transport_io(endpoint, error))?;
            if metadata.len() > LEASE_MAX_BYTES {
                return Err(unsafe_endpoint(
                    endpoint,
                    "Unix instance lock identity is oversized",
                ));
            }
            let mut prior = String::new();
            file.read_to_string(&mut prior)
                .map_err(|error| transport_io(endpoint, error))?;
            if !valid_lease_identity(prior.trim()) {
                return Err(unsafe_endpoint(
                    endpoint,
                    "Unix instance lock has no valid PID/start identity",
                ));
            }
            true
        };
        let identity = current_process_identity(endpoint)?;
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| file.write_all(identity.as_bytes()))
            .and_then(|()| file.sync_data())
            .map_err(|error| transport_io(endpoint, error))?;
        Ok(Self {
            file,
            has_predecessor_identity,
            owned_path: lock_path,
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
            remove_on_drop: created,
        })
    }

    fn retain(&mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for UnixInstanceLease {
    fn drop(&mut self) {
        use std::os::{fd::AsRawFd as _, unix::fs::MetadataExt as _};
        if !self.remove_on_drop {
            return;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&self.owned_path)
            && metadata.file_type().is_file()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && metadata.uid() == effective_uid()
        {
            let _ = std::fs::remove_file(&self.owned_path);
        }
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn valid_lease_identity(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("v1 pid=") else {
        return false;
    };
    let Some((pid, start)) = rest.split_once(" start=") else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !start.is_empty()
        && start
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(target_os = "linux")]
fn current_process_identity(endpoint: &IpcEndpoint) -> TransportResult<String> {
    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|error| transport_io(endpoint, error))?;
    let suffix = stat
        .rsplit_once(") ")
        .map(|(_, suffix)| suffix)
        .ok_or_else(|| unsafe_endpoint(endpoint, "cannot parse Linux process start identity"))?;
    let start = suffix
        .split_whitespace()
        .nth(19)
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| unsafe_endpoint(endpoint, "cannot parse Linux process start identity"))?;
    Ok(format!("v1 pid={} start={start}", std::process::id()))
}

#[cfg(target_os = "macos")]
fn current_process_identity(endpoint: &IpcEndpoint) -> TransportResult<String> {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size {
        return Err(unsafe_endpoint(
            endpoint,
            "cannot read macOS process start identity",
        ));
    }
    Ok(format!(
        "v1 pid={} start={}.{}",
        std::process::id(),
        info.pbi_start_tvsec,
        info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "linux")]
fn verify_peer_uid(
    stream: &std::os::unix::net::UnixStream,
    endpoint: &IpcEndpoint,
) -> TransportResult<()> {
    use std::os::fd::AsRawFd as _;
    let mut credentials = unsafe { std::mem::zeroed::<libc::ucred>() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &mut length,
        )
    } != 0
        || length as usize != std::mem::size_of::<libc::ucred>()
    {
        return Err(unsafe_endpoint(
            endpoint,
            "cannot verify Linux Unix-socket peer credentials",
        ));
    }
    if credentials.uid != effective_uid() {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix-socket peer effective UID does not match the server scope",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_peer_uid(
    stream: &std::os::unix::net::UnixStream,
    endpoint: &IpcEndpoint,
) -> TransportResult<()> {
    use std::os::fd::AsRawFd as _;
    let mut uid = 0;
    let mut gid = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(unsafe_endpoint(
            endpoint,
            "cannot verify macOS Unix-socket peer credentials",
        ));
    }
    if uid != effective_uid() {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix-socket peer effective UID does not match the server scope",
        ));
    }
    Ok(())
}

fn ensure_private_directory(directory: &Path, endpoint: &IpcEndpoint) -> TransportResult<()> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    if !directory.is_absolute() {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix runtime directory must be absolute",
        ));
    }
    let mut ancestry = directory.ancestors().collect::<Vec<_>>();
    ancestry.reverse();
    for component in ancestry {
        match std::fs::symlink_metadata(component) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(unsafe_endpoint(
                        endpoint,
                        "Unix runtime directory ancestry contains a symlink or non-directory",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = std::fs::DirBuilder::new();
                builder.mode(0o700);
                match builder.create(component) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = std::fs::symlink_metadata(component)
                            .map_err(|error| transport_io(endpoint, error))?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(unsafe_endpoint(
                                endpoint,
                                "Unix runtime directory creation lost an ancestry race",
                            ));
                        }
                    }
                    Err(error) => return Err(transport_io(endpoint, error)),
                }
            }
            Err(error) => return Err(transport_io(endpoint, error)),
        }
    }
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|error| transport_io(endpoint, error))?;
    if !metadata.is_dir() || metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0 {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix runtime directory is not private to the effective UID",
        ));
    }
    Ok(())
}

fn unix_socket_path_limit() -> usize {
    103
}

fn connect_bounded(
    path: &str,
    timeout: Duration,
    endpoint: &IpcEndpoint,
) -> TransportResult<std::os::unix::net::UnixStream> {
    use std::os::{
        fd::{FromRawFd as _, IntoRawFd as _, OwnedFd},
        unix::ffi::OsStrExt as _,
    };
    let bytes = std::ffi::OsStr::new(path).as_bytes();
    if !Path::new(path).is_absolute()
        || bytes.is_empty()
        || bytes.len() > unix_socket_path_limit()
        || bytes.contains(&0)
    {
        return Err(IpcTransportError::new(
            IpcTransportErrorCode::InvalidEndpoint,
            endpoint.to_string(),
            io::Error::new(io::ErrorKind::InvalidInput, "invalid Unix socket path"),
        ));
    }
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(transport_io(endpoint, io::Error::last_os_error()));
    }
    let owned = unsafe { OwnedFd::from_raw_fd(raw) };
    let descriptor_flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    let status_flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    if descriptor_flags < 0
        || status_flags < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFD, descriptor_flags | libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFL, status_flags | libc::O_NONBLOCK) } < 0
    {
        return Err(transport_io(endpoint, io::Error::last_os_error()));
    }
    let mut address = unsafe { std::mem::zeroed::<libc::sockaddr_un>() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (destination, source) in address.sun_path.iter_mut().zip(bytes.iter().copied()) {
        *destination = source as libc::c_char;
    }
    let address_length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    #[cfg(target_vendor = "apple")]
    {
        address.sun_len = address_length as u8;
    }
    let connected = unsafe {
        libc::connect(
            raw,
            (&raw const address).cast(),
            address_length as libc::socklen_t,
        )
    };
    if connected != 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(code) if code == libc::EINPROGRESS || code == libc::EWOULDBLOCK)
        {
            return Err(transport_io(endpoint, error));
        }
        let mut descriptor = libc::pollfd {
            fd: raw,
            events: libc::POLLOUT,
            revents: 0,
        };
        let timeout_ms = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if ready == 0 {
            return Err(timeout_error(
                IpcTransportErrorCode::ConnectTimeout,
                endpoint,
            ));
        }
        if ready < 0 {
            return Err(transport_io(endpoint, io::Error::last_os_error()));
        }
        let mut socket_error = 0;
        let mut socket_error_length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                raw,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&raw mut socket_error).cast(),
                &mut socket_error_length,
            )
        } != 0
        {
            return Err(transport_io(endpoint, io::Error::last_os_error()));
        }
        if socket_error != 0 {
            return Err(transport_io(
                endpoint,
                io::Error::from_raw_os_error(socket_error),
            ));
        }
    }
    if unsafe { libc::fcntl(raw, libc::F_SETFL, status_flags) } < 0 {
        return Err(transport_io(endpoint, io::Error::last_os_error()));
    }
    let raw = owned.into_raw_fd();
    Ok(unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw) })
}

fn unsafe_endpoint(endpoint: &IpcEndpoint, message: &str) -> IpcTransportError {
    IpcTransportError::new(
        IpcTransportErrorCode::UnsafeEndpoint,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::PermissionDenied, message.to_owned()),
    )
}
