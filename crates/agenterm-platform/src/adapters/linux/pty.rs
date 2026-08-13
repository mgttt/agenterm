use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use libc::{self, c_int, pid_t};

use crate::contract::pty::{
    NativeInputOwnership, NativeTerminalKey, ProcessId, PtyError, PtyResult, TerminalSize,
};

/// Return the native login-shell argument for a bare supported POSIX shell.
pub fn login_shell_argument(
    program: &std::path::Path,
    explicit_arguments: usize,
) -> Option<&'static str> {
    if explicit_arguments != 0 {
        return None;
    }
    program
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            matches!(
                *name,
                "bash" | "zsh" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh"
            )
        })
        .map(|_| "-l")
}

const PTY_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// A command configuration for spawning a process inside a newly allocated PTY.
#[derive(Clone, Debug)]
pub struct ChildCommand {
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    current_dir: Option<PathBuf>,
    size: Option<TerminalSize>,
}

impl ChildCommand {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            current_dir: None,
            size: None,
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    #[must_use]
    pub fn size(mut self, size: TerminalSize) -> Self {
        self.size = Some(size);
        self
    }

    pub fn spawn(self) -> PtyResult<SpawnedPty> {
        spawn_child(self).map_err(|error| PtyError::failed("spawn", "pty_spawn_failed", error))
    }
}

/// A spawned process together with the PTY master used to communicate with it.
#[derive(Debug)]
pub struct SpawnedPty {
    master: PtyMaster,
    child: PtyChild,
}

impl SpawnedPty {
    #[must_use]
    pub fn into_parts(self) -> (PtyMaster, PtyChild) {
        (self.master, self.child)
    }
}

/// The I/O endpoint for a pseudoterminal master descriptor.
#[derive(Debug)]
pub struct PtyIo {
    fd: Arc<OwnedFd>,
}

impl PtyIo {
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        read_fd(self.fd.as_raw_fd(), buffer)
    }
}

/// The master handle of a pseudoterminal.
#[derive(Debug)]
pub struct PtyMaster {
    io: PtyIo,
}

impl PtyMaster {
    fn from_fd(fd: OwnedFd) -> io::Result<Self> {
        set_nonblocking(fd.as_raw_fd())?;
        Ok(Self {
            io: PtyIo { fd: Arc::new(fd) },
        })
    }

    pub fn resize(&self, size: TerminalSize) -> PtyResult<()> {
        apply_winsize(self.io.fd.as_raw_fd(), size)
            .map_err(|error| PtyError::failed("resize", "pty_resize_failed", error))
    }

    pub fn try_clone(&self) -> PtyResult<Self> {
        let fd = dup_fd(self.io.fd.as_raw_fd())
            .map_err(|error| PtyError::failed("clone reader", "pty_reader_clone_failed", error))?;
        Self::from_fd(fd)
            .map_err(|error| PtyError::failed("clone reader", "pty_reader_clone_failed", error))
    }

    pub fn try_clone_for_startup_reader(&mut self) -> PtyResult<Self> {
        self.try_clone()
    }

    #[must_use]
    pub fn io(&self) -> &PtyIo {
        &self.io
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        write_all_with_timeout(self.io.fd.as_raw_fd(), bytes, PTY_WRITE_TIMEOUT)
    }
}

/// A handle for signaling and reaping a PTY-backed child process.
#[derive(Debug)]
pub struct PtyChild {
    pid: ProcessId,
}

impl PtyChild {
    #[must_use]
    pub fn pid(&self) -> ProcessId {
        self.pid
    }

    pub fn wait(&mut self) -> PtyResult<ExitStatus> {
        let mut status: c_int = 0;
        loop {
            let result = unsafe { libc::waitpid(self.pid.as_u32() as pid_t, &mut status, 0) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(PtyError::failed("wait", "pty_wait_failed", error));
            }
            if result == self.pid.as_u32() as pid_t {
                return Ok(ExitStatus::from_raw(status));
            }
        }
    }

    pub fn try_clone_for_wait(&self) -> PtyResult<Self> {
        Ok(Self { pid: self.pid })
    }

    pub fn close_pseudoconsole(&self) {}

