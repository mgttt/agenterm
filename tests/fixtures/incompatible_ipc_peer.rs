use std::{
    env, fs,
    io::{self, Write as _},
    net::TcpListener,
    path::Path,
    thread,
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let address = args.next().ok_or("missing address")?;
    let ready = args.next().ok_or("missing ready path")?;
    let stop = args.next().ok_or("missing stop path")?;
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }

    let address = address.to_str().ok_or("address is not UTF-8")?;
    let listener = TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;
    fs::write(&ready, b"ready")?;

    while !Path::new(&stop).exists() {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.write_all(b"not-json\n")?;
                stream.flush()?;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
