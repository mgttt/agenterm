use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use rhai::{Engine, EvalAltResult, Module};

use crate::{
    script_error::runtime_error, script_process::ScriptDuration, script_stdlib::ScriptBytes,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ADDRESS_BYTES: usize = 4 * 1024;
const MAX_IO_BYTES: usize = 1024 * 1024;
const MAX_RESOLVED_ADDRESSES: usize = 32;

#[derive(Clone, Debug)]
pub struct ScriptTcpStream(Arc<Mutex<TcpStream>>);

pub(crate) fn register(engine: &mut Engine, std_module: &mut Module) {
    engine.register_type_with_name::<ScriptTcpStream>("TcpStream");
    engine.register_get("peer_addr", tcp_peer_addr);
    engine.register_get("local_addr", tcp_local_addr);
    engine.register_fn("set_read_timeout", tcp_set_read_timeout);
    engine.register_fn("set_write_timeout", tcp_set_write_timeout);
    engine.register_fn("set_nodelay", tcp_set_nodelay);
    engine.register_fn("write_all", tcp_write_all_text);
    engine.register_fn("write_all", tcp_write_all_bytes);
    engine.register_fn("flush", tcp_flush);
    engine.register_fn("read", tcp_read);
    engine.register_fn("read_line", tcp_read_line);
    engine.register_fn("shutdown", tcp_shutdown);

    let mut tcp_stream = Module::new();
    tcp_stream.set_native_fn("connect", tcp_connect);
    tcp_stream.set_native_fn("connect_timeout", tcp_connect_timeout);

    let mut net = Module::new();
    net.set_sub_module("TcpStream", tcp_stream);
    std_module.set_sub_module("net", net);
}

fn tcp_connect(address: &str) -> Result<ScriptTcpStream, Box<EvalAltResult>> {
    connect_with_timeout(address, DEFAULT_TIMEOUT)
}

fn tcp_connect_timeout(
    address: &str,
    timeout: ScriptDuration,
) -> Result<ScriptTcpStream, Box<EvalAltResult>> {
    connect_with_timeout(
        address,
        validate_timeout(timeout, "std.net.TcpStream.connect_timeout")?,
    )
}

fn connect_with_timeout(
    address: &str,
    timeout: Duration,
) -> Result<ScriptTcpStream, Box<EvalAltResult>> {
    if address.is_empty() || address.len() > MAX_ADDRESS_BYTES {
        return Err(net_error(
            "net_address_invalid",
            "std.net.TcpStream.connect",
            "TCP address must be 1..4096 UTF-8 bytes",
            false,
            false,
            Some("invalid_input"),
        ));
    }
    let addresses = address
        .to_socket_addrs()
        .map_err(|error| io_error("net_resolve", "std.net.TcpStream.connect", error))?
        .take(MAX_RESOLVED_ADDRESSES)
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(net_error(
            "net_resolve_empty",
            "std.net.TcpStream.connect",
            "TCP address resolved to no endpoints",
            true,
            false,
            Some("not_found"),
        ));
    }

    let started = Instant::now();
    let mut last_error = None;
    for resolved in addresses {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&resolved, remaining) {
            Ok(stream) => {
                stream.set_read_timeout(Some(timeout)).map_err(|error| {
                    io_error(
                        "net_read_timeout_config",
                        "std.net.TcpStream.connect",
                        error,
                    )
                })?;
                stream.set_write_timeout(Some(timeout)).map_err(|error| {
                    io_error(
                        "net_write_timeout_config",
                        "std.net.TcpStream.connect",
                        error,
                    )
                })?;
                return Ok(ScriptTcpStream(Arc::new(Mutex::new(stream))));
            }
            Err(error) => last_error = Some(error),
        }
    }
    let error = last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "deadline"));
    Err(io_error(
        if matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ) {
            "net_connect_timeout"
        } else {
            "net_connect"
        },
        "std.net.TcpStream.connect",
        error,
    ))
}

fn tcp_peer_addr(stream: &mut ScriptTcpStream) -> Result<String, Box<EvalAltResult>> {
    lock_stream(stream, "TcpStream.peer_addr")?
        .peer_addr()
        .map(|address| address.to_string())
        .map_err(|error| io_error("net_peer_addr", "TcpStream.peer_addr", error))
}