    pub fn terminate_forcefully(&self) -> PtyResult<()> {
        let result = unsafe { libc::kill(self.pid.as_u32() as pid_t, libc::SIGKILL) };
        if result == -1 {
            return Err(PtyError::failed(
                "terminate",
                "pty_terminate_failed",
                io::Error::last_os_error(),
            ));
        }
        Ok(())
    }

    pub fn send_native_key(&self, _key: NativeTerminalKey, _repeat_count: u16) -> PtyResult<()> {
        Err(PtyError::unsupported(
            "send native key",
            "the POSIX PTY adapter has no native console key-event transport",
        ))
    }

    pub fn native_input_ownership(&self) -> PtyResult<NativeInputOwnership> {
        Err(PtyError::unsupported(
            "inspect native input ownership",
            "the POSIX PTY adapter has no Win32 console input mode",
        ))
    }
}

/// Resolve `command.program` the way a POSIX shell would, failing with
/// `NotFound` when no executable exists. Absolute paths and paths with a
/// directory component are not looked up on `PATH`.
fn resolve_posix_executable(command: &ChildCommand) -> io::Result<PathBuf> {
    let program = &command.program;
    if program.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "PTY executable path is empty",
        ));
    }
    if program.is_absolute() || program.components().count() > 1 {
        let candidate = if program.is_absolute() {
            program.clone()
        } else {
            let base = command
                .current_dir
                .clone()
                .or_else(|| env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(program)
        };
        return if is_executable_file(&candidate) {
            Ok(candidate)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("PTY executable not found: {}", candidate.to_string_lossy()),
            ))
        };
    }

    let path_value = command
        .env
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value.clone())
        .or_else(|| env::var_os("PATH"));
    let Some(path_value) = path_value else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "PTY executable not found on PATH: {}",
                program.to_string_lossy()
            ),
        ));
    };
    for directory in env::split_paths(&path_value) {
        let candidate = directory.join(program);
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "PTY executable not found on PATH: {}",
            program.to_string_lossy()
        ),
    ))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn spawn_child(command: ChildCommand) -> io::Result<SpawnedPty> {
    // Fail before fork when the program is missing — matches Windows ConPTY
    // "executable not found" and keeps agenterm-con `-e bad` exit non-zero on
    // macOS/Linux instead of opening a host that only discovers exit 127 later.
    let resolved_program = resolve_posix_executable(&command)?;
    let (master_fd, slave_fd) = open_pty_pair(command.size)?;
    let master = PtyMaster::from_fd(master_fd)?;

    let program = CString::new(resolved_program.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "program path contains NUL"))?;
    let args = build_argv(&program, &command.args)?;
    let env_pairs = build_env_pairs(&command.env)?;
    let current_dir = command
        .current_dir
        .as_ref()
        .map(|directory| CString::new(directory.as_os_str().as_bytes()))
        .transpose()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "working directory contains NUL",
            )
        })?;
    // Built pre-fork: a multithreaded process only clones the calling thread on
    // fork(), so the child may only call async-signal-safe functions until it
    // execve()s. The Rust allocator is not on that list, so every pointer the
    // child needs (argv, envp, cwd) must already exist before fork() runs.
    let argv_ptrs = build_ptr_array(&args);
    let envp_ptrs = build_ptr_array(&env_pairs);

    let child_pid = unsafe { libc::fork() };
    if child_pid == -1 {
        return Err(io::Error::last_os_error());
    }

    if child_pid == 0 {
        let master_raw = master.io.fd.as_raw_fd();
        if let Err(error) = child_setup(
            master_raw,
            slave_fd.as_raw_fd(),
            current_dir.as_deref(),
            &program,
            &argv_ptrs,
            &envp_ptrs,
        ) {
            let message = CString::new(format!("pty child setup failed: {error}"))
                .unwrap_or_else(|_| CString::new("pty child setup failed").expect("static"));
            unsafe {
                libc::write(
                    libc::STDERR_FILENO,
                    message.as_ptr() as *const libc::c_void,
                    message.as_bytes().len(),
                );
            }
            unsafe {
                libc::_exit(127);
            }
        }
        unreachable!("exec replaces the child process image");
    }

    drop(slave_fd);

    let pid = ProcessId::new(child_pid as u32).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("child process returned an invalid pid: {error}"),
        )
    })?;

    Ok(SpawnedPty {
        master,
        child: PtyChild { pid },
    })
}

