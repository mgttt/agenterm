use std::{
    fmt,
    io::{self, BufRead as _, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};

use crate::ipc_endpoint::IpcEndpoint;

// UI command completion embeds a bounded 1 MiB `IpcResponse` JSON document
// inside the outer request string. JSON re-escaping can approach 2x, so the
// transport frame must be larger than the operation-argument budget while
// remaining explicitly bounded.
pub(crate) const IPC_REQUEST_MAX_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const IPC_RESPONSE_MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IpcTransportErrorCode {
    UnsupportedEndpoint,
    InvalidEndpoint,
    EndpointInUse,
    #[cfg(unix)]
    UnsafeEndpoint,
    ConnectTimeout,
    AcceptTimeout,
    Io,
}

#[derive(Debug)]
pub(crate) struct IpcTransportError {
    pub(crate) code: IpcTransportErrorCode,
    pub(crate) endpoint: String,
    source: io::Error,
}

impl IpcTransportError {
    fn new(
        code: IpcTransportErrorCode,
        endpoint: impl Into<String>,
        source: impl Into<io::Error>,
    ) -> Self {
        Self {
            code,
            endpoint: endpoint.into(),
            source: source.into(),
        }
    }

    #[cfg(any(unix, test))]
    pub(crate) fn io_kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

impl fmt::Display for IpcTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "IPC transport {:?} for {}: {}",
            self.code, self.endpoint, self.source
        )
    }
}

impl std::error::Error for IpcTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

type TransportResult<T> = std::result::Result<T, IpcTransportError>;

/// A transport-neutral local IPC listener. This adapter is intentionally
/// staged before the caller migration so TCP and native endpoints can coexist.
pub(crate) struct IpcListener {
    endpoint: IpcEndpoint,
    inner: ListenerInner,
}

enum ListenerInner {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(UnixListener),
    #[cfg(windows)]
    NamedPipe(windows_pipe::NamedPipeListener),
}

impl IpcListener {
    pub(crate) fn bind(endpoint: &IpcEndpoint) -> TransportResult<Self> {
        let inner = match endpoint {
            IpcEndpoint::Tcp { host, port } => {
                let address = resolve_one(host, *port, endpoint)?;
                let listener =
                    TcpListener::bind(address).map_err(|error| map_bind_error(endpoint, error))?;
                listener
                    .set_nonblocking(true)
                    .map_err(|error| transport_io(endpoint, error))?;
                ListenerInner::Tcp(listener)
            }
            #[cfg(unix)]
            IpcEndpoint::UnixSocket(path) => {
                ListenerInner::Unix(UnixListener::bind(path, endpoint)?)
            }
            #[cfg(not(unix))]
            IpcEndpoint::UnixSocket(_) => {
                return Err(unsupported(endpoint, "Unix sockets are unavailable"));
            }
            #[cfg(windows)]
            IpcEndpoint::NamedPipe(name) => {
                ListenerInner::NamedPipe(windows_pipe::NamedPipeListener::bind(name, endpoint)?)
            }
            #[cfg(not(windows))]
            IpcEndpoint::NamedPipe(_) => {
                return Err(unsupported(endpoint, "Windows named pipes are unavailable"));
            }
        };
        Ok(Self {
            endpoint: endpoint.clone(),
            inner,
        })
    }

    pub(crate) fn accept(&mut self, timeout: Duration) -> TransportResult<IpcStream> {
        if timeout.is_zero() {
            return Err(timeout_error(
                IpcTransportErrorCode::AcceptTimeout,
                &self.endpoint,
            ));
        }
        match &mut self.inner {
            ListenerInner::Tcp(listener) => {
                let stream = poll_accept(timeout, &self.endpoint, || {
                    listener.accept().map(|(stream, _)| stream)
                })?;
                IpcStream::from_tcp(stream, &self.endpoint, timeout)
            }
            #[cfg(unix)]
            ListenerInner::Unix(listener) => {
                let stream = poll_accept(timeout, &self.endpoint, || {
                    listener.listener.accept().map(|(stream, _)| stream)
                })?;
                IpcStream::from_unix(stream, &self.endpoint, timeout)
            }
            #[cfg(windows)]
            ListenerInner::NamedPipe(listener) => listener.accept(timeout),
        }
    }
}

/// A byte-stream IPC connection. Newline-delimited JSON framing remains owned
/// by the existing protocol layer.
pub(crate) struct IpcStream {
    endpoint: IpcEndpoint,
    inner: StreamInner,
}

enum StreamInner {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    NamedPipe(windows_pipe::NamedPipeStream),
}

