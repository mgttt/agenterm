//! Linux parent-console output adapter.

pub(crate) fn write_stderr(message: &str) -> bool {
    use std::io::Write as _;
    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "{message}").is_ok() && stderr.flush().is_ok()
}

pub(crate) fn write_stdout(message: &str) -> bool {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{message}").is_ok() && stdout.flush().is_ok()
}