/// Runs between `fork()` and `execve()`. Every argument is pre-built by the
/// parent so this function performs no heap allocation: POSIX only guarantees
/// async-signal-safe functions are safe to call here in a process that had
/// more than one thread at fork time (see fork(2)), and the Rust allocator
/// gives no such guarantee.
fn child_setup(
    master_fd: RawFd,
    slave_fd: RawFd,
    current_dir: Option<&CStr>,
    program: &CStr,
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
) -> io::Result<()> {
    unsafe {
        if libc::close(master_fd) == -1 {
            return Err(io::Error::last_os_error());
        }

        if libc::dup2(slave_fd, libc::STDIN_FILENO) == -1
            || libc::dup2(slave_fd, libc::STDOUT_FILENO) == -1
            || libc::dup2(slave_fd, libc::STDERR_FILENO) == -1
        {
            return Err(io::Error::last_os_error());
        }

        if slave_fd > libc::STDERR_FILENO {
            libc::close(slave_fd);
        }

        if libc::setsid() == -1 {
            return Err(io::Error::last_os_error());
        }

        // Let the selected libc declaration infer the request type: Linux GNU
        // uses c_ulong, Linux musl uses c_int, and the BSD/macOS declaration
        // uses c_ulong without exporting Linux's `Ioctl` alias. This source is
        // shared by both Unix adapters, so naming either platform typedef here
        // makes another supported target fail at compile time.
        if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) == -1 {
            return Err(io::Error::last_os_error());
        }

        let child_pid = libc::getpid();
        if libc::tcsetpgrp(libc::STDIN_FILENO, child_pid) == -1 {
            return Err(io::Error::last_os_error());
        }

        if let Some(directory) = current_dir
            && libc::chdir(directory.as_ptr()) == -1
        {
            return Err(io::Error::last_os_error());
        }

        let result = libc::execve(program.as_ptr(), argv.as_ptr(), envp.as_ptr());
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Builds a null-terminated argv/envp-style pointer array. Must be called
/// before `fork()`; see [`child_setup`].
fn build_ptr_array(entries: &[CString]) -> Vec<*const libc::c_char> {
    entries
        .iter()
        .map(|entry| entry.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

fn build_argv(program: &CStr, args: &[OsString]) -> io::Result<Vec<CString>> {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(
        CString::new(program.to_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "argv0 contains NUL"))?,
    );
    for arg in args {
        argv.push(
            CString::new(arg.as_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "argument contains NUL")
            })?,
        );
    }
    Ok(argv)
}

fn build_env_pairs(overrides: &[(OsString, OsString)]) -> io::Result<Vec<CString>> {
    let mut entries: Vec<CString> = std::env::vars_os()
        .map(|(key, value)| env_entry_to_cstring(&key, &value))
        .collect::<io::Result<_>>()?;

    for (key, value) in overrides {
        let encoded = env_entry_to_cstring(key, value)?;
        if let Some(existing) = entries.iter_mut().find(|entry| {
            entry
                .to_bytes()
                .split(|byte| *byte == b'=')
                .next()
                .is_some_and(|name| name == key.as_bytes())
        }) {
            *existing = encoded;
        } else {
            entries.push(encoded);
        }
    }

    Ok(entries)
}

fn env_entry_to_cstring(key: &OsStr, value: &OsStr) -> io::Result<CString> {
    let mut bytes = Vec::with_capacity(key.len() + value.len() + 1);
    bytes.extend_from_slice(key.as_bytes());
    bytes.push(b'=');
    bytes.extend_from_slice(value.as_bytes());
    CString::new(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "environment value contains NUL",
        )
    })
}

fn open_pty_pair(size: Option<TerminalSize>) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master: c_int = 0;
    let mut slave: c_int = 0;
    let mut winsize = size.map(into_winsize);
    let winsize_ptr = winsize
        .as_mut()
        .map_or(std::ptr::null_mut(), |value| value as *mut libc::winsize);

    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            winsize_ptr,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    let master_fd = unsafe { OwnedFd::from_raw_fd(master) };
    let slave_fd = unsafe { OwnedFd::from_raw_fd(slave) };
    set_cloexec(master_fd.as_raw_fd())?;
    set_cloexec(slave_fd.as_raw_fd())?;
    Ok((master_fd, slave_fd))
}