impl IpcStream {
    pub(crate) fn connect(endpoint: &IpcEndpoint, timeout: Duration) -> TransportResult<Self> {
        if timeout.is_zero() {
            return Err(timeout_error(
                IpcTransportErrorCode::ConnectTimeout,
                endpoint,
            ));
        }
        match endpoint {
            IpcEndpoint::Tcp { host, port } => {
                let address = resolve_one(host, *port, endpoint)?;
                let stream = TcpStream::connect_timeout(&address, timeout).map_err(|error| {
                    let code = if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) {
                        IpcTransportErrorCode::ConnectTimeout
                    } else {
                        IpcTransportErrorCode::Io
                    };
                    IpcTransportError::new(code, endpoint.to_string(), error)
                })?;
                Self::from_tcp(stream, endpoint, timeout)
            }
            #[cfg(unix)]
            IpcEndpoint::UnixSocket(path) => {
                let stream = connect_unix_bounded(path, timeout, endpoint)?;
                Self::from_unix(stream, endpoint, timeout)
            }
            #[cfg(not(unix))]
            IpcEndpoint::UnixSocket(_) => {
                Err(unsupported(endpoint, "Unix sockets are unavailable"))
            }
            #[cfg(windows)]
            IpcEndpoint::NamedPipe(name) => {
                windows_pipe::NamedPipeStream::connect(name, endpoint, timeout)
            }
            #[cfg(not(windows))]
            IpcEndpoint::NamedPipe(_) => {
                Err(unsupported(endpoint, "Windows named pipes are unavailable"))
            }
        }
    }

    fn from_tcp(
        stream: TcpStream,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        stream
            // Windows accepted sockets inherit the listener's nonblocking
            // mode. Return every connected stream to blocking mode before
            // applying bounded read/write timeouts, otherwise an ordinary
            // scheduling gap surfaces as WSAEWOULDBLOCK.
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(timeout)))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| transport_io(endpoint, error))?;
        Ok(Self {
            endpoint: endpoint.clone(),
            inner: StreamInner::Tcp(stream),
        })
    }

    #[cfg(unix)]
    fn from_unix(
        stream: std::os::unix::net::UnixStream,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        // Validate both accepted clients and connected servers. The same
        // constructor owns both paths, preventing a future caller from
        // accidentally bypassing the OS peer-credential boundary.
        verify_unix_peer_uid(&stream, endpoint)?;
        stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(timeout)))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| transport_io(endpoint, error))?;
        Ok(Self {
            endpoint: endpoint.clone(),
            inner: StreamInner::Unix(stream),
        })
    }

    pub(crate) fn set_io_timeout(&mut self, timeout: Duration) -> TransportResult<()> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream
                .set_read_timeout(Some(timeout))
                .and_then(|()| stream.set_write_timeout(Some(timeout)))
                .map_err(|error| transport_io(&self.endpoint, error)),
            #[cfg(unix)]
            StreamInner::Unix(stream) => stream
                .set_read_timeout(Some(timeout))
                .and_then(|()| stream.set_write_timeout(Some(timeout)))
                .map_err(|error| transport_io(&self.endpoint, error)),
            #[cfg(windows)]
            StreamInner::NamedPipe(stream) => {
                stream.set_timeout(timeout);
                Ok(())
            }
        }
    }
}

impl Read for IpcStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.read(buffer),
            #[cfg(unix)]
            StreamInner::Unix(stream) => stream.read(buffer),
            #[cfg(windows)]
            StreamInner::NamedPipe(stream) => stream.read(buffer),
        }
    }
}

impl Write for IpcStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.write(buffer),
            #[cfg(unix)]
            StreamInner::Unix(stream) => stream.write(buffer),
            #[cfg(windows)]
            StreamInner::NamedPipe(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            StreamInner::Unix(stream) => stream.flush(),
            #[cfg(windows)]
            StreamInner::NamedPipe(stream) => stream.flush(),
        }
    }
}

fn resolve_one(host: &str, port: u16, endpoint: &IpcEndpoint) -> TransportResult<SocketAddr> {
    use std::net::ToSocketAddrs as _;

    (host, port)
        .to_socket_addrs()
        .map_err(|error| transport_io(endpoint, error))?
        .next()
        .ok_or_else(|| {
            IpcTransportError::new(
                IpcTransportErrorCode::InvalidEndpoint,
                endpoint.to_string(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "endpoint resolved no addresses",
                ),
            )
        })
}

fn poll_accept<T>(
    timeout: Duration,
    endpoint: &IpcEndpoint,
    mut accept: impl FnMut() -> io::Result<T>,
) -> TransportResult<T> {
    let deadline = Instant::now() + timeout;
    loop {
        match accept() {
            Ok(value) => return Ok(value),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(timeout_error(
                        IpcTransportErrorCode::AcceptTimeout,
                        endpoint,
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(2)));
            }
            Err(error) => return Err(transport_io(endpoint, error)),
        }
    }
}