fn tcp_local_addr(stream: &mut ScriptTcpStream) -> Result<String, Box<EvalAltResult>> {
    lock_stream(stream, "TcpStream.local_addr")?
        .local_addr()
        .map(|address| address.to_string())
        .map_err(|error| io_error("net_local_addr", "TcpStream.local_addr", error))
}

fn tcp_set_read_timeout(
    stream: &mut ScriptTcpStream,
    timeout: ScriptDuration,
) -> Result<(), Box<EvalAltResult>> {
    let timeout = validate_timeout(timeout, "TcpStream.set_read_timeout")?;
    lock_stream(stream, "TcpStream.set_read_timeout")?
        .set_read_timeout(Some(timeout))
        .map_err(|error| {
            io_error(
                "net_read_timeout_config",
                "TcpStream.set_read_timeout",
                error,
            )
        })
}

fn tcp_set_write_timeout(
    stream: &mut ScriptTcpStream,
    timeout: ScriptDuration,
) -> Result<(), Box<EvalAltResult>> {
    let timeout = validate_timeout(timeout, "TcpStream.set_write_timeout")?;
    lock_stream(stream, "TcpStream.set_write_timeout")?
        .set_write_timeout(Some(timeout))
        .map_err(|error| {
            io_error(
                "net_write_timeout_config",
                "TcpStream.set_write_timeout",
                error,
            )
        })
}

fn tcp_set_nodelay(stream: &mut ScriptTcpStream, enabled: bool) -> Result<(), Box<EvalAltResult>> {
    lock_stream(stream, "TcpStream.set_nodelay")?
        .set_nodelay(enabled)
        .map_err(|error| io_error("net_nodelay", "TcpStream.set_nodelay", error))
}

fn tcp_write_all_text(stream: &mut ScriptTcpStream, value: &str) -> Result<(), Box<EvalAltResult>> {
    write_all(stream, value.as_bytes())
}

fn tcp_write_all_bytes(
    stream: &mut ScriptTcpStream,
    value: ScriptBytes,
) -> Result<(), Box<EvalAltResult>> {
    write_all(stream, &value.0)
}

fn write_all(stream: &mut ScriptTcpStream, value: &[u8]) -> Result<(), Box<EvalAltResult>> {
    if value.len() > MAX_IO_BYTES {
        return Err(net_error(
            "net_write_limit",
            "TcpStream.write_all",
            "TCP write exceeds the 1048576-byte call limit",
            false,
            true,
            Some("limit"),
        ));
    }
    lock_stream(stream, "TcpStream.write_all")?
        .write_all(value)
        .map_err(|error| io_error("net_write", "TcpStream.write_all", error))
}

fn tcp_flush(stream: &mut ScriptTcpStream) -> Result<(), Box<EvalAltResult>> {
    lock_stream(stream, "TcpStream.flush")?
        .flush()
        .map_err(|error| io_error("net_flush", "TcpStream.flush", error))
}

fn tcp_read(
    stream: &mut ScriptTcpStream,
    max_bytes: rhai::INT,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    let max_bytes = validate_io_limit(max_bytes, "TcpStream.read")?;
    let mut output = vec![0_u8; max_bytes];
    let count = lock_stream(stream, "TcpStream.read")?
        .read(&mut output)
        .map_err(|error| io_error("net_read", "TcpStream.read", error))?;
    output.truncate(count);
    Ok(ScriptBytes(output))
}

fn tcp_read_line(
    stream: &mut ScriptTcpStream,
    max_bytes: rhai::INT,
) -> Result<String, Box<EvalAltResult>> {
    let max_bytes = validate_io_limit(max_bytes, "TcpStream.read_line")?;
    let mut stream = lock_stream(stream, "TcpStream.read_line")?;
    let mut output = Vec::with_capacity(max_bytes.min(8 * 1024));
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) if output.is_empty() => {
                return Err(net_error(
                    "net_eof",
                    "TcpStream.read_line",
                    "TCP peer closed before returning a line",
                    false,
                    false,
                    Some("unexpected_eof"),
                ));
            }
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => {
                if output.len() == max_bytes {
                    return Err(net_error(
                        "net_read_limit",
                        "TcpStream.read_line",
                        "TCP line exceeds the selected byte limit",
                        false,
                        true,
                        Some("limit"),
                    ));
                }
                output.push(byte[0]);
            }
            Err(error) => {
                return Err(io_error("net_read", "TcpStream.read_line", error));
            }
        }
    }
    if output.last() == Some(&b'\r') {
        output.pop();
    }
    String::from_utf8(output).map_err(|_| {
        net_error(
            "net_read_not_utf8",
            "TcpStream.read_line",
            "TCP line is not valid UTF-8",
            false,
            false,
            Some("invalid_data"),
        )
    })
}