fn into_winsize(size: TerminalSize) -> libc::winsize {
    libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn apply_winsize(fd: RawFd, size: TerminalSize) -> io::Result<()> {
    let winsize = into_winsize(size);
    let result = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &winsize) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn dup_fd(fd: RawFd) -> io::Result<OwnedFd> {
    let duplicated = unsafe { libc::dup(fd) };
    if duplicated == -1 {
        return Err(io::Error::last_os_error());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(duplicated) };
    set_cloexec(owned.as_raw_fd())?;
    Ok(owned)
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_fd(fd: RawFd, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        let result =
            unsafe { libc::read(fd, buffer.as_mut_ptr().cast::<libc::c_void>(), buffer.len()) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                wait_until_readable(fd)?;
                continue;
            }
            return Err(error);
        }
        return Ok(result as usize);
    }
}

fn write_all_with_timeout(fd: RawFd, mut buffer: &[u8], timeout: Duration) -> io::Result<()> {
    let started = std::time::Instant::now();
    while !buffer.is_empty() {
        if started.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("pty write made no progress for {} ms", timeout.as_millis()),
            ));
        }
        let result =
            unsafe { libc::write(fd, buffer.as_ptr().cast::<libc::c_void>(), buffer.len()) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                wait_until_writable(fd, started, timeout)?;
                continue;
            }
            return Err(error);
        }
        if result == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
        }
        buffer = &buffer[result as usize..];
    }
    Ok(())
}

fn wait_until_readable(fd: RawFd) -> io::Result<()> {
    loop {
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll_fd, 1, -1) };
        if ready > 0 {
            return Ok(());
        }
        if ready == 0 {
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

fn wait_until_writable(
    fd: RawFd,
    started: std::time::Instant,
    timeout: Duration,
) -> io::Result<()> {
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("pty write made no progress for {} ms", timeout.as_millis()),
            ));
        }
        let millis = i32::try_from(remaining.as_millis().max(1)).unwrap_or(i32::MAX);
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll_fd, 1, millis) };
        if ready > 0 {
            if poll_fd.revents & libc::POLLOUT != 0 {
                return Ok(());
            }
            if poll_fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "pty is no longer writable",
                ));
            }
            continue;
        }
        if ready == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("pty write made no progress for {} ms", timeout.as_millis()),
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn openpty_spawn_write_read_round_trip() {
        let spawned = ChildCommand::new("/bin/sh")
            .arg("-c")
            .arg("echo marker")
            .size(TerminalSize { rows: 24, cols: 80 })
            .spawn()
            .expect("spawn shell in pty");

        let (mut master, mut child) = spawned.into_parts();
        let reader = master
            .try_clone_for_startup_reader()
            .expect("clone pty reader");

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        let mut buffer = [0_u8; 256];
        while output.len() < b"marker".len() && std::time::Instant::now() < deadline {
            match reader.io().read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => output.extend_from_slice(&buffer[..size]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("pty read failed: {error}"),
            }
        }

        child.terminate_forcefully().ok();
        let _ = child.wait();

        assert!(
            output
                .windows(b"marker".len())
                .any(|window| window == b"marker"),
            "expected marker in pty output, got {:?}",
            String::from_utf8_lossy(&output)
        );
    }

    #[test]
    fn native_console_key_injection_is_explicitly_unsupported() {
        let child = PtyChild {
            pid: ProcessId::new(1).expect("valid fixture pid"),
        };

        let error = child
            .send_native_key(NativeTerminalKey::Up, 3)
            .expect_err("POSIX PTYs do not expose Win32 console key events");

        assert!(matches!(error, PtyError::Unsupported { .. }));
        assert!(error.to_string().contains("send native key unsupported"));
    }

    #[test]
    fn native_input_ownership_is_explicitly_unsupported() {
        let child = PtyChild {
            pid: ProcessId::new(1).expect("valid fixture pid"),
        };

        let error = child
            .native_input_ownership()
            .expect_err("POSIX PTYs do not expose Win32 console input modes");

        assert!(matches!(error, PtyError::Unsupported { .. }));
        assert!(
            error
                .to_string()
                .contains("inspect native input ownership unsupported")
        );
    }
}