fn transport_io(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    IpcTransportError::new(IpcTransportErrorCode::Io, endpoint.to_string(), error)
}

fn map_bind_error(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    let code = if error.kind() == io::ErrorKind::AddrInUse {
        IpcTransportErrorCode::EndpointInUse
    } else {
        IpcTransportErrorCode::Io
    };
    IpcTransportError::new(code, endpoint.to_string(), error)
}

fn timeout_error(code: IpcTransportErrorCode, endpoint: &IpcEndpoint) -> IpcTransportError {
    IpcTransportError::new(
        code,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::TimedOut, "bounded IPC operation timed out"),
    )
}

fn unsupported(endpoint: &IpcEndpoint, message: &str) -> IpcTransportError {
    IpcTransportError::new(
        IpcTransportErrorCode::UnsupportedEndpoint,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::Unsupported, message),
    )
}

#[cfg(unix)]
struct UnixListener {
    listener: std::os::unix::net::UnixListener,
    owned_path: std::path::PathBuf,
    device: u64,
    inode: u64,
    // Keep the advisory lock alive until after `Drop` has removed the exact
    // socket inode owned by this listener.
    instance_lease: UnixInstanceLease,
}

#[cfg(unix)]
impl UnixListener {
    fn bind(path: &str, endpoint: &IpcEndpoint) -> TransportResult<Self> {
        use std::os::unix::{
            ffi::OsStrExt as _,
            fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _},
        };

        let path = std::path::PathBuf::from(path);
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
        ensure_private_unix_directory(parent, endpoint)?;
        let mut instance_lease = UnixInstanceLease::acquire(&path, endpoint)?;

        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.uid() != unsafe { libc::geteuid() } || !metadata.file_type().is_socket()
                {
                    return Err(unsafe_endpoint(
                        endpoint,
                        "existing Unix endpoint is not an owned socket",
                    ));
                }
                match connect_unix_bounded(
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
                            || current.uid() != unsafe { libc::geteuid() }
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
        })
    }
}

#[cfg(unix)]
impl Drop for UnixListener {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
        if let Ok(metadata) = std::fs::symlink_metadata(&self.owned_path)
            && metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && metadata.uid() == unsafe { libc::geteuid() }
            && std::fs::remove_file(&self.owned_path).is_ok()
        {
            self.instance_lease.remove_on_drop = true;
        }
    }
}

#[cfg(unix)]
struct UnixInstanceLease {
    file: std::fs::File,
    has_predecessor_identity: bool,
    owned_path: std::path::PathBuf,
    device: u64,
    inode: u64,
    remove_on_drop: bool,
}

