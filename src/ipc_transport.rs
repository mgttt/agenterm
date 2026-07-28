use std::io::{BufRead as _, Read as _};

use anyhow::{Context as _, Result};

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

#[cfg(windows)]
mod server {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc::{self, Receiver, Sender, SyncSender},
        },
        thread,
    };

    use std::io::Write as _;

    use anyhow::{Context as _, Result};

    use crate::{
        IPC_TIMEOUT, client,
        protocol::{IpcRequest, IpcResponse},
        request_gui_wake,
        wake_signal::WakeSignal,
    };

    use super::read_bounded_ipc_line;

    const IPC_MAX_REQUEST_BYTES: u64 = 256 * 1024;
    const IPC_MAX_CONCURRENT_CONNECTIONS: usize = 32;
    const IPC_MAX_PENDING_REQUESTS: usize = 64;

    pub(crate) struct IpcEnvelope {
        pub(crate) request: IpcRequest,
        pub(crate) respond_to: Sender<IpcResponse>,
    }

    pub(crate) fn start_ipc_server(
        window: isize,
        wake_signal: Arc<WakeSignal>,
    ) -> Result<Receiver<IpcEnvelope>> {
        let listener = std::net::TcpListener::bind(client::ipc_socket_addr()?)
            .context("another AgenTerm server is already using the local IPC port")?;
        let (sender, receiver) = mpsc::sync_channel(IPC_MAX_PENDING_REQUESTS);
        let wake_window = window;
        thread::spawn(move || {
            let active_connections = Arc::new(AtomicUsize::new(0));
            for connection in listener.incoming() {
                let Ok(connection) = connection else {
                    continue;
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
        Ok(receiver)
    }

    struct IpcConnectionPermit(Arc<AtomicUsize>);

    impl Drop for IpcConnectionPermit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn handle_ipc_connection(
        connection: std::net::TcpStream,
        sender: &SyncSender<IpcEnvelope>,
        wake_window: isize,
        wake_signal: &WakeSignal,
    ) {
        let _ = connection.set_read_timeout(Some(IPC_TIMEOUT));
        let _ = connection.set_write_timeout(Some(IPC_TIMEOUT));
        let mut reader = std::io::BufReader::new(connection);
        let response =
            match read_bounded_ipc_line(&mut reader, IPC_MAX_REQUEST_BYTES, "AgenTerm IPC request")
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

#[cfg(windows)]
pub(crate) use server::{IpcEnvelope, start_ipc_server};

#[cfg(test)]
mod tests {
    use super::read_bounded_ipc_line;

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
}
