//! Windows ConPTY adapter.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::platform::contract::pty::{ProcessId, PtyError, PtyResult, TerminalSize};

#[derive(Clone, Debug)]
pub(crate) struct ChildCommand(rmux_pty::ChildCommand);

impl ChildCommand {
    #[must_use]
    pub(crate) fn new(program: impl Into<PathBuf>) -> Self {
        Self(rmux_pty::ChildCommand::new(program))
    }

    #[must_use]
    pub(crate) fn arg(self, arg: impl Into<OsString>) -> Self {
        Self(self.0.arg(arg))
    }

    #[must_use]
    pub(crate) fn env(self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        Self(self.0.env(key, value))
    }

    #[must_use]
    pub(crate) fn current_dir(self, path: impl Into<PathBuf>) -> Self {
        Self(self.0.current_dir(path))
    }

    #[must_use]
    pub(crate) fn size(self, size: TerminalSize) -> Self {
        Self(self.0.size(native_size(size)))
    }

    pub(crate) fn spawn(self) -> PtyResult<SpawnedPty> {
        let (master, child) = self
            .0
            .spawn()
            .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?
            .into_parts();
        Ok(SpawnedPty {
            master: PtyMaster(master),
            child: PtyChild(child),
        })
    }
}

#[derive(Debug)]
pub(crate) struct SpawnedPty {
    master: PtyMaster,
    child: PtyChild,
}

impl SpawnedPty {
    #[must_use]
    pub(crate) fn into_parts(self) -> (PtyMaster, PtyChild) {
        (self.master, self.child)
    }
}

#[derive(Debug)]
pub(crate) struct PtyIo<'a>(&'a rmux_pty::PtyIo);

impl PtyIo<'_> {
    pub(crate) fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

#[derive(Debug)]
pub(crate) struct PtyMaster(rmux_pty::PtyMaster);

impl PtyMaster {
    pub(crate) fn resize(&self, size: TerminalSize) -> PtyResult<()> {
        self.0
            .resize(native_size(size))
            .map_err(|error| pty_error("resize", "pty_resize_failed", error))
    }

    pub(crate) fn try_clone_for_startup_reader(&mut self) -> PtyResult<Self> {
        self.0
            .try_clone_for_startup_reader()
            .map(Self)
            .map_err(|error| pty_error("clone reader", "pty_reader_clone_failed", error))
    }

    #[must_use]
    pub(crate) fn io(&self) -> PtyIo<'_> {
        PtyIo(self.0.io())
    }

    pub(crate) fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }
}

#[derive(Debug)]
pub(crate) struct PtyChild(rmux_pty::PtyChild);

impl PtyChild {
    #[must_use]
    pub(crate) fn pid(&self) -> ProcessId {
        ProcessId::new(self.0.pid().as_u32())
            .expect("rmux-pty returned a previously validated process id")
    }

    pub(crate) fn wait(&mut self) -> PtyResult<ExitStatus> {
        self.0
            .wait()
            .map_err(|error| pty_error("wait", "pty_wait_failed", error))
    }

    pub(crate) fn try_clone_for_wait(&self) -> PtyResult<Self> {
        self.0
            .try_clone_for_wait()
            .map(Self)
            .map_err(|error| pty_error("clone wait handle", "pty_wait_clone_failed", error))
    }

    pub(crate) fn close_pseudoconsole(&self) {
        self.0.close_pseudoconsole();
    }

    pub(crate) fn terminate_forcefully(&self) -> PtyResult<()> {
        self.0
            .terminate_forcefully()
            .map_err(|error| pty_error("terminate", "pty_terminate_failed", error))
    }
}

const fn native_size(size: TerminalSize) -> rmux_pty::TerminalSize {
    rmux_pty::TerminalSize {
        rows: size.rows,
        cols: size.cols,
    }
}

fn pty_error(operation: &'static str, code: &'static str, error: rmux_pty::PtyError) -> PtyError {
    match error {
        rmux_pty::PtyError::Unsupported(reason) => PtyError::unsupported(operation, reason),
        error => PtyError::failed(operation, code, error),
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessId, TerminalSize, native_size};

    #[test]
    fn native_size_preserves_neutral_row_and_column_order() {
        let native = native_size(TerminalSize { rows: 24, cols: 80 });

        assert_eq!(native.rows, 24);
        assert_eq!(native.cols, 80);
    }

    #[test]
    fn neutral_process_id_rejects_zero() {
        assert!(ProcessId::new(0).is_err());
        assert_eq!(ProcessId::new(42).expect("valid process id").as_u32(), 42);
    }
}
