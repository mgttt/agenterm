//! Windows ConPTY adapter.

use std::ffi::OsString;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, ENABLE_LINE_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, FreeConsole, GetConsoleMode,
    SetConsoleCtrlHandler,
};

use crate::contract::pty::{
    NativeInputOwnership, NativeTerminalKey, ProcessId, PtyError, PtyResult, TerminalSize,
};

const ENHANCED_KEY: u32 = 0x0100;
static CONSOLE_ATTACH_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
pub struct ChildCommand(rmux_pty::ChildCommand);

impl ChildCommand {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self(rmux_pty::ChildCommand::new(program))
    }

    #[must_use]
    pub fn arg(self, arg: impl Into<OsString>) -> Self {
        Self(self.0.arg(arg))
    }

    #[must_use]
    pub fn env(self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        Self(self.0.env(key, value))
    }

    #[must_use]
    pub fn current_dir(self, path: impl Into<PathBuf>) -> Self {
        Self(self.0.current_dir(path))
    }

    #[must_use]
    pub fn size(self, size: TerminalSize) -> Self {
        Self(self.0.size(native_size(size)))
    }

    pub fn spawn(self) -> PtyResult<SpawnedPty> {
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

#[derive(Debug)]
pub struct PtyIo<'a>(&'a rmux_pty::PtyIo);

impl PtyIo<'_> {
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

#[derive(Debug)]
pub struct PtyMaster(rmux_pty::PtyMaster);

impl PtyMaster {
    pub fn resize(&self, size: TerminalSize) -> PtyResult<()> {
        self.0
            .resize(native_size(size))
            .map_err(|error| pty_error("resize", "pty_resize_failed", error))
    }

    pub fn try_clone_for_startup_reader(&mut self) -> PtyResult<Self> {
        self.0
            .try_clone_for_startup_reader()
            .map(Self)
            .map_err(|error| pty_error("clone reader", "pty_reader_clone_failed", error))
    }

    #[must_use]
    pub fn io(&self) -> PtyIo<'_> {
        PtyIo(self.0.io())
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }
}

#[derive(Debug)]
pub struct PtyChild(rmux_pty::PtyChild);

impl PtyChild {
    #[must_use]
    pub fn pid(&self) -> ProcessId {
        ProcessId::new(self.0.pid().as_u32())
            .expect("rmux-pty returned a previously validated process id")
    }

    pub fn wait(&mut self) -> PtyResult<ExitStatus> {
        self.0
            .wait()
            .map_err(|error| pty_error("wait", "pty_wait_failed", error))
    }

    pub fn try_clone_for_wait(&self) -> PtyResult<Self> {
        self.0
            .try_clone_for_wait()
            .map(Self)
            .map_err(|error| pty_error("clone wait handle", "pty_wait_clone_failed", error))
    }

    pub fn close_pseudoconsole(&self) {
        self.0.close_pseudoconsole();
    }

    pub fn terminate_forcefully(&self) -> PtyResult<()> {
        self.0
            .terminate_forcefully()
            .map_err(|error| pty_error("terminate", "pty_terminate_failed", error))
    }

    /// Injects a native console key event into the child console.
    ///
    /// This preserves Win32 key identity and repeat semantics instead of
    /// approximating the key with bytes written to the ConPTY input stream.
    pub fn send_native_key(&self, key: NativeTerminalKey, repeat_count: u16) -> PtyResult<()> {
        if repeat_count == 0 {
            return Err(PtyError::failed(
                "send native key",
                "pty_native_key_invalid_repeat",
                "repeat count must be greater than zero",
            ));
        }
        let _guard = CONSOLE_ATTACH_LOCK.lock().map_err(|_| {
            PtyError::failed(
                "send native key",
                "pty_console_attach_lock_poisoned",
                "Windows console attach lock poisoned",
            )
        })?;
        rmux_pty::write_windows_console_key(
            self.0.pid(),
            native_console_key_event(key, repeat_count),
        )
        .map_err(|error| PtyError::failed("send native key", "pty_native_key_failed", error))
    }

    /// Queries the target child's console mode without guessing from terminal
    /// output or from the host process's own console state.
    pub fn native_input_ownership(&self) -> PtyResult<NativeInputOwnership> {
        let _guard = CONSOLE_ATTACH_LOCK.lock().map_err(|_| {
            PtyError::failed(
                "inspect native input ownership",
                "pty_console_attach_lock_poisoned",
                "Windows console attach lock poisoned",
            )
        })?;
        let _attachment = ConsoleAttachment::attach(self.0.pid().as_u32()).map_err(|error| {
            PtyError::failed(
                "inspect native input ownership",
                "pty_console_attach_failed",
                error,
            )
        })?;
        let input = open_console_input().map_err(|error| {
            PtyError::failed(
                "inspect native input ownership",
                "pty_console_input_open_failed",
                error,
            )
        })?;
        let mode = console_input_mode(&input).map_err(|error| {
            PtyError::failed(
                "inspect native input ownership",
                "pty_console_mode_query_failed",
                error,
            )
        })?;
        Ok(classify_console_input_mode(mode))
    }
}