fn tcp_shutdown(stream: &mut ScriptTcpStream) -> Result<(), Box<EvalAltResult>> {
    lock_stream(stream, "TcpStream.shutdown")?
        .shutdown(Shutdown::Both)
        .map_err(|error| io_error("net_shutdown", "TcpStream.shutdown", error))
}

fn validate_timeout(
    timeout: ScriptDuration,
    operation: &'static str,
) -> Result<Duration, Box<EvalAltResult>> {
    if timeout.0.is_zero() || timeout.0 > MAX_TIMEOUT {
        return Err(net_error(
            "net_timeout_invalid",
            operation,
            "TCP timeout must be from 1 through 60000 milliseconds",
            false,
            false,
            Some("invalid_input"),
        ));
    }
    Ok(timeout.0)
}

fn validate_io_limit(
    value: rhai::INT,
    operation: &'static str,
) -> Result<usize, Box<EvalAltResult>> {
    usize::try_from(value)
        .ok()
        .filter(|value| (1..=MAX_IO_BYTES).contains(value))
        .ok_or_else(|| {
            net_error(
                "net_io_limit_invalid",
                operation,
                "TCP byte limit must be from 1 through 1048576",
                false,
                false,
                Some("invalid_input"),
            )
        })
}

fn lock_stream<'a>(
    stream: &'a ScriptTcpStream,
    operation: &'static str,
) -> Result<MutexGuard<'a, TcpStream>, Box<EvalAltResult>> {
    stream.0.lock().map_err(|_| {
        net_error(
            "net_stream_poisoned",
            operation,
            "TCP stream state is unavailable",
            false,
            false,
            Some("host_invariant"),
        )
    })
}

fn io_error(code: &'static str, operation: &'static str, error: io::Error) -> Box<EvalAltResult> {
    let timed_out = matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    );
    net_error(
        if timed_out {
            match code {
                "net_read" => "net_read_timeout",
                "net_write" => "net_write_timeout",
                other => other,
            }
        } else {
            code
        },
        operation,
        if timed_out {
            "TCP operation exceeded its deadline"
        } else {
            "TCP operation failed"
        },
        timed_out
            || matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::NotConnected
            ),
        false,
        Some(io_cause(error.kind())),
    )
}

fn io_cause(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "timeout",
        io::ErrorKind::ConnectionRefused => "connection_refused",
        io::ErrorKind::ConnectionReset => "connection_reset",
        io::ErrorKind::ConnectionAborted => "connection_aborted",
        io::ErrorKind::NotConnected => "not_connected",
        io::ErrorKind::AddrInUse => "address_in_use",
        io::ErrorKind::AddrNotAvailable => "address_unavailable",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::UnexpectedEof => "unexpected_eof",
        _ => "io",
    }
}

#[allow(clippy::too_many_arguments)]
fn net_error(
    code: &'static str,
    operation: &'static str,
    message: &'static str,
    retryable: bool,
    truncated: bool,
    cause: Option<&'static str>,
) -> Box<EvalAltResult> {
    runtime_error(
        "network",
        code,
        operation,
        message,
        retryable,
        "tcp_stream",
        truncated,
        cause,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn tcp_stream_round_trip_is_bounded_typed_and_address_observable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert_eq!(request, "hello\n");
            stream.write_all(b"world\r\n").unwrap();
        });

        let mut client =
            connect_with_timeout(&address.to_string(), Duration::from_secs(2)).unwrap();
        assert_eq!(tcp_peer_addr(&mut client).unwrap(), address.to_string());
        tcp_write_all_text(&mut client, "hello\n").unwrap();
        tcp_flush(&mut client).unwrap();
        assert_eq!(tcp_read_line(&mut client, 64).unwrap(), "world");
        tcp_shutdown(&mut client).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn tcp_limits_fail_before_io() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let _ = listener.accept();
        });
        let mut client =
            connect_with_timeout(&address.to_string(), Duration::from_secs(2)).unwrap();
        assert!(tcp_read(&mut client, 0).is_err());
        assert!(tcp_write_all_text(&mut client, &"x".repeat(MAX_IO_BYTES + 1)).is_err());
        drop(client);
        server.join().unwrap();
    }
}