#[cfg(unix)]
impl UnixInstanceLease {
    fn acquire(path: &std::path::Path, endpoint: &IpcEndpoint) -> TransportResult<Self> {
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
        let lock_path = std::path::PathBuf::from(lock_name);
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
            || stat.st_uid != unsafe { libc::geteuid() }
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
            let code = if matches!(
                error.raw_os_error(),
                Some(value) if value == libc::EWOULDBLOCK || value == libc::EAGAIN
            ) {
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
            if !valid_unix_lease_identity(prior.trim()) {
                return Err(unsafe_endpoint(
                    endpoint,
                    "Unix instance lock has no valid PID/start identity",
                ));
            }
            true
        };

        let identity = current_unix_process_identity(endpoint)?;
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

#[cfg(unix)]
impl Drop for UnixInstanceLease {
    fn drop(&mut self) {
        use std::os::{fd::AsRawFd as _, unix::fs::MetadataExt as _};

        if !self.remove_on_drop {
            return;
        }
        // The descriptor remains locked while this exact no-follow identity
        // check and cleanup run. Never remove a replacement path.
        if let Ok(metadata) = std::fs::symlink_metadata(&self.owned_path)
            && metadata.file_type().is_file()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && metadata.uid() == unsafe { libc::geteuid() }
        {
            let _ = std::fs::remove_file(&self.owned_path);
        }
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(unix)]
fn valid_unix_lease_identity(value: &str) -> bool {
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
fn current_unix_process_identity(endpoint: &IpcEndpoint) -> TransportResult<String> {
    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|error| transport_io(endpoint, error))?;
    // The command name is parenthesized and may contain spaces or `)`, so
    // locate its final delimiter before indexing field 22 (starttime).
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
fn current_unix_process_identity(endpoint: &IpcEndpoint) -> TransportResult<String> {
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

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn current_unix_process_identity(endpoint: &IpcEndpoint) -> TransportResult<String> {
    Err(unsupported(
        endpoint,
        "native Unix IPC identity is supported on Linux and macOS",
    ))
}

#[cfg(target_os = "linux")]
fn verify_unix_peer_uid(
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
    if credentials.uid != unsafe { libc::geteuid() } {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix-socket peer effective UID does not match the server scope",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_unix_peer_uid(
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
    if uid != unsafe { libc::geteuid() } {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix-socket peer effective UID does not match the server scope",
        ));
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn verify_unix_peer_uid(
    _stream: &std::os::unix::net::UnixStream,
    endpoint: &IpcEndpoint,
) -> TransportResult<()> {
    Err(unsupported(
        endpoint,
        "Unix peer identity is supported on Linux and macOS",
    ))
}

#[cfg(unix)]
fn ensure_private_unix_directory(
    directory: &std::path::Path,
    endpoint: &IpcEndpoint,
) -> TransportResult<()> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};

    if !directory.is_absolute() {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix runtime directory must be absolute",
        ));
    }

    // Validate the complete existing ancestry with no-follow metadata. A
    // symlink above the leaf must not redirect a privileged socket bind.
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

    // Existing explicit parents are validation-only. Only directories created
    // above are assigned mode 0700; never repair/chmod user-supplied paths.
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|error| transport_io(endpoint, error))?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(unsafe_endpoint(
            endpoint,
            "Unix runtime directory is not private to the effective UID",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn unix_socket_path_limit() -> usize {
    // macOS sockaddr_un::sun_path is 104 bytes and Linux is 108. Reserving
    // space for the terminator keeps one portable derivation.
    103
}

#[cfg(unix)]
fn connect_unix_bounded(
    path: &str,
    timeout: Duration,
    endpoint: &IpcEndpoint,
) -> TransportResult<std::os::unix::net::UnixStream> {
    use std::os::{
        fd::{FromRawFd as _, IntoRawFd as _, OwnedFd},
        unix::ffi::OsStrExt as _,
    };

    let bytes = std::ffi::OsStr::new(path).as_bytes();
    if !std::path::Path::new(path).is_absolute()
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
        if !matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EINPROGRESS || code == libc::EWOULDBLOCK
        ) {
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

#[cfg(unix)]
fn unsafe_endpoint(endpoint: &IpcEndpoint, message: &str) -> IpcTransportError {
    IpcTransportError::new(
        IpcTransportErrorCode::UnsafeEndpoint,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::PermissionDenied, message.to_owned()),
    )
}

#[cfg(windows)]
mod windows_pipe {
    use std::{
        ffi::OsStr,
        io::{self, Read, Write},
        mem::size_of,
        os::windows::{
            ffi::OsStrExt as _,
            io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
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
            FILE_GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
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
        ipc_endpoint::{IpcEndpoint, current_user_sid_bytes},
        ipc_transport::{
            IpcStream, IpcTransportError, IpcTransportErrorCode, StreamInner, TransportResult,
            timeout_error, transport_io,
        },
    };

    const PIPE_BUFFER_BYTES: u32 = 256 * 1024;
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    const PIPE_NAME_MAX_UTF16: usize = 256;

    pub(super) struct NamedPipeListener {
        name: Vec<u16>,
        endpoint: IpcEndpoint,
        security: PipeSecurity,
        pending: Option<OwnedHandle>,
    }

    impl NamedPipeListener {
        pub(super) fn bind(name: &str, endpoint: &IpcEndpoint) -> TransportResult<Self> {
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

        pub(super) fn accept(&mut self, timeout: Duration) -> TransportResult<IpcStream> {
            let pipe = match self.pending.take() {
                Some(pipe) => pipe,
                None => create_pipe(&self.name, false, &mut self.security, &self.endpoint)?,
            };
            match connect_pipe_instance(&pipe, timeout, &self.endpoint) {
                Ok(()) => {
                    // Keep the pipe namespace continuously owned while the
                    // accepted stream is handed to another thread.
                    self.pending = Some(create_pipe(
                        &self.name,
                        false,
                        &mut self.security,
                        &self.endpoint,
                    )?);
                    Ok(IpcStream {
                        endpoint: self.endpoint.clone(),
                        inner: StreamInner::NamedPipe(NamedPipeStream {
                            handle: pipe,
                            timeout,
                        }),
                    })
                }
                Err(error) => {
                    // CancelIoEx has completed before this point. Reset and
                    // retain the same first instance so a timeout never opens
                    // a namespace-claim race.
                    unsafe {
                        DisconnectNamedPipe(pipe.as_raw_handle());
                    }
                    self.pending = Some(pipe);
                    Err(error)
                }
            }
        }
    }

    pub(super) struct NamedPipeStream {
        handle: OwnedHandle,
        timeout: Duration,
    }

    impl NamedPipeStream {
        pub(super) fn connect(
            name: &str,
            endpoint: &IpcEndpoint,
            timeout: Duration,
        ) -> TransportResult<IpcStream> {
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
                    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
                    return Ok(IpcStream {
                        endpoint: endpoint.clone(),
                        inner: StreamInner::NamedPipe(Self { handle, timeout }),
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

        fn raw_handle(&self) -> HANDLE {
            self.handle.as_raw_handle()
        }

        pub(super) fn set_timeout(&mut self, timeout: Duration) {
            self.timeout = timeout;
        }
    }

    impl Read for NamedPipeStream {
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

    impl Write for NamedPipeStream {
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
            // FlushFileBuffers may wait indefinitely for a peer read. A
            // completed overlapped WriteFile is the bounded flush boundary.
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
                && matches!(
                    raw_error,
                    Some(value) if value == ERROR_ACCESS_DENIED || value == ERROR_PIPE_BUSY
                ) {
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
            let sid = current_user_sid_bytes().map_err(|error| transport_io(endpoint, error))?;
            let sid_words = sid.len().div_ceil(size_of::<usize>());
            let mut aligned_sid = vec![0usize; sid_words];
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
        let suffix = name.strip_prefix(r"\\.\pipe\agenterm-").ok_or_else(|| {
            IpcTransportError::new(
                IpcTransportErrorCode::InvalidEndpoint,
                endpoint.to_string(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "named pipe must use the local AgenTerm namespace",
                ),
            )
        })?;
        if suffix.is_empty()
            || suffix
                .chars()
                .any(|value| !(value.is_ascii_alphanumeric() || matches!(value, '-' | '_')))
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
}

pub(crate) fn read_bounded_ipc_line(
    reader: &mut impl std::io::BufRead,
    max_bytes: u64,
    label: &str,
) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(max_bytes + 1)
        .read_until(b'\n', &mut bytes)
        .with_context(|| format!("could not read {label}"))?;
    if bytes.is_empty() {
        anyhow::bail!("{label} connection closed before a message arrived");
    }
    if bytes.len() as u64 > max_bytes {
        anyhow::bail!("{label} exceeded the {max_bytes}-byte limit");
    }
    if bytes.last() != Some(&b'\n') {
        anyhow::bail!("{label} was not newline terminated");
    }
    String::from_utf8(bytes).with_context(|| format!("{label} was not valid UTF-8"))
}

mod server {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc::{self, Sender, SyncSender},
        },
        thread::{self, JoinHandle},
        time::Duration,
    };

    use std::io::Write as _;

    use anyhow::{Context as _, Result};

    use crate::{
        IPC_TIMEOUT, client,
        protocol::{IpcRequest, IpcResponse},
        request_gui_wake,
        wake_signal::WakeSignal,
    };

    use super::{IPC_REQUEST_MAX_BYTES, read_bounded_ipc_line};

    const IPC_MAX_CONCURRENT_CONNECTIONS: usize = 32;
    const IPC_MAX_PENDING_REQUESTS: usize = 64;

    pub(crate) struct IpcEnvelope {
        pub(crate) request: IpcRequest,
        pub(crate) respond_to: Sender<IpcResponse>,
    }

    pub(crate) struct IpcServer {
        receiver: mpsc::Receiver<IpcEnvelope>,
        stop: Arc<AtomicBool>,
        listener_thread: Option<JoinHandle<()>>,
    }

    impl IpcServer {
        pub(crate) fn try_iter(&self) -> mpsc::TryIter<'_, IpcEnvelope> {
            self.receiver.try_iter()
        }

        #[cfg(unix)]
        pub(crate) fn try_recv(&self) -> Result<IpcEnvelope, mpsc::TryRecvError> {
            self.receiver.try_recv()
        }
    }

    impl Drop for IpcServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(listener_thread) = self.listener_thread.take() {
                // Every listener accept is bounded to 250 ms, so joining here
                // gives its owned Unix socket and lease a deterministic,
                // identity-checked Drop before the server process exits.
                let _ = listener_thread.join();
            }
        }
    }

    pub(crate) fn start_ipc_server(
        window: isize,
        wake_signal: Arc<WakeSignal>,
    ) -> Result<IpcServer> {
        let endpoint = client::ipc_endpoint()?;
        let listener = super::IpcListener::bind(&endpoint)
            .map_err(anyhow::Error::new)
            .context("failed to bind the selected AgenTerm IPC endpoint")?;
        Ok(spawn_ipc_server(listener, window, wake_signal))
    }

    pub(super) fn spawn_ipc_server(
        mut listener: super::IpcListener,
        wake_window: isize,
        wake_signal: Arc<WakeSignal>,
    ) -> IpcServer {
        let (sender, receiver) = mpsc::sync_channel(IPC_MAX_PENDING_REQUESTS);
        let stop = Arc::new(AtomicBool::new(false));
        let listener_stop = Arc::clone(&stop);
        let listener_thread = thread::spawn(move || {
            let active_connections = Arc::new(AtomicUsize::new(0));
            while !listener_stop.load(Ordering::Acquire) {
                let connection = match listener.accept(Duration::from_millis(250)) {
                    Ok(connection) => connection,
                    Err(error) if error.code == super::IpcTransportErrorCode::AcceptTimeout => {
                        continue;
                    }
                    Err(_) => continue,
                };
                let admitted = active_connections
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                        (active < IPC_MAX_CONCURRENT_CONNECTIONS).then_some(active + 1)
                    })
                    .is_ok();
                if !admitted {
                    continue;
                }
                let sender = sender.clone();
                let wake_signal = Arc::clone(&wake_signal);
                let permit = IpcConnectionPermit(Arc::clone(&active_connections));
                thread::spawn(move || {
                    let _permit = permit;
                    handle_ipc_connection(connection, &sender, wake_window, &wake_signal);
                });
            }
        });
        IpcServer {
            receiver,
            stop,
            listener_thread: Some(listener_thread),
        }
    }

    struct IpcConnectionPermit(Arc<AtomicUsize>);

    impl Drop for IpcConnectionPermit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn handle_ipc_connection(
        connection: super::IpcStream,
        sender: &SyncSender<IpcEnvelope>,
        wake_window: isize,
        wake_signal: &WakeSignal,
    ) {
        let mut reader = std::io::BufReader::new(connection);
        let response =
            match read_bounded_ipc_line(&mut reader, IPC_REQUEST_MAX_BYTES, "AgenTerm IPC request")
            {
                Ok(line) => match serde_json::from_str::<IpcRequest>(&line) {
                    Ok(request) => {
                        let (response_sender, response_receiver) = mpsc::channel();
                        if sender
                            .try_send(IpcEnvelope {
                                request,
                                respond_to: response_sender,
                            })
                            .is_err()
                        {
                            IpcResponse::typed_failure(
                                "AgenTerm IPC mailbox is unavailable or full",
                                "ipc_mailbox_unavailable",
                                "availability",
                                true,
                            )
                        } else {
                            request_gui_wake(wake_window, wake_signal);
                            response_receiver
                                .recv_timeout(IPC_TIMEOUT)
                                .unwrap_or_else(|_| {
                                    IpcResponse::failure("AgenTerm GUI did not process the command")
                                })
                        }
                    }
                    Err(error) => IpcResponse::failure(format!("invalid IPC request: {error}")),
                },
                Err(error) => IpcResponse::failure(format!("{error:#}")),
            };
        if let Ok(serialized) = serde_json::to_string(&response) {
            let connection = reader.get_mut();
            let _ = connection.write_all(serialized.as_bytes());
            let _ = connection.write_all(b"\n");
            let _ = connection.flush();
        }
    }
}

#[cfg(unix)]
pub(crate) use server::IpcEnvelope;
pub(crate) use server::{IpcServer, start_ipc_server};

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        time::Duration,
    };

    use super::{
        IPC_REQUEST_MAX_BYTES, IpcEndpoint, IpcListener, IpcStream, IpcTransportErrorCode,
        read_bounded_ipc_line,
    };

    #[test]
    fn ipc_lines_require_a_bounded_newline_terminated_utf8_message() {
        let mut valid = std::io::Cursor::new(b"{\"ok\":true}\ntrailing".to_vec());
        assert_eq!(
            read_bounded_ipc_line(&mut valid, 32, "test message").unwrap(),
            "{\"ok\":true}\n"
        );

        let mut oversized = std::io::Cursor::new(b"12345\n".to_vec());
        assert!(
            read_bounded_ipc_line(&mut oversized, 4, "test message")
                .unwrap_err()
                .to_string()
                .contains("exceeded")
        );

        let mut unterminated = std::io::Cursor::new(b"{}".to_vec());
        assert!(
            read_bounded_ipc_line(&mut unterminated, 4, "test message")
                .unwrap_err()
                .to_string()
                .contains("newline terminated")
        );

        let mut invalid_utf8 = std::io::Cursor::new(vec![0xff, b'\n']);
        assert!(
            read_bounded_ipc_line(&mut invalid_utf8, 4, "test message")
                .unwrap_err()
                .to_string()
                .contains("valid UTF-8")
        );
    }

    #[test]
    fn tcp_adapter_preserves_bounded_byte_stream_framing() {
        let endpoint = IpcEndpoint::Tcp {
            host: "127.0.0.1".to_owned(),
            port: reserve_tcp_port(),
        };
        let mut listener = IpcListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept(Duration::from_secs(2)).unwrap();
            let line = read_bounded_ipc_line(&mut std::io::BufReader::new(&mut stream), 32, "test")
                .unwrap();
            assert_eq!(line, "ping\n");
            stream.write_all(b"pong\n").unwrap();
        });
        let mut client = IpcStream::connect(&endpoint, Duration::from_secs(2)).unwrap();
        // The accepted side must block within its configured deadline instead
        // of inheriting the listener's nonblocking flag and failing early.
        std::thread::sleep(Duration::from_millis(25));
        client.write_all(b"ping\n").unwrap();
        let mut response = [0; 5];
        client.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"pong\n");
        server.join().unwrap();
    }

    #[test]
    fn accept_timeout_is_typed_and_bounded() {
        let endpoint = IpcEndpoint::Tcp {
            host: "127.0.0.1".to_owned(),
            port: reserve_tcp_port(),
        };
        let mut listener = IpcListener::bind(&endpoint).unwrap();
        let error = listener
            .accept(Duration::from_millis(10))
            .err()
            .expect("accept without a client must time out");
        assert_eq!(error.code, IpcTransportErrorCode::AcceptTimeout);
        assert_eq!(error.io_kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn ipc_server_drop_joins_its_bounded_listener() {
        use std::sync::Arc;

        let endpoint = IpcEndpoint::Tcp {
            host: "127.0.0.1".to_owned(),
            port: reserve_tcp_port(),
        };
        let listener = IpcListener::bind(&endpoint).unwrap();
        let server = super::server::spawn_ipc_server(
            listener,
            0,
            Arc::new(crate::wake_signal::WakeSignal::new()),
        );

        let started = std::time::Instant::now();
        drop(server);

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "listener shutdown must remain bounded"
        );
    }

    #[test]
    fn transport_frame_covers_bounded_embedded_ui_completion() {
        assert_eq!(IPC_REQUEST_MAX_BYTES, 4 * 1024 * 1024);
        assert!(
            IPC_REQUEST_MAX_BYTES
                >= (crate::ui_command::UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES as u64) * 2
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_recovers_only_an_owned_stale_socket() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let directory = unique_temp_directory("stale");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("server.sock");
        std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::fs::write(
            path.with_file_name("server.sock.lock"),
            "v1 pid=999999 start=12345",
        )
        .unwrap();
        std::fs::set_permissions(
            path.with_file_name("server.sock.lock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();

        let endpoint = IpcEndpoint::UnixSocket(path.to_string_lossy().into_owned());
        let listener = IpcListener::bind(&endpoint).unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        drop(listener);
        assert!(!path.exists());
        assert!(!path.with_file_name("server.sock.lock").exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_rejects_unidentified_stale_socket_and_duplicate_authority() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = unique_temp_directory("lease");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("server.sock");
        std::os::unix::net::UnixListener::bind(&path).unwrap();
        let endpoint = IpcEndpoint::UnixSocket(path.to_string_lossy().into_owned());
        let unidentified = IpcListener::bind(&endpoint)
            .err()
            .expect("a pre-lease stale socket must fail closed");
        assert_eq!(unidentified.code, IpcTransportErrorCode::UnsafeEndpoint);

        std::fs::remove_file(&path).unwrap();
        let listener = IpcListener::bind(&endpoint).unwrap();
        let duplicate = IpcListener::bind(&endpoint)
            .err()
            .expect("the held same-instance lease must reject a duplicate authority");
        assert_eq!(duplicate.code, IpcTransportErrorCode::EndpointInUse);
        drop(listener);
        assert!(!path.with_file_name("server.sock.lock").exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_accepts_a_same_uid_peer() {
        use std::io::{Read as _, Write as _};
        use std::os::unix::fs::PermissionsExt as _;

        let directory = unique_temp_directory("peer");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("server.sock");
        let endpoint = IpcEndpoint::UnixSocket(path.to_string_lossy().into_owned());
        let mut listener = IpcListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept(Duration::from_secs(2)).unwrap();
            let mut byte = [0; 1];
            stream.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [7]);
        });
        let mut client = IpcStream::connect(&endpoint, Duration::from_secs(2)).unwrap();
        client.write_all(&[7]).unwrap();
        server.join().unwrap();
        assert!(!path.with_file_name("server.sock.lock").exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_ipc_server_drop_joins_listener_and_removes_owned_endpoint() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::Arc;

        let directory = unique_temp_directory("server-drop");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("server.sock");
        let endpoint = IpcEndpoint::UnixSocket(path.to_string_lossy().into_owned());
        let listener = IpcListener::bind(&endpoint).unwrap();
        let server = super::server::spawn_ipc_server(
            listener,
            0,
            Arc::new(crate::wake_signal::WakeSignal::new()),
        );
        assert!(path.exists());
        assert!(path.with_file_name("server.sock.lock").exists());

        let started = std::time::Instant::now();
        drop(server);

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "listener shutdown must remain bounded"
        );
        assert!(!path.exists());
        assert!(!path.with_file_name("server.sock.lock").exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_completes_a_large_response_after_socket_backpressure() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = unique_temp_directory("backpressure");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("server.sock");
        let endpoint = IpcEndpoint::UnixSocket(path.to_string_lossy().into_owned());
        let mut listener = IpcListener::bind(&endpoint).unwrap();
        let mut expected = vec![b'x'; 2 * 1024 * 1024];
        expected.push(b'\n');
        let response = expected.clone();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept(Duration::from_secs(2)).unwrap();
            stream.write_all(&response).unwrap();
        });

        let mut client = IpcStream::connect(&endpoint, Duration::from_secs(2)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let mut actual = Vec::new();
        client.read_to_end(&mut actual).unwrap();
        server.join().unwrap();
        assert_eq!(actual, expected);

        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_rejects_a_symlink_runtime_directory() {
        let directory = unique_temp_directory("symlink");
        let target = directory.with_extension("target");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &directory).unwrap();
        let endpoint =
            IpcEndpoint::UnixSocket(directory.join("server.sock").to_string_lossy().into_owned());
        let error = IpcListener::bind(&endpoint)
            .err()
            .expect("symlink runtime directory must be rejected");
        assert_eq!(error.code, IpcTransportErrorCode::UnsafeEndpoint);
        std::fs::remove_file(directory).unwrap();
        std::fs::remove_dir(target).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_adapter_rejects_symlink_ancestry_and_never_repairs_existing_parent() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let root = unique_temp_directory("ancestor");
        let target = root.with_extension("target");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();

        let redirect = root.join("redirect");
        symlink(&target, &redirect).unwrap();
        let endpoint = IpcEndpoint::UnixSocket(
            redirect
                .join("private")
                .join("server.sock")
                .to_string_lossy()
                .into_owned(),
        );
        let error = IpcListener::bind(&endpoint)
            .err()
            .expect("symlink ancestry must be rejected");
        assert_eq!(error.code, IpcTransportErrorCode::UnsafeEndpoint);

        let public = root.join("public");
        std::fs::create_dir(&public).unwrap();
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755)).unwrap();
        let endpoint =
            IpcEndpoint::UnixSocket(public.join("server.sock").to_string_lossy().into_owned());
        let error = IpcListener::bind(&endpoint)
            .err()
            .expect("existing public parent must be rejected, not repaired");
        assert_eq!(error.code, IpcTransportErrorCode::UnsafeEndpoint);
        assert_eq!(
            std::fs::symlink_metadata(&public)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );

        std::fs::remove_file(redirect).unwrap();
        std::fs::remove_dir(public).unwrap();
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_dir(target).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_adapter_round_trips_with_a_private_overlapped_pipe() {
        let endpoint = IpcEndpoint::NamedPipe(format!(
            r"\\.\pipe\agenterm-test-{}-{}",
            std::process::id(),
            unique_counter()
        ));
        let mut listener = IpcListener::bind(&endpoint).unwrap();
        let duplicate = IpcListener::bind(&endpoint)
            .err()
            .expect("FILE_FLAG_FIRST_PIPE_INSTANCE must reject a second authority");
        assert_eq!(duplicate.code, IpcTransportErrorCode::EndpointInUse);
        let timeout = listener
            .accept(Duration::from_millis(5))
            .err()
            .expect("empty named pipe must time out");
        assert_eq!(timeout.code, IpcTransportErrorCode::AcceptTimeout);
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept(Duration::from_secs(2)).unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"ping\n");
            stream.write_all(b"pong\n").unwrap();
        });
        let mut client = IpcStream::connect(&endpoint, Duration::from_secs(2)).unwrap();
        client.write_all(b"ping\n").unwrap();
        let mut response = [0; 5];
        client.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"pong\n");
        server.join().unwrap();
    }

    fn reserve_tcp_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[cfg(unix)]
    fn unique_temp_directory(label: &str) -> std::path::PathBuf {
        #[cfg(unix)]
        let base = crate::ipc_endpoint::fallback_unix_runtime_base();
        #[cfg(not(unix))]
        let base = std::env::temp_dir();
        base.join(format!(
            "agenterm-ipc-{label}-{}-{}",
            std::process::id(),
            unique_counter()
        ))
    }

    #[cfg(any(unix, windows))]
    fn unique_counter() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}