const fn classify_console_input_mode(mode: u32) -> NativeInputOwnership {
    if mode & ENABLE_LINE_INPUT != 0 {
        NativeInputOwnership::Cooked
    } else if mode & ENABLE_VIRTUAL_TERMINAL_INPUT != 0 {
        NativeInputOwnership::RawVt
    } else {
        NativeInputOwnership::RawNative
    }
}

fn open_console_input() -> io::Result<OwnedHandle> {
    const CONIN: [u16; 7] = [
        b'C' as u16,
        b'O' as u16,
        b'N' as u16,
        b'I' as u16,
        b'N' as u16,
        b'$' as u16,
        0,
    ];
    let handle = unsafe {
        // SAFETY: CONIN is NUL-terminated and identifies the input device of
        // the console attached for the duration of this query.
        CreateFileW(
            CONIN.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        // SAFETY: CreateFileW returned a unique owned handle, transferred once.
        OwnedHandle::from_raw_handle(handle as _)
    })
}

fn console_input_mode(input: &OwnedHandle) -> io::Result<u32> {
    let mut mode = 0;
    let result = unsafe {
        // SAFETY: input is an open CONIN$ handle and mode is writable.
        GetConsoleMode(input.as_raw_handle() as _, &mut mode)
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(mode)
}

struct ConsoleAttachment {
    _ignore_console_control: ConsoleControlIgnoreGuard,
}

impl ConsoleAttachment {
    fn attach(process_id: u32) -> io::Result<Self> {
        let _ = unsafe {
            // SAFETY: Console attachment is process-wide and serialized above.
            FreeConsole()
        };
        let attached = unsafe {
            // SAFETY: AttachConsole validates the nonzero child process id.
            AttachConsole(process_id)
        };
        if attached == 0 {
            return Err(io::Error::last_os_error());
        }
        match ConsoleControlIgnoreGuard::install() {
            Ok(ignore_console_control) => Ok(Self {
                _ignore_console_control: ignore_console_control,
            }),
            Err(error) => {
                let _ = unsafe {
                    // SAFETY: AttachConsole succeeded immediately above.
                    FreeConsole()
                };
                Err(error)
            }
        }
    }
}

impl Drop for ConsoleAttachment {
    fn drop(&mut self) {
        let _ = unsafe {
            // SAFETY: This releases this scope's serialized attachment.
            FreeConsole()
        };
    }
}

struct ConsoleControlIgnoreGuard;

impl ConsoleControlIgnoreGuard {
    fn install() -> io::Result<Self> {
        let result = unsafe {
            // SAFETY: The callback has the required static system ABI.
            SetConsoleCtrlHandler(Some(ignore_console_control_event), 1)
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self)
    }
}

impl Drop for ConsoleControlIgnoreGuard {
    fn drop(&mut self) {
        let _ = unsafe {
            // SAFETY: Removes the exact callback installed by install.
            SetConsoleCtrlHandler(Some(ignore_console_control_event), 0)
        };
    }
}

unsafe extern "system" fn ignore_console_control_event(_control_type: u32) -> i32 {
    1
}

const fn native_console_key_event(
    key: NativeTerminalKey,
    repeat_count: u16,
) -> rmux_pty::WindowsConsoleKeyEvent {
    let (virtual_key_code, virtual_scan_code) = match key {
        NativeTerminalKey::Up => (0x26, 0x48),
        NativeTerminalKey::Down => (0x28, 0x50),
    };
    rmux_pty::WindowsConsoleKeyEvent::new(
        virtual_key_code,
        virtual_scan_code,
        0,
        ENHANCED_KEY,
        repeat_count,
    )
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
    use super::{
        NativeInputOwnership, NativeTerminalKey, ProcessId, TerminalSize,
        classify_console_input_mode, native_console_key_event, native_size,
    };

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

    #[test]
    fn native_cursor_keys_preserve_win32_identity_and_repeat() {
        let up = native_console_key_event(NativeTerminalKey::Up, 3);
        assert_eq!(up.virtual_key_code(), 0x26);
        assert_eq!(up.virtual_scan_code(), 0x48);
        assert_eq!(up.unicode_char(), 0);
        assert_eq!(up.control_key_state(), super::ENHANCED_KEY);
        assert_eq!(up.repeat_count(), 3);

        let down = native_console_key_event(NativeTerminalKey::Down, 7);
        assert_eq!(down.virtual_key_code(), 0x28);
        assert_eq!(down.virtual_scan_code(), 0x50);
        assert_eq!(down.unicode_char(), 0);
        assert_eq!(down.control_key_state(), super::ENHANCED_KEY);
        assert_eq!(down.repeat_count(), 7);
    }

    #[test]
    fn console_input_mode_classification_has_explicit_precedence() {
        assert_eq!(
            classify_console_input_mode(super::ENABLE_LINE_INPUT),
            NativeInputOwnership::Cooked
        );
        assert_eq!(
            classify_console_input_mode(super::ENABLE_VIRTUAL_TERMINAL_INPUT),
            NativeInputOwnership::RawVt
        );
        assert_eq!(
            classify_console_input_mode(0),
            NativeInputOwnership::RawNative
        );
        assert_eq!(
            classify_console_input_mode(
                super::ENABLE_LINE_INPUT | super::ENABLE_VIRTUAL_TERMINAL_INPUT
            ),
            NativeInputOwnership::Cooked
        );
    }
}
