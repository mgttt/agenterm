use std::{
    io::{self, BufRead as _, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};

use crate::platform::contract::ipc::IpcEndpoint;
pub(crate) use crate::platform::contract::ipc_transport::{
    IpcTransportError, IpcTransportErrorCode, TransportResult, map_bind_error, timeout_error,
    transport_io, unsupported,
};

// UI command completion embeds a bounded 1 MiB `IpcResponse` JSON document
// inside the outer request string. JSON re-escaping can approach 2x, so the
// transport frame must be larger than the operation-argument budget while
// remaining explicitly bounded.
pub(crate) const IPC_REQUEST_MAX_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const IPC_RESPONSE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// A transport-neutral local IPC listener. This adapter is intentionally
/// staged before the caller migration so TCP and native endpoints can coexist.
pub(crate) struct IpcListener {
    endpoint: IpcEndpoint,
    inner: ListenerInner,
}

enum ListenerInner {
    Tcp(TcpListener),
    Native(crate::platform::services::ipc::NativeListener),
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
            IpcEndpoint::UnixSocket(_) | IpcEndpoint::NamedPipe(_) => ListenerInner::Native(
                crate::platform::services::ipc::NativeListener::bind(endpoint)?,
            ),
            _ => return Err(unsupported(endpoint, "endpoint variant is not supported")),
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
            ListenerInner::Native(listener) => listener
                .accept(timeout)
                .map(|stream| IpcStream::from_native(stream, &self.endpoint)),
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
    Native(crate::platform::services::ipc::NativeStream),
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
            IpcEndpoint::UnixSocket(_) | IpcEndpoint::NamedPipe(_) => {
                crate::platform::services::ipc::NativeStream::connect(endpoint, timeout)
                    .map(|stream| Self::from_native(stream, endpoint))
            }
            _ => Err(unsupported(endpoint, "endpoint variant is not supported")),
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

    fn from_native(
        stream: crate::platform::services::ipc::NativeStream,
        endpoint: &IpcEndpoint,
    ) -> Self {
        Self {
            endpoint: endpoint.clone(),
            inner: StreamInner::Native(stream),
        }
    }

    pub(crate) fn set_io_timeout(&mut self, timeout: Duration) -> TransportResult<()> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream
                .set_read_timeout(Some(timeout))
                .and_then(|()| stream.set_write_timeout(Some(timeout)))
                .map_err(|error| transport_io(&self.endpoint, error)),
            StreamInner::Native(stream) => stream.set_io_timeout(timeout),
        }
    }
}

impl Read for IpcStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.read(buffer),
            StreamInner::Native(stream) => stream.read(buffer),
        }
    }
}

impl Write for IpcStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.write(buffer),
            StreamInner::Native(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            StreamInner::Tcp(stream) => stream.flush(),
            StreamInner::Native(stream) => stream.flush(),
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

        #[allow(dead_code)]
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

#[allow(unused_imports)]
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
