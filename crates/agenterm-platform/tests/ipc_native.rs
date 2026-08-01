#![cfg(feature = "ipc")]

use std::{
    io::{Read as _, Write as _},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agenterm_platform::ipc::{IpcEndpoint, NativeListener, NativeStream};

fn unique_endpoint() -> IpcEndpoint {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    #[cfg(windows)]
    {
        IpcEndpoint::NamedPipe(format!(
            r"\\.\pipe\platform-native-{}-{nonce}",
            std::process::id()
        ))
    }
    #[cfg(unix)]
    {
        IpcEndpoint::UnixSocket(
            agenterm_platform::ipc::native_runtime_directory()
                .join(format!("native-{}-{nonce}.sock", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[test]
fn native_stream_round_trip_preserves_borrowed_descriptor_ownership() {
    let endpoint = unique_endpoint();
    let timeout = Duration::from_secs(2);
    let mut listener = NativeListener::bind(&endpoint).expect("bind native listener");
    let mut client = NativeStream::connect(&endpoint, timeout).expect("connect native stream");
    let mut server = listener.accept(timeout).expect("accept native stream");

    #[cfg(windows)]
    {
        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
        assert!(!client.as_raw_handle().is_null());
        let _borrowed = client.as_handle();
        client = NativeStream::from_owned_handle(client.into_owned_handle(), timeout);
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsFd as _, AsRawFd as _};
        assert!(client.as_raw_fd() >= 0);
        let _borrowed = client.as_fd();
        client = NativeStream::from_owned_fd(client.into_owned_fd(), &endpoint, timeout)
            .expect("adopt owned native descriptor");
    }

    client.write_all(b"platform").expect("write client frame");
    let mut request = [0_u8; 8];
    server.read_exact(&mut request).expect("read server frame");
    assert_eq!(&request, b"platform");

    server.write_all(b"ok").expect("write server frame");
    let mut response = [0_u8; 2];
    client.read_exact(&mut response).expect("read client frame");
    assert_eq!(&response, b"ok");
}
