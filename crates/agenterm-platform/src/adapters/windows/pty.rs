//! Direct Windows ConPTY adapter.

use std::cmp::Ordering as CmpOrdering;
use std::collections::VecDeque;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    DuplicateHandle, E_HANDLE, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF,
    ERROR_INVALID_DATA, ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER, ERROR_IO_PENDING,
    ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, S_OK, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Console::{
    COORD, ENABLE_LINE_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, HPCON, INPUT_RECORD,
    INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, SetConsoleCtrlHandler,
    WriteConsoleInputW,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, CreatePipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOEXW;
use windows_sys::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateEventW,
    CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetCurrentProcessId, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    ResetEvent, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use crate::contract::pty::{
    NativeInputOwnership, NativeTerminalKey, ProcessId, PtyError, PtyResult, TerminalSize,
};

use super::console_agent;

/// Windows shells do not accept the POSIX login-shell argument.
pub fn login_shell_argument(
    _program: &std::path::Path,
    _explicit_arguments: usize,
) -> Option<&'static str> {
    None
}

const ENHANCED_KEY: u32 = 0x0100;
const DEFAULT_TERMINAL_SIZE: TerminalSize = TerminalSize { rows: 24, cols: 80 };
const PIPE_BUFFER_SIZE: u32 = 64 * 1024;
const OUTPUT_QUEUE_CAPACITY: usize = 64 * 1024;
const OUTPUT_READ_SIZE: usize = 16 * 1024;
const WRITE_CHUNK_SIZE: usize = 4 * 1024;
const PTY_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

const PSEUDOCONSOLE_RESIZE_QUIRK: u32 = 0x2;
const PSEUDOCONSOLE_WIN32_INPUT_MODE: u32 = 0x4;
const PSEUDOCONSOLE_PASSTHROUGH_MODE: u32 = 0x8;
const PASSTHROUGH_MIN_BUILD: u32 = 22_621;
/// The build that first exported the ConPTY entry points from `kernel32`
/// (Windows 10 1809). Older supported systems — Windows Server 2016 is build
/// 14393 and still in support — export none of the three.
const CONPTY_MIN_BUILD: u32 = 17_763;
const DSR_BOOTSTRAP_TIMEOUT: Duration = Duration::from_millis(200);

static NEXT_CONPTY_PIPE_ID: AtomicU64 = AtomicU64::new(1);

/// The ConPTY entry points, resolved at run time rather than imported.
///
/// `CreatePseudoConsole` and its two companions arrived in Windows 10 1809.
/// A static import of them is not a statement about this adapter — it is a
/// statement about the whole executable, because the PE loader resolves every
/// import before `main` runs. On Windows Server 2016 that turns "the terminal
/// cannot open a PTY tab" into "the program refuses to start", with an
/// entry-point dialog naming a symbol the user has no way to act on.
///
/// Resolving through `GetProcAddress` moves the failure to the one call that
/// needs the feature, where it can be reported as an ordinary unsupported
/// operation. `kernel32` is loaded into every process, so `GetModuleHandleW`
/// borrows the existing reference and there is nothing to free.
mod conpty {
    use super::{CONPTY_MIN_BUILD, COORD, HPCON};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    /// Named so each `transmute` states the exact signature it is producing
    /// rather than inferring one from the field it lands in.
    type Create = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
    type Resize = unsafe extern "system" fn(HPCON, COORD) -> i32;
    type Close = unsafe extern "system" fn(HPCON);
    /// What `GetProcAddress` hands back once the `Option` is unwrapped: a
    /// signature-less pointer that carries no information about the export.
    type Resolved = unsafe extern "system" fn() -> isize;

    struct Entries {
        create: Create,
        resize: Resize,
        close: Close,
    }

    /// `None` records a system without ConPTY. Resolution is attempted once:
    /// a missing export does not become available later in the process.
    static ENTRIES: OnceLock<Option<Entries>> = OnceLock::new();

    fn entries() -> Option<&'static Entries> {
        ENTRIES.get_or_init(resolve).as_ref()
    }

    fn resolve() -> Option<Entries> {
        let name = "kernel32.dll\0".encode_utf16().collect::<Vec<_>>();
        // SAFETY: the name is NUL terminated. GetModuleHandleW borrows the
        // loader's reference to an already-loaded module without adding one,
        // so the returned handle must not be freed.
        let module = unsafe { GetModuleHandleW(name.as_ptr()) };
        if module.is_null() {
            return None;
        }
        // All three or none: a system exporting only part of the trio is not
        // one this adapter knows how to drive, and partial use would trade a
        // load-time failure for a null call.
        // SAFETY: `module` is live for the process lifetime and each name is a
        // NUL-terminated C string. Every transmute target below is the exact
        // documented signature of the export being resolved.
        unsafe {
            Some(Entries {
                create: std::mem::transmute::<Resolved, Create>(GetProcAddress(
                    module,
                    c"CreatePseudoConsole".as_ptr().cast(),
                )?),
                resize: std::mem::transmute::<Resolved, Resize>(GetProcAddress(
                    module,
                    c"ResizePseudoConsole".as_ptr().cast(),
                )?),
                close: std::mem::transmute::<Resolved, Close>(GetProcAddress(
                    module,
                    c"ClosePseudoConsole".as_ptr().cast(),
                )?),
            })
        }
    }

    /// Whether this system can host a pseudoconsole at all. The spawn path
    /// asks before choosing a backend; every other caller should simply call
    /// through and handle the refusal.
    pub(super) fn is_available() -> bool {
        entries().is_some()
    }

    /// Explains the refusal in terms the reader can act on: a version, not a
    /// symbol name.
    pub(super) fn unavailable_reason() -> String {
        format!(
            "this Windows build does not export ConPTY; a pseudoconsole needs \
             Windows 10 build {CONPTY_MIN_BUILD} (1809) or Windows Server 2019 or newer"
        )
    }

    /// `None` means ConPTY is absent; `Some` carries the export's `HRESULT`.
    ///
    /// # Safety
    /// The caller upholds `CreatePseudoConsole`'s contract: both handles are
    /// live for the call and `hpc` is a valid out-pointer.
    pub(super) unsafe fn create(
        size: COORD,
        input: HANDLE,
        output: HANDLE,
        flags: u32,
        hpc: *mut HPCON,
    ) -> Option<i32> {
        let create = entries()?.create;
        Some(unsafe { create(size, input, output, flags, hpc) })
    }

    /// # Safety
    /// `hpc` is a live pseudoconsole the caller keeps alive across this call.
    pub(super) unsafe fn resize(hpc: HPCON, size: COORD) -> Option<i32> {
        let resize = entries()?.resize;
        Some(unsafe { resize(hpc, size) })
    }

    /// # Safety
    /// `hpc` is a live pseudoconsole and is not used again afterwards.
    pub(super) unsafe fn close(hpc: HPCON) {
        // Unreachable without a live HPCON, which only `create` produces and
        // only on a system where resolution already succeeded. Skipping is
        // still the correct response: there is no close function to call.
        if let Some(entries) = entries() {
            unsafe { (entries.close)(hpc) };
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowsConsoleKeyEvent {
    virtual_key_code: u16,
    virtual_scan_code: u16,
    unicode_char: u16,
    control_key_state: u32,
    repeat_count: u16,
}

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
        let size = self.size.unwrap_or(DEFAULT_TERMINAL_SIZE);
        // Selected by capability, not by build number: a system either
        // exports the pseudoconsole entry points or it does not, and that is
        // the only fact this decision depends on. The build number is for the
        // message a user reads, never for the branch.
        if !conpty::is_available() || force_console_agent() {
            return spawn_through_console_agent(self, size);
        }
        let dsr_bootstrap = should_enable_dsr_bootstrap(&self.program);
        let mut session = create_session(size, dsr_bootstrap)
            .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;
        let job = JobObjectGuard::new()
            .map_err(|error| pty_error("create job", "pty_job_create_failed", error))?;

        let process =
            create_suspended_process_with_fallback(&self, &mut session, size, dsr_bootstrap, 0);
        let process = match process {
            Ok(process) => process,
            Err(error) => {
                session.close();
                return Err(pty_error("spawn", "pty_spawn_failed", error));
            }
        };

        match job.assign(process.process_handle()) {
            Ok(()) => resume_as_child(process, job, session),
            Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                // A process created in an outer job can reject the first
                // assignment. Kill the still-suspended unprotected attempt,
                // then retry with CREATE_BREAKAWAY_FROM_JOB. The second
                // attempt must also be assigned before ResumeThread; failure
                // never falls back to an unprotected child.
                drop(process);
                drop(job);

                // The first suspended process was already attached to this
                // pseudoconsole. Terminating it can drive the sole output pump
                // to EOF, so a breakaway retry must own a fresh ConPTY and
                // fresh pipes rather than reviving a closed queue.
                session.close();
                session = match create_session(size, dsr_bootstrap) {
                    Ok(session) => session,
                    Err(session_error) => {
                        return Err(pty_error(
                            "create breakaway session",
                            "pty_spawn_failed",
                            session_error,
                        ));
                    }
                };

                let breakaway_job = match JobObjectGuard::new() {
                    Ok(job) => job,
                    Err(job_error) => {
                        session.close();
                        return Err(pty_error(
                            "create breakaway job",
                            "pty_job_create_failed",
                            job_error,
                        ));
                    }
                };
                let breakaway_process = match create_suspended_process_with_fallback(
                    &self,
                    &mut session,
                    size,
                    dsr_bootstrap,
                    CREATE_BREAKAWAY_FROM_JOB,
                ) {
                    Ok(process) => process,
                    Err(process_error) => {
                        session.close();
                        return Err(pty_error(
                            "spawn breakaway child",
                            "pty_spawn_failed",
                            process_error,
                        ));
                    }
                };
                if let Err(assign_error) = breakaway_job.assign(breakaway_process.process_handle())
                {
                    drop(breakaway_process);
                    session.close();
                    return Err(pty_error(
                        "assign breakaway child",
                        "pty_job_assign_failed",
                        assign_error,
                    ));
                }
                resume_as_child(breakaway_process, breakaway_job, session)
            }
            Err(error) => {
                drop(process);
                session.close();
                Err(pty_error("assign child", "pty_job_assign_failed", error))
            }
        }
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
pub struct PtyIo<'a>(&'a OutputPipe);

impl PtyIo<'_> {
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

#[derive(Debug)]
pub struct PtyMaster {
    session: Arc<ConptySession>,
}

impl PtyMaster {
    pub fn resize(&self, size: TerminalSize) -> PtyResult<()> {
        self.session
            .resize(size)
            .map_err(|error| pty_error("resize", "pty_resize_failed", error))
    }

    pub fn try_clone_for_startup_reader(&mut self) -> PtyResult<Self> {
        // The backend owns one synchronous output reader. Clones share its
        // bounded lossless queue rather than duplicating the OS handle, which
        // gives shutdown one output-pipe close authority.
        Ok(Self {
            session: Arc::clone(&self.session),
        })
    }

    #[must_use]
    pub fn io(&self) -> PtyIo<'_> {
        PtyIo(&self.session.output)
    }

    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.session.input.write_all(bytes, PTY_WRITE_TIMEOUT)
    }
}

#[derive(Debug)]
pub struct PtyChild {
    process: OwnedHandle,
    job: Option<JobObjectGuard>,
    session: Arc<ConptySession>,
    pid: ProcessId,
}

impl PtyChild {
    #[must_use]
    pub fn pid(&self) -> ProcessId {
        self.pid
    }

    pub fn wait(&mut self) -> PtyResult<ExitStatus> {
        let wait = unsafe {
            // SAFETY: process is a live handle owned by this child; waiting
            // does not transfer or invalidate it.
            WaitForSingleObject(self.process.as_raw_handle() as HANDLE, INFINITE)
        };
        if wait == WAIT_FAILED {
            return Err(pty_error("wait", "pty_wait_failed", last_os_error()));
        }
        if wait != WAIT_OBJECT_0 {
            return Err(pty_error(
                "wait",
                "pty_wait_failed",
                io::Error::other(format!("unexpected wait status {wait}")),
            ));
        }
        exit_status(&self.process).map_err(|error| pty_error("wait", "pty_wait_failed", error))
    }

    pub fn try_clone_for_wait(&self) -> PtyResult<Self> {
        let process = duplicate_handle(&self.process)
            .map_err(|error| pty_error("clone wait handle", "pty_wait_clone_failed", error))?;
        Ok(Self {
            process,
            job: None,
            session: Arc::clone(&self.session),
            pid: self.pid,
        })
    }

    pub fn close_pseudoconsole(&self) {
        self.session.close();
    }

    pub fn terminate_forcefully(&self) -> PtyResult<()> {
        let Some(job) = &self.job else {
            return Err(PtyError::failed(
                "terminate",
                "pty_terminate_without_job",
                "Windows child has no Job Object cleanup owner",
            ));
        };
        job.terminate(1)
            .map_err(|error| pty_error("terminate", "pty_terminate_failed", error))?;
        match wait_process_state(&self.process) {
            ProcessWaitState::Exited => {}
            ProcessWaitState::Running => {
                terminate_process(&self.process, 1)
                    .map_err(|error| pty_error("terminate", "pty_terminate_failed", error))?;
            }
            ProcessWaitState::Failed(code) => {
                return Err(pty_error(
                    "terminate",
                    "pty_terminate_failed",
                    io::Error::from_raw_os_error(code as i32),
                ));
            }
        }
        self.session.close();
        Ok(())
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
        let _attachment =
            super::console::ConsoleGuard::attach_process(self.pid.as_u32()).map_err(|error| {
                PtyError::failed("send native key", "pty_console_attach_failed", error)
            })?;
        let input = open_console_input().map_err(|error| {
            PtyError::failed("send native key", "pty_console_input_open_failed", error)
        })?;
        write_console_key(
            input.as_raw_handle() as HANDLE,
            native_console_key_event(key, repeat_count),
        )
        .map_err(|error| PtyError::failed("send native key", "pty_native_key_failed", error))
    }

    /// Queries the target child's console mode without guessing from terminal
    /// output or from the host process's own console state.
    pub fn native_input_ownership(&self) -> PtyResult<NativeInputOwnership> {
        let _attachment =
            super::console::ConsoleGuard::attach_process(self.pid.as_u32()).map_err(|error| {
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

impl Drop for PtyChild {
    fn drop(&mut self) {
        let Some(job) = &self.job else {
            return;
        };

        // Keep the explicit owner ordering even though KILL_ON_JOB_CLOSE is
        // the final safety net: terminate the tree first, then close HPCON
        // while the independent output pump is still draining it.
        let _ = job.terminate(1);
        if wait_process_state(&self.process) == ProcessWaitState::Running {
            let _ = terminate_process(&self.process, 1);
        }
        self.session.close();
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
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(last_os_error());
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
        return Err(last_os_error());
    }
    Ok(mode)
}

const fn native_console_key_event(
    key: NativeTerminalKey,
    repeat_count: u16,
) -> WindowsConsoleKeyEvent {
    let (virtual_key_code, virtual_scan_code) = match key {
        NativeTerminalKey::Up => (0x26, 0x48),
        NativeTerminalKey::Down => (0x28, 0x50),
    };
    WindowsConsoleKeyEvent {
        virtual_key_code,
        virtual_scan_code,
        unicode_char: 0,
        control_key_state: ENHANCED_KEY,
        repeat_count,
    }
}

fn write_console_key(handle: HANDLE, key: WindowsConsoleKeyEvent) -> io::Result<()> {
    let records = [key_input_record(key, true), key_input_record(key, false)];
    let mut written = 0u32;
    let succeeded = unsafe {
        // SAFETY: handle is an open console input handle and records points to
        // two initialized INPUT_RECORD values for the duration of the call.
        WriteConsoleInputW(handle, records.as_ptr(), records.len() as u32, &mut written)
    };
    if succeeded == 0 {
        return Err(last_os_error());
    }
    if written != records.len() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!(
                "WriteConsoleInputW wrote {written} of {} records",
                records.len()
            ),
        ));
    }
    Ok(())
}

fn key_input_record(key: WindowsConsoleKeyEvent, key_down: bool) -> INPUT_RECORD {
    INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: i32::from(key_down),
                wRepeatCount: if key_down { key.repeat_count.max(1) } else { 1 },
                wVirtualKeyCode: key.virtual_key_code,
                wVirtualScanCode: key.virtual_scan_code,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: key.unicode_char,
                },
                dwControlKeyState: key.control_key_state,
            },
        },
    }
}

#[derive(Debug)]
/// One terminal session, whichever backend is providing it.
///
/// The name is historical: on a system with ConPTY this owns an `HPCON`, and
/// on one without it owns an agent process that emulates one. Both speak the
/// same two pipes, so everything outside this file is identical either way —
/// which is the property that keeps the fallback from leaking into the
/// terminal.
struct ConptySession {
    /// `Some` only for the ConPTY backend.
    hpc: Mutex<Option<HPCON>>,
    /// `Some` only for the agent backend: the pipe that carries resizes.
    agent: Option<Mutex<Option<OwnedHandle>>>,
    output: Arc<OutputPipe>,
    input: Arc<InputWriter>,
    passthrough: bool,
}

impl ConptySession {
    fn hpc(&self) -> io::Result<HPCON> {
        let guard = self
            .hpc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .as_ref()
            .copied()
            .ok_or_else(|| io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32))
    }

    fn resize(&self, size: TerminalSize) -> io::Result<()> {
        let coord = coord_from_size(size)?;
        if let Some(agent) = &self.agent {
            let guard = agent.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            // A resize racing teardown is benign here for the same reason it
            // is below: the session is going away and the new size with it.
            let Some(control) = guard.as_ref() else {
                return Ok(());
            };
            return console_agent::request_resize(control, size.cols, size.rows);
        }
        let guard = self
            .hpc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(hpc) = guard.as_ref().copied() else {
            // A resize racing with normal child teardown is benign. The
            // previous backend receives E_HANDLE from ResizePseudoConsole and
            // treats the same terminal-exit state as success.
            return Ok(());
        };
        let result = unsafe {
            // SAFETY: the lock keeps the sole close authority from consuming
            // hpc through this call, and coord has been range checked.
            conpty::resize(hpc, coord)
        };
        // Holding a live HPCON means resolution already succeeded, so `None`
        // cannot occur here; treat it as the same benign no-op as a resize
        // that raced teardown rather than inventing a failure.
        let Some(result) = result else { return Ok(()) };
        if result != S_OK {
            if is_benign_resize_after_exit(result) {
                return Ok(());
            }
            return Err(hresult_error(result));
        }
        Ok(())
    }

    fn uses_passthrough(&self) -> bool {
        self.passthrough
    }

    fn take_hpc(&self) -> Option<HPCON> {
        let mut guard = self
            .hpc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.take()
    }

    fn close(&self) {
        // ClosePseudoConsole blocks on pre-24H2 Windows until clients stop
        // writing. Switch the independent output pump to discard mode first;
        // it continues its single synchronous ReadFile through this call and
        // never waits on the product's bounded consumer queue.
        self.input.close();
        self.output.begin_drain();
        if let Some(agent) = &self.agent {
            // Dropping the control pipe is what ends the agent's control
            // thread. The agent itself exits with its child, and the host's
            // Job Object is the authority that kills both if it does not.
            let mut guard = agent.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            drop(guard.take());
        }
        if let Some(hpc) = self.take_hpc() {
            // SAFETY: take_hpc is the sole transfer point for the live HPCON;
            // this call consumes no Rust-owned memory and is idempotent because
            // subsequent callers observe None.
            unsafe { conpty::close(hpc) };
        }
    }
}

impl Drop for ConptySession {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
struct OutputPipe {
    state: Mutex<OutputState>,
    wake: Condvar,
}

#[derive(Debug)]
struct OutputState {
    queue: VecDeque<u8>,
    draining: bool,
    eof: bool,
    error: Option<io::Error>,
}

impl OutputPipe {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputState {
                queue: VecDeque::with_capacity(OUTPUT_QUEUE_CAPACITY),
                draining: false,
                eof: false,
                error: None,
            }),
            wake: Condvar::new(),
        }
    }

    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if !state.queue.is_empty() {
                let amount = state.queue.len().min(buffer.len());
                {
                    let (first, second) = state.queue.as_slices();
                    let first_amount = amount.min(first.len());
                    buffer[..first_amount].copy_from_slice(&first[..first_amount]);
                    let second_amount = amount - first_amount;
                    if second_amount != 0 {
                        buffer[first_amount..amount].copy_from_slice(&second[..second_amount]);
                    }
                }
                state.queue.drain(..amount);
                self.wake.notify_all();
                return Ok(amount);
            }
            if let Some(error) = state.error.take() {
                return Err(error);
            }
            if state.eof {
                return Ok(0);
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn push(&self, mut bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !bytes.is_empty() {
            while state.queue.len() == OUTPUT_QUEUE_CAPACITY && !state.draining && !state.eof {
                state = self
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if state.draining || state.eof {
                // During shutdown the pump must keep reading the OS pipe, but
                // it must not wait for a consumer that may already be gone.
                return;
            }
            let available = OUTPUT_QUEUE_CAPACITY - state.queue.len();
            let amount = available.min(bytes.len());
            state.queue.extend(&bytes[..amount]);
            bytes = &bytes[amount..];
            self.wake.notify_all();
        }
    }

    fn begin_drain(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.draining = true;
        self.wake.notify_all();
    }

    fn finish(&self, error: Option<io::Error>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.error.is_none() {
            state.error = error;
        }
        state.eof = true;
        self.wake.notify_all();
    }
}

#[derive(Debug)]
struct InputWriter {
    handle: OwnedHandle,
    event: OwnedHandle,
    operation: Mutex<()>,
    closed: AtomicBool,
}

impl InputWriter {
    fn new(handle: OwnedHandle) -> io::Result<Self> {
        let event = unsafe {
            // SAFETY: null security attributes and name request default event
            // security; the event starts nonsignaled and is reused serially.
            CreateEventW(null(), 1, 0, null())
        };
        if event.is_null() {
            return Err(last_os_error());
        }
        Ok(Self {
            handle,
            event: unsafe {
                // SAFETY: CreateEventW returned a unique owned handle.
                OwnedHandle::from_raw_handle(event as _)
            },
            operation: Mutex::new(()),
            closed: AtomicBool::new(false),
        })
    }

    fn write_all(&self, mut bytes: &[u8], timeout: Duration) -> io::Result<()> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| io::Error::other("ConPTY input writer mutex poisoned"))?;
        let mut last_progress = Instant::now();
        while !bytes.is_empty() {
            if self.closed.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "ConPTY input writer is closed",
                ));
            }
            let remaining = timeout.saturating_sub(last_progress.elapsed());
            if remaining.is_zero() {
                return Err(write_timeout(timeout));
            }
            let amount = bytes.len().min(WRITE_CHUNK_SIZE);
            let written = self.write_chunk(&bytes[..amount], remaining, timeout)?;
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "ConPTY input pipe accepted zero bytes",
                ));
            }
            bytes = &bytes[written..];
            last_progress = Instant::now();
        }
        Ok(())
    }

    fn write_chunk(
        &self,
        bytes: &[u8],
        remaining: Duration,
        timeout: Duration,
    ) -> io::Result<usize> {
        let reset = unsafe {
            // SAFETY: event is live and all writes are serialized by operation.
            ResetEvent(self.event.as_raw_handle() as HANDLE)
        };
        if reset == 0 {
            return Err(last_os_error());
        }

        let mut overlapped = OVERLAPPED {
            hEvent: self.event.as_raw_handle() as HANDLE,
            ..OVERLAPPED::default()
        };
        let started = unsafe {
            // SAFETY: the named-pipe client handle was opened with
            // FILE_FLAG_OVERLAPPED; bytes and overlapped remain alive through
            // completion or cancellation drain.
            WriteFile(
                self.handle.as_raw_handle() as HANDLE,
                bytes.as_ptr().cast(),
                u32::try_from(bytes.len()).expect("write chunk fits DWORD"),
                null_mut(),
                &mut overlapped,
            )
        };
        if started != 0 {
            return self.completed_transfer(&overlapped);
        }

        let error = last_error_code();
        if error != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let wait = unsafe {
            // SAFETY: event belongs to this pending operation and remains live.
            WaitForSingleObject(
                self.event.as_raw_handle() as HANDLE,
                wait_timeout_millis(remaining),
            )
        };
        match wait {
            WAIT_OBJECT_0 => self.completed_transfer(&overlapped),
            WAIT_TIMEOUT => match self.cancel_and_drain(&overlapped)? {
                CancellationOutcome::Completed(amount) => Ok(amount),
                CancellationOutcome::Cancelled => Err(write_timeout(timeout)),
            },
            WAIT_FAILED => {
                let wait_error = last_os_error();
                self.cancel_and_drain(&overlapped)?;
                Err(wait_error)
            }
            status => {
                self.cancel_and_drain(&overlapped)?;
                Err(io::Error::other(format!(
                    "unexpected ConPTY write wait status {status}"
                )))
            }
        }
    }

    fn completed_transfer(&self, overlapped: &OVERLAPPED) -> io::Result<usize> {
        let mut transferred = 0u32;
        let completed = unsafe {
            // SAFETY: overlapped identifies this operation and remains alive
            // until the result is collected.
            GetOverlappedResult(
                self.handle.as_raw_handle() as HANDLE,
                overlapped,
                &mut transferred,
                0,
            )
        };
        if completed == 0 {
            return Err(last_os_error());
        }
        Ok(transferred as usize)
    }

    fn cancel_and_drain(&self, overlapped: &OVERLAPPED) -> io::Result<CancellationOutcome> {
        let cancelled = unsafe {
            // SAFETY: overlapped identifies the live operation issued on this
            // handle; cancellation is scoped to that operation.
            CancelIoEx(self.handle.as_raw_handle() as HANDLE, overlapped)
        };
        let cancel_error = if cancelled == 0 {
            let code = last_error_code();
            (code != ERROR_NOT_FOUND).then_some(io::Error::from_raw_os_error(code as i32))
        } else {
            None
        };
        let mut transferred = 0u32;
        let completed = unsafe {
            // SAFETY: this wait drains the canceled operation before the
            // caller reuses its stack OVERLAPPED and input buffer.
            GetOverlappedResult(
                self.handle.as_raw_handle() as HANDLE,
                overlapped,
                &mut transferred,
                1,
            )
        };
        if completed == 0 {
            let code = last_error_code();
            if code == ERROR_OPERATION_ABORTED {
                if let Some(error) = cancel_error {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("failed to cancel timed-out ConPTY write: {error}"),
                    ));
                }
                return Ok(CancellationOutcome::Cancelled);
            }
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(CancellationOutcome::Completed(transferred as usize))
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        unsafe {
            // SAFETY: the handle remains owned by self. A null OVERLAPPED
            // requests cancellation of any pending write; its writer still
            // drains GetOverlappedResult before releasing stack buffers.
            let _ = CancelIoEx(self.handle.as_raw_handle() as HANDLE, null());
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CancellationOutcome {
    Completed(usize),
    Cancelled,
}

#[derive(Debug)]
struct DsrBootstrap {
    deadline: Instant,
    completed: bool,
    pending: Vec<u8>,
}

const DSR_REQUEST: &[u8] = b"\x1b[6n";
const DSR_RESPONSE: &[u8] = b"\x1b[1;1R";

impl DsrBootstrap {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + DSR_BOOTSTRAP_TIMEOUT,
            completed: false,
            pending: Vec::new(),
        }
    }

    fn filter(&mut self, bytes: &[u8]) -> (Vec<u8>, Option<&'static [u8]>) {
        if self.completed || Instant::now() > self.deadline {
            self.completed = true;
            let mut output = std::mem::take(&mut self.pending);
            output.extend_from_slice(bytes);
            return (output, None);
        }

        let mut combined = Vec::with_capacity(self.pending.len() + bytes.len());
        combined.extend_from_slice(&self.pending);
        combined.extend_from_slice(bytes);
        if let Some(offset) = find_subslice(&combined, DSR_REQUEST) {
            self.completed = true;
            self.pending.clear();
            let mut output = Vec::with_capacity(combined.len() - DSR_REQUEST.len());
            output.extend_from_slice(&combined[..offset]);
            output.extend_from_slice(&combined[offset + DSR_REQUEST.len()..]);
            return (output, Some(DSR_RESPONSE));
        }

        let pending_len = partial_dsr_prefix_len(&combined);
        self.pending.clear();
        self.pending
            .extend_from_slice(&combined[combined.len() - pending_len..]);
        (combined[..combined.len() - pending_len].to_vec(), None)
    }

    fn flush(&mut self) -> Vec<u8> {
        self.completed = true;
        std::mem::take(&mut self.pending)
    }
}

fn should_enable_dsr_bootstrap(program: &Path) -> bool {
    let Some(name) = program.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "pwsh.exe" | "powershell.exe"
    )
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn partial_dsr_prefix_len(bytes: &[u8]) -> usize {
    (1..DSR_REQUEST.len())
        .rev()
        .find(|length| bytes.ends_with(&DSR_REQUEST[..*length]))
        .unwrap_or(0)
}

fn output_pump(
    output_read: OwnedHandle,
    output: Arc<OutputPipe>,
    input: Arc<InputWriter>,
    mut dsr: Option<DsrBootstrap>,
) {
    let mut buffer = [0u8; OUTPUT_READ_SIZE];
    loop {
        match read_pipe(&output_read, &mut buffer) {
            Ok(0) => {
                if let Some(dsr) = dsr.as_mut() {
                    let tail = dsr.flush();
                    output.push(&tail);
                }
                output.finish(None);
                return;
            }
            Ok(amount) => {
                let (filtered, response) = match dsr.as_mut() {
                    Some(dsr) => dsr.filter(&buffer[..amount]),
                    None => (buffer[..amount].to_vec(), None),
                };
                if dsr.as_ref().is_some_and(|bootstrap| bootstrap.completed) {
                    dsr = None;
                }
                if let Some(response) = response
                    && let Err(error) = input.write_all(response, PTY_WRITE_TIMEOUT)
                {
                    output.finish(Some(error));
                    return;
                }
                output.push(&filtered);
            }
            Err(error) => {
                // ERROR_BROKEN_PIPE and ERROR_HANDLE_EOF are converted to
                // normal EOF by read_pipe. Other errors terminate this one
                // pump and release its sole read handle before HPCON close.
                output.finish(Some(error));
                return;
            }
        }
    }
}

fn spawn_output_pump(
    output_read: OwnedHandle,
    output: Arc<OutputPipe>,
    input: Arc<InputWriter>,
    dsr: Option<DsrBootstrap>,
) -> io::Result<()> {
    crate::threading::spawn_named_detached(
        "agenterm-conpty-output",
        Box::new(move || output_pump(output_read, output, input, dsr)),
    )
}

#[derive(Debug)]
struct PipePair {
    read: OwnedHandle,
    write: OwnedHandle,
}

fn create_output_pipe(buffer_size: u32) -> io::Result<PipePair> {
    let mut read = null_mut();
    let mut write = null_mut();
    let created = unsafe {
        // SAFETY: read and write are valid output slots; null security
        // attributes create handles owned by this process.
        CreatePipe(&mut read, &mut write, null(), buffer_size)
    };
    if created == 0 {
        return Err(last_os_error());
    }
    Ok(PipePair {
        read: unsafe {
            // SAFETY: CreatePipe succeeded and transferred this unique handle.
            OwnedHandle::from_raw_handle(read as _)
        },
        write: unsafe {
            // SAFETY: CreatePipe succeeded and transferred this unique handle.
            OwnedHandle::from_raw_handle(write as _)
        },
    })
}

fn create_input_pipe(buffer_size: u32) -> io::Result<PipePair> {
    let id = NEXT_CONPTY_PIPE_ID.fetch_add(1, Ordering::Relaxed);
    let process_id = unsafe {
        // SAFETY: GetCurrentProcessId has no preconditions.
        GetCurrentProcessId()
    };
    let name = format!(r"\\.\pipe\agenterm-conpty-input-{process_id}-{id}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let read = unsafe {
        // SAFETY: name is NUL-terminated, the pipe mode is byte-oriented, and
        // the returned server handle is unique on success.
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            buffer_size,
            0,
            null(),
        )
    };
    if read == INVALID_HANDLE_VALUE {
        return Err(last_os_error());
    }
    let read = unsafe {
        // SAFETY: CreateNamedPipeW returned a unique owned server handle.
        OwnedHandle::from_raw_handle(read as _)
    };
    let write = unsafe {
        // SAFETY: name remains live and NUL-terminated; the client is opened
        // for overlapped writes so they can be canceled on timeout.
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    if write.is_null() || write == INVALID_HANDLE_VALUE {
        return Err(last_os_error());
    }
    Ok(PipePair {
        read,
        write: unsafe {
            // SAFETY: CreateFileW returned a unique owned client handle.
            OwnedHandle::from_raw_handle(write as _)
        },
    })
}

fn create_session(size: TerminalSize, dsr_bootstrap: bool) -> io::Result<Arc<ConptySession>> {
    let selected = selected_conpty_flags();
    match create_session_with_flags(size, selected, dsr_bootstrap) {
        Ok(session) => Ok(session),
        Err(error) if selected & PSEUDOCONSOLE_PASSTHROUGH_MODE != 0 => {
            match create_session_with_flags(size, base_conpty_flags(), dsr_bootstrap) {
                Ok(session) => Ok(session),
                Err(_) => create_session_with_flags(size, 0, dsr_bootstrap).or(Err(error)),
            }
        }
        Err(error) if selected != 0 => {
            create_session_with_flags(size, 0, dsr_bootstrap).or(Err(error))
        }
        Err(error) => Err(error),
    }
}

fn create_session_with_flags(
    size: TerminalSize,
    flags: u32,
    dsr_bootstrap: bool,
) -> io::Result<Arc<ConptySession>> {
    let coord = coord_from_size(size)?;
    let input = create_input_pipe(PIPE_BUFFER_SIZE)?;
    let output = create_output_pipe(PIPE_BUFFER_SIZE)?;
    let hpc = create_pseudo_console(coord, &input.read, &output.write, flags)?;

    let PipePair {
        read: input_read,
        write: input_write,
    } = input;
    let PipePair {
        read: output_read,
        write: output_write,
    } = output;
    // CreatePseudoConsole duplicates/owns the two endpoints supplied to it;
    // the parent retains only the input writer and output reader.
    drop(input_read);
    drop(output_write);

    let input = match InputWriter::new(input_write) {
        Ok(input) => Arc::new(input),
        Err(error) => {
            // The output read endpoint must be released before the HPCON guard
            // calls ClosePseudoConsole, otherwise an early construction error
            // could recreate the documented old-Windows close wait.
            drop(output_read);
            drop(hpc);
            return Err(error);
        }
    };
    let output = Arc::new(OutputPipe::new());
    let dsr = dsr_bootstrap.then(DsrBootstrap::new);
    if let Err(error) = spawn_output_pump(output_read, Arc::clone(&output), Arc::clone(&input), dsr)
    {
        // On thread creation failure, the moved output handle is dropped with
        // the rejected closure before the HPCON owner is dropped.
        drop(input);
        drop(hpc);
        return Err(error);
    }
    let passthrough = flags & PSEUDOCONSOLE_PASSTHROUGH_MODE != 0;
    Ok(Arc::new(ConptySession {
        hpc: Mutex::new(Some(hpc.into_raw())),
        agent: None,
        output,
        input,
        passthrough,
    }))
}

fn create_pseudo_console(
    coord: COORD,
    input: &OwnedHandle,
    output: &OwnedHandle,
    flags: u32,
) -> io::Result<HpcOwner> {
    let mut hpc: HPCON = 0;
    let result = unsafe {
        // SAFETY: both pipe handles are live and borrowed for this call;
        // coord was range checked and hpc is a valid out-pointer.
        conpty::create(
            coord,
            input.as_raw_handle() as HANDLE,
            output.as_raw_handle() as HANDLE,
            flags,
            &mut hpc,
        )
    };
    // The one place an absent ConPTY becomes visible: an ordinary refusal from
    // the operation that needs it, rather than a loader error before `main`.
    let Some(result) = result else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            conpty::unavailable_reason(),
        ));
    };
    if result != S_OK {
        return Err(hresult_error(result));
    }
    if hpc == 0 {
        return Err(io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32));
    }
    Ok(HpcOwner(Some(hpc)))
}

#[derive(Debug)]
struct HpcOwner(Option<HPCON>);

impl HpcOwner {
    fn into_raw(mut self) -> HPCON {
        self.0
            .take()
            .expect("HPCON owner must contain a live handle")
    }
}

impl Drop for HpcOwner {
    fn drop(&mut self) {
        if let Some(hpc) = self.0.take() {
            // Construction failures drop pipe readers before reaching this
            // point; normal sessions use ConptySession::close as the single
            // idempotent close authority instead.
            unsafe { conpty::close(hpc) };
        }
    }
}

fn coord_from_size(size: TerminalSize) -> io::Result<COORD> {
    if size.cols == 0 || size.rows == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal size must be at least 1x1",
        ));
    }
    let cols = i16::try_from(size.cols).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal column count exceeds Windows COORD range",
        )
    })?;
    let rows = i16::try_from(size.rows).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal row count exceeds Windows COORD range",
        )
    })?;
    Ok(COORD { X: cols, Y: rows })
}

fn selected_conpty_flags() -> u32 {
    select_conpty_flags(current_windows_build().ok(), false)
}

fn select_conpty_flags(build: Option<u32>, disabled: bool) -> u32 {
    if !disabled && build.is_some_and(|build| build >= PASSTHROUGH_MIN_BUILD) {
        base_conpty_flags() | PSEUDOCONSOLE_PASSTHROUGH_MODE
    } else {
        base_conpty_flags()
    }
}

fn current_windows_build() -> io::Result<u32> {
    let mut info = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
        ..OSVERSIONINFOEXW::default()
    };
    let status = unsafe {
        // SAFETY: info is initialized writable storage with the exact size
        // required by RtlGetVersion; the function only writes this structure.
        RtlGetVersion(&mut info)
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    Ok(info.dwBuildNumber)
}

const fn base_conpty_flags() -> u32 {
    PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE
}

fn create_suspended_process_with_fallback(
    command: &ChildCommand,
    session: &mut Arc<ConptySession>,
    size: TerminalSize,
    dsr_bootstrap: bool,
    extra_creation_flags: u32,
) -> io::Result<SuspendedProcess> {
    match create_suspended_process(command, session, extra_creation_flags) {
        Err(error)
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
                && session.uses_passthrough() =>
        {
            session.close();
            *session = create_session_with_flags(size, base_conpty_flags(), dsr_bootstrap)?;
            create_suspended_process(command, session, extra_creation_flags)
        }
        result => result,
    }
}

fn create_suspended_process(
    command: &ChildCommand,
    session: &Arc<ConptySession>,
    extra_creation_flags: u32,
) -> io::Result<SuspendedProcess> {
    let mut attributes = AttributeList::with_pseudoconsole(session.hpc()?)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.as_mut_ptr();

    let application_path = resolve_application_path(command)?;
    let application = wide_null(application_path.as_os_str())?;
    let mut command_line = command_line(command)?;
    let mut environment = environment_block(command)?;
    let current_dir = command
        .current_dir
        .as_ref()
        .map(|path| wide_null(path.as_os_str()))
        .transpose()?;
    let mut process_info = PROCESS_INFORMATION::default();

    ensure_child_processes_inherit_ctrl_c();
    let created = unsafe {
        // SAFETY: all UTF-16 buffers are NUL-terminated and live through the
        // call; startup, attributes, and process_info are initialized; handle
        // inheritance is disabled.
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_SUSPENDED
                | extra_creation_flags,
            environment
                .as_mut()
                .map_or(null(), |block| block.as_mut_ptr().cast()),
            current_dir.as_ref().map_or(null(), |path| path.as_ptr()),
            &startup.StartupInfo as *const STARTUPINFOW,
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(last_os_error());
    }
    let process = unsafe {
        // SAFETY: CreateProcessW succeeded and transferred this unique handle.
        OwnedHandle::from_raw_handle(process_info.hProcess as _)
    };
    let thread = unsafe {
        // SAFETY: CreateProcessW succeeded and transferred this unique handle.
        OwnedHandle::from_raw_handle(process_info.hThread as _)
    };
    Ok(SuspendedProcess {
        process: Some(process),
        thread: Some(thread),
        pid: process_info.dwProcessId,
        armed: true,
    })
}

fn ensure_child_processes_inherit_ctrl_c() {
    unsafe {
        // SAFETY: clearing the inherited Ctrl-C-ignore bit affects only this
        // process and is required before spawning the ConPTY child.
        let _ = SetConsoleCtrlHandler(None, 0);
    }
}

#[derive(Debug)]
struct SuspendedProcess {
    process: Option<OwnedHandle>,
    thread: Option<OwnedHandle>,
    pid: u32,
    armed: bool,
}

impl SuspendedProcess {
    fn process_handle(&self) -> &OwnedHandle {
        self.process
            .as_ref()
            .expect("suspended process handle is live until transfer")
    }
}

impl Drop for SuspendedProcess {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(process) = self.process.as_ref() {
            let _ = terminate_process(process, 1);
            let _ = unsafe {
                // SAFETY: process remains owned while the partial-creation
                // guard waits briefly for TerminateProcess to take effect.
                WaitForSingleObject(process.as_raw_handle() as HANDLE, 500)
            };
        }
    }
}

/// Selects the pre-ConPTY backend on a machine that has ConPTY.
///
/// Without this the fallback could only ever be exercised on a system old
/// enough to need it, which is neither CI nor any developer's machine — the
/// exact shape that let the original load-time defect ship. Reading the
/// environment on every spawn is deliberate: a cached answer could not be
/// changed between the tests in one process.
fn force_console_agent() -> bool {
    env::var_os("AGENTERM_FORCE_CONSOLE_AGENT").is_some_and(|value| value == "1")
}

/// The pre-ConPTY path: an agent process stands in for the pseudoconsole.
///
/// Deliberately built from the same pieces as the ConPTY path — the same
/// pipes, the same output pump, the same command line and environment block —
/// so the two backends cannot drift into starting a child from different
/// inputs. What differs is only who creates the child: here the agent does,
/// because the child must be born attached to the agent's hidden console.
fn spawn_through_console_agent(command: ChildCommand, size: TerminalSize) -> PtyResult<SpawnedPty> {
    let job = JobObjectGuard::new()
        .map_err(|error| pty_error("create job", "pty_job_create_failed", error))?;
    let input = create_input_pipe(PIPE_BUFFER_SIZE)
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;
    let output = create_output_pipe(PIPE_BUFFER_SIZE)
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;

    let child_command_line =
        command_line(&command).map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;
    let environment = environment_block(&command)
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;
    let current_dir = command
        .current_dir
        .as_ref()
        .map(|path| wide_null(path.as_os_str()))
        .transpose()
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;

    let agent = console_agent::spawn_agent(
        &input.read,
        &output.write,
        &child_command_line,
        environment.as_deref(),
        current_dir.as_deref(),
        size.cols,
        size.rows,
    )
    .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;

    // The agent owns duplicates of both endpoints now; the host keeps only
    // the writer it feeds and the reader it drains, exactly as with ConPTY.
    drop(input.read);
    drop(output.write);

    let writer = InputWriter::new(input.write)
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", error))?;
    let session = Arc::new(ConptySession {
        hpc: Mutex::new(None),
        agent: Some(Mutex::new(Some(agent.control))),
        output: Arc::new(OutputPipe::new()),
        input: Arc::new(writer),
        // The agent emits the sequences it decides to emit; there is no
        // passthrough mode to inherit from a pseudoconsole.
        passthrough: false,
    });
    if let Err(error) = spawn_output_pump(
        output.read,
        Arc::clone(&session.output),
        Arc::clone(&session.input),
        None,
    ) {
        session.close();
        return Err(pty_error("spawn", "pty_spawn_failed", error));
    }

    // The agent is the process the host waits on and kills: it exits with its
    // child's status, and its own job kills the child if the host kills it.
    let pid = ProcessId::new(agent.pid)
        .map_err(|error| pty_error("spawn", "pty_spawn_failed", io::Error::other(error)))?;
    if let Err(error) = job.assign(&agent.process) {
        session.close();
        return Err(pty_error("spawn", "pty_job_assign_failed", error));
    }
    Ok(SpawnedPty {
        master: PtyMaster {
            session: Arc::clone(&session),
        },
        child: PtyChild {
            process: agent.process,
            job: Some(job),
            session,
            pid,
        },
    })
}

fn resume_as_child(
    mut process: SuspendedProcess,
    job: JobObjectGuard,
    session: Arc<ConptySession>,
) -> PtyResult<SpawnedPty> {
    let resumed = unsafe {
        // SAFETY: process.thread is the primary thread returned suspended by
        // CreateProcessW and remains owned here.
        ResumeThread(
            process
                .thread
                .as_ref()
                .expect("primary thread is live until transfer")
                .as_raw_handle() as HANDLE,
        )
    };
    if resumed == u32::MAX {
        drop(process);
        drop(job);
        session.close();
        return Err(pty_error(
            "resume child",
            "pty_resume_failed",
            last_os_error(),
        ));
    }
    let pid = match ProcessId::new(process.pid) {
        Ok(pid) => pid,
        Err(error) => {
            drop(process);
            drop(job);
            session.close();
            return Err(pty_error("spawn", "pty_spawn_invalid_pid", error));
        }
    };
    process.armed = false;
    let process_handle = process
        .process
        .take()
        .expect("process handle transfers exactly once");
    drop(
        process
            .thread
            .take()
            .expect("primary thread handle transfers exactly once"),
    );
    Ok(SpawnedPty {
        master: PtyMaster {
            session: Arc::clone(&session),
        },
        child: PtyChild {
            process: process_handle,
            job: Some(job),
            session,
            pid,
        },
    })
}

#[derive(Debug)]
struct JobObjectGuard {
    handle: OwnedHandle,
}

impl JobObjectGuard {
    fn new() -> io::Result<Self> {
        let handle = unsafe {
            // SAFETY: null security attributes and name request an unnamed job;
            // ownership is transferred only after the handle is checked.
            CreateJobObjectW(null(), null())
        };
        if handle.is_null() {
            return Err(last_os_error());
        }
        let handle = unsafe {
            // SAFETY: CreateJobObjectW returned a unique owned handle.
            OwnedHandle::from_raw_handle(handle as _)
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            // SAFETY: handle is live and limits points to the initialized
            // structure for the duration of the call.
            SetInformationJobObject(
                handle.as_raw_handle() as HANDLE,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(last_os_error());
        }
        Ok(Self { handle })
    }

    fn assign(&self, process: &OwnedHandle) -> io::Result<()> {
        let assigned = unsafe {
            // SAFETY: both handles are live and borrowed; assignment transfers
            // no Rust handle ownership.
            AssignProcessToJobObject(
                self.handle.as_raw_handle() as HANDLE,
                process.as_raw_handle() as HANDLE,
            )
        };
        if assigned == 0 {
            return Err(last_os_error());
        }
        Ok(())
    }

    fn terminate(&self, exit_code: u32) -> io::Result<()> {
        let terminated = unsafe {
            // SAFETY: this job handle is live and owned by the guard.
            TerminateJobObject(self.handle.as_raw_handle() as HANDLE, exit_code)
        };
        if terminated == 0 {
            return Err(last_os_error());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct AttributeList {
    storage: Vec<usize>,
}

impl AttributeList {
    fn with_pseudoconsole(hpc: HPCON) -> io::Result<Self> {
        let mut bytes = 0usize;
        unsafe {
            // SAFETY: this is the documented sizing probe; the null list is
            // intentional and bytes is a valid out-pointer.
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(last_os_error());
        }
        let slots = bytes.checked_add(size_of::<usize>() - 1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "attribute list too large")
        })? / size_of::<usize>();
        let mut storage = vec![0usize; slots];
        let list = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) } == 0 {
            return Err(last_os_error());
        }
        let updated = unsafe {
            // SAFETY: list is initialized; the HPCON value is passed in the
            // ABI form required by PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE.
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                hpc as *mut core::ffi::c_void,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            unsafe {
                // SAFETY: list was initialized successfully above.
                DeleteProcThreadAttributeList(list);
            }
            return Err(last_os_error());
        }
        Ok(Self { storage })
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: AttributeList exists only after successful initialization
            // and owns this storage until Drop.
            DeleteProcThreadAttributeList(self.as_mut_ptr());
        }
    }
}

fn read_pipe(handle: &OwnedHandle, buffer: &mut [u8]) -> io::Result<usize> {
    let length = u32::try_from(buffer.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "read buffer exceeds Windows DWORD length",
        )
    })?;
    if length == 0 {
        return Ok(0);
    }
    let mut read = 0u32;
    let ok = unsafe {
        // SAFETY: handle is live, buffer is writable for length bytes, and the
        // synchronous call receives a valid byte-count out-pointer.
        ReadFile(
            handle.as_raw_handle() as HANDLE,
            buffer.as_mut_ptr().cast(),
            length,
            &mut read,
            null_mut(),
        )
    };
    if ok == 0 {
        let code = last_error_code();
        if matches!(code, ERROR_BROKEN_PIPE | ERROR_HANDLE_EOF) {
            return Ok(0);
        }
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    Ok(read as usize)
}

fn duplicate_handle(handle: &OwnedHandle) -> io::Result<OwnedHandle> {
    let current = unsafe {
        // SAFETY: GetCurrentProcess returns the current-process pseudo-handle.
        GetCurrentProcess()
    };
    let mut duplicate: HANDLE = null_mut();
    let ok = unsafe {
        // SAFETY: source and destination are this process, duplicate is a valid
        // out-pointer, and the source handle remains live through the call.
        DuplicateHandle(
            current,
            handle.as_raw_handle() as HANDLE,
            current,
            &mut duplicate,
            0,
            0,
            0x0000_0002,
        )
    };
    if ok == 0 {
        return Err(last_os_error());
    }
    Ok(unsafe {
        // SAFETY: DuplicateHandle returned a unique owned handle.
        OwnedHandle::from_raw_handle(duplicate as _)
    })
}

fn exit_status(process: &OwnedHandle) -> io::Result<ExitStatus> {
    let mut code = 0u32;
    let ok = unsafe {
        // SAFETY: process is live and code is a writable out-pointer.
        GetExitCodeProcess(process.as_raw_handle() as HANDLE, &mut code)
    };
    if ok == 0 {
        return Err(last_os_error());
    }
    Ok(ExitStatus::from_raw(code))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessWaitState {
    Exited,
    Running,
    Failed(u32),
}

const fn classify_process_wait(status: u32, error: u32) -> ProcessWaitState {
    match status {
        WAIT_OBJECT_0 => ProcessWaitState::Exited,
        WAIT_TIMEOUT => ProcessWaitState::Running,
        WAIT_FAILED => ProcessWaitState::Failed(error),
        _ => ProcessWaitState::Failed(ERROR_INVALID_DATA),
    }
}

#[inline(never)]
fn wait_process_state(process: &OwnedHandle) -> ProcessWaitState {
    let wait = unsafe {
        // SAFETY: process is live; this bounded wait only observes state.
        WaitForSingleObject(process.as_raw_handle() as HANDLE, 500)
    };
    let error = if wait == WAIT_FAILED {
        last_error_code()
    } else {
        0
    };
    classify_process_wait(wait, error)
}

#[cfg(test)]
mod process_wait_tests {
    use super::*;

    #[test]
    fn native_wait_statuses_are_classified_without_allocating_errors() {
        assert_eq!(
            classify_process_wait(WAIT_OBJECT_0, 0),
            ProcessWaitState::Exited
        );
        assert_eq!(
            classify_process_wait(WAIT_TIMEOUT, 0),
            ProcessWaitState::Running
        );
        assert_eq!(
            classify_process_wait(WAIT_FAILED, ERROR_INVALID_HANDLE),
            ProcessWaitState::Failed(ERROR_INVALID_HANDLE)
        );
        assert_eq!(
            classify_process_wait(7, 0),
            ProcessWaitState::Failed(ERROR_INVALID_DATA)
        );
    }
}

fn terminate_process(process: &OwnedHandle, exit_code: u32) -> io::Result<()> {
    let terminated = unsafe {
        // SAFETY: process is live and TerminateProcess borrows the handle.
        TerminateProcess(process.as_raw_handle() as HANDLE, exit_code)
    };
    if terminated == 0 {
        return Err(last_os_error());
    }
    Ok(())
}

fn hresult_error(result: i32) -> io::Error {
    io::Error::from_raw_os_error(result)
}

fn is_benign_resize_after_exit(result: i32) -> bool {
    result == E_HANDLE
        || result == hresult_from_win32(ERROR_BROKEN_PIPE)
        || result == hresult_from_win32(ERROR_INVALID_PARAMETER)
}

fn hresult_from_win32(error: u32) -> i32 {
    if error == 0 {
        0
    } else {
        ((error & 0x0000_FFFF) | 0x8007_0000) as i32
    }
}

fn write_timeout(timeout: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "PTY write made no progress for {} ms",
            bounded_duration_millis(timeout)
        ),
    )
}

fn bounded_duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn wait_timeout_millis(timeout: Duration) -> u32 {
    let millis = timeout
        .as_millis()
        .saturating_add(u128::from(
            !timeout.subsec_nanos().is_multiple_of(1_000_000),
        ))
        .max(1);
    u32::try_from(millis)
        .unwrap_or(INFINITE - 1)
        .min(INFINITE - 1)
}

fn pty_error(
    operation: &'static str,
    code: &'static str,
    error: impl std::fmt::Display,
) -> PtyError {
    PtyError::failed(operation, code, error)
}

#[cfg(test)]
mod timeout_format_tests {
    use super::*;

    #[test]
    fn duration_millis_are_exact_or_saturate_before_formatting() {
        assert_eq!(
            bounded_duration_millis(Duration::from_millis(12_345)),
            12_345
        );
        assert_eq!(bounded_duration_millis(Duration::MAX), u64::MAX);
        assert_eq!(
            write_timeout(Duration::from_millis(250)).to_string(),
            "PTY write made no progress for 250 ms"
        );
    }
}

fn last_error_code() -> u32 {
    unsafe {
        // SAFETY: GetLastError reads the calling thread's last-error slot.
        GetLastError()
    }
}

fn last_os_error() -> io::Error {
    io::Error::from_raw_os_error(last_error_code() as i32)
}

fn resolve_application_path(command: &ChildCommand) -> io::Result<PathBuf> {
    if command.program.is_absolute() || has_path_component(&command.program) {
        let base = command.current_dir.clone().unwrap_or(env::current_dir()?);
        let candidate = if command.program.is_absolute() {
            command.program.clone()
        } else {
            base.join(&command.program)
        };
        let pathext = effective_env_value(command, "PATHEXT");
        if let Some(path) = resolve_application_candidate(&candidate, pathext.as_deref())? {
            return Ok(path);
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "ConPTY executable not found: {}",
                candidate.to_string_lossy()
            ),
        ));
    }

    let Some(path_value) = effective_env_value(command, "PATH") else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "ConPTY executable not found on PATH: {}",
                command.program.to_string_lossy()
            ),
        ));
    };
    let pathext = effective_env_value(command, "PATHEXT");
    let extensions = executable_extensions(&command.program, pathext.as_deref());
    let current_dir = command
        .current_dir
        .clone()
        .or_else(|| env::current_dir().ok());
    for directory in env::split_paths(&path_value) {
        let directory = if directory.is_absolute() {
            directory
        } else if let Some(current_dir) = &current_dir {
            current_dir.join(directory)
        } else {
            directory
        };
        for extension in &extensions {
            let candidate = append_extension(&directory.join(&command.program), extension);
            if let Ok(Some(path)) = resolve_exact_application_candidate(&candidate) {
                return Ok(path);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "ConPTY executable not found on PATH: {}",
            command.program.to_string_lossy()
        ),
    ))
}

fn has_path_component(path: &Path) -> bool {
    path.parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
}

fn resolve_application_candidate(
    path: &Path,
    pathext: Option<&OsStr>,
) -> io::Result<Option<PathBuf>> {
    for extension in executable_extensions(path, pathext) {
        if let Some(path) =
            resolve_exact_application_candidate(&append_extension(path, &extension))?
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn resolve_exact_application_candidate(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    if !is_direct_application_path(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "ConPTY executable must be .exe or .com: {}",
                path.to_string_lossy()
            ),
        ));
    }
    if path.is_absolute() {
        Ok(Some(path.to_owned()))
    } else {
        Ok(Some(env::current_dir()?.join(path)))
    }
}

fn executable_extensions(program: &Path, pathext: Option<&OsStr>) -> Vec<OsString> {
    if program.extension().is_some() {
        return vec![OsString::new()];
    }
    let mut extensions = vec![OsString::new()];
    append_direct_application_extensions(&mut extensions, pathext);
    extensions
}

fn append_direct_application_extensions(extensions: &mut Vec<OsString>, pathext: Option<&OsStr>) {
    let Some(pathext) = pathext else {
        extensions.push(OsString::from(".COM"));
        extensions.push(OsString::from(".EXE"));
        return;
    };
    let mut segment = [0_u16; 4];
    let mut length = 0_usize;
    let mut overflow = false;
    let mut saw_nonempty = false;
    for unit in pathext.encode_wide() {
        if unit == b';' as u16 {
            push_direct_application_extension(extensions, &segment[..length], overflow);
            length = 0;
            overflow = false;
        } else {
            saw_nonempty = true;
            if length < segment.len() {
                segment[length] = unit;
                length += 1;
            } else {
                overflow = true;
            }
        }
    }
    push_direct_application_extension(extensions, &segment[..length], overflow);
    if !saw_nonempty {
        extensions.push(OsString::from(".COM"));
        extensions.push(OsString::from(".EXE"));
    }
}

fn push_direct_application_extension(
    extensions: &mut Vec<OsString>,
    units: &[u16],
    overflow: bool,
) {
    if overflow {
        return;
    }
    match classify_exe_or_com(units, true) {
        Some(DirectApplicationExtension::Exe) => extensions.push(OsString::from(".EXE")),
        Some(DirectApplicationExtension::Com) => extensions.push(OsString::from(".COM")),
        None => {}
    }
}

fn append_extension(path: &Path, extension: &OsStr) -> PathBuf {
    let mut candidate = path.as_os_str().to_owned();
    candidate.push(extension);
    PathBuf::from(candidate)
}

fn is_direct_application_path(path: &Path) -> bool {
    path.extension()
        .map(|extension| is_exe_or_com(extension, false))
        .unwrap_or(true)
}

fn is_exe_or_com(value: &OsStr, allow_leading_dot: bool) -> bool {
    let mut buffered = [0_u16; 4];
    let mut length = 0;
    for unit in value.encode_wide() {
        if length == buffered.len() {
            return false;
        }
        buffered[length] = unit;
        length += 1;
    }
    classify_exe_or_com(&buffered[..length], allow_leading_dot).is_some()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectApplicationExtension {
    Exe,
    Com,
}

fn classify_exe_or_com(
    units: &[u16],
    allow_leading_dot: bool,
) -> Option<DirectApplicationExtension> {
    let units = if allow_leading_dot && units.first() == Some(&(b'.' as u16)) {
        &units[1..]
    } else {
        units
    };
    if units.len() != 3 {
        return None;
    }
    match (
        ascii_lower(units[0]),
        ascii_lower(units[1]),
        ascii_lower(units[2]),
    ) {
        (b'e', b'x', b'e') => Some(DirectApplicationExtension::Exe),
        (b'c', b'o', b'm') => Some(DirectApplicationExtension::Com),
        _ => None,
    }
}

const fn ascii_lower(unit: u16) -> u8 {
    if unit >= b'A' as u16 && unit <= b'Z' as u16 {
        (unit + (b'a' - b'A') as u16) as u8
    } else if unit <= u8::MAX as u16 {
        unit as u8
    } else {
        0
    }
}

#[cfg(test)]
mod application_extension_tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt as _;

    #[test]
    fn direct_application_extension_is_exact_ascii_and_allocation_free() {
        for extension in ["exe", "EXE", "com", "CoM"] {
            assert!(is_exe_or_com(OsStr::new(extension), false));
        }
        for extension in [".exe", ".EXE", ".com", ".CoM"] {
            assert!(is_exe_or_com(OsStr::new(extension), true));
        }
        for extension in ["", ".", "ex", "exe2", "..exe", ".bat", " exe"] {
            assert!(!is_exe_or_com(OsStr::new(extension), true));
        }
        let non_unicode = OsString::from_wide(&[b'e' as u16, 0xd800, b'e' as u16]);
        assert!(!is_exe_or_com(&non_unicode, false));
    }

    #[test]
    fn pathext_stream_preserves_fallback_order_and_filters_without_text_conversion() {
        let extensions = executable_extensions(
            Path::new("tool"),
            Some(OsStr::new(".BAT;EXE;.com;;.EXE;too-long")),
        );
        assert_eq!(
            extensions,
            ["", ".EXE", ".COM", ".EXE"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            executable_extensions(Path::new("tool"), Some(OsStr::new(";;;"))),
            ["", ".COM", ".EXE"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            executable_extensions(Path::new("tool"), Some(OsStr::new(".BAT"))),
            vec![OsString::new()]
        );
        assert!(os_str_eq_ascii_ignore_case(
            OsStr::new("PathExt"),
            "PATHEXT"
        ));
        assert!(!os_str_eq_ascii_ignore_case(
            OsStr::new("PATHEXT2"),
            "PATHEXT"
        ));
    }
}

fn effective_env_value(command: &ChildCommand, name: &str) -> Option<OsString> {
    command
        .env
        .iter()
        .rev()
        .find(|(key, _)| os_str_eq_ascii_ignore_case(key, name))
        .map(|(_, value)| value.clone())
        .or_else(|| env::var_os(name))
}

fn os_str_eq_ascii_ignore_case(value: &OsStr, ascii: &str) -> bool {
    let mut units = value.encode_wide();
    for byte in ascii.bytes() {
        if units.next().map(ascii_lower) != Some(ascii_lower(byte as u16)) {
            return false;
        }
    }
    units.next().is_none()
}

fn command_line(command: &ChildCommand) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    append_quoted_arg(&mut line, command.program.as_os_str())?;
    for arg in &command.args {
        line.push(b' ' as u16);
        append_quoted_arg(&mut line, arg)?;
    }
    line.push(0);
    Ok(line)
}

fn append_quoted_arg(output: &mut Vec<u16>, arg: &OsStr) -> io::Result<()> {
    const BACKSLASH: u16 = b'\\' as u16;
    const DOUBLE_QUOTE: u16 = b'"' as u16;
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;
    let units = arg.encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process argument contains NUL",
        ));
    }
    if !units
        .iter()
        .any(|unit| matches!(*unit, SPACE | TAB | DOUBLE_QUOTE))
    {
        output.extend(units);
        return Ok(());
    }

    output.push(DOUBLE_QUOTE);
    let mut backslashes = 0usize;
    for unit in units {
        match unit {
            BACKSLASH => backslashes += 1,
            DOUBLE_QUOTE => {
                output.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
                output.push(DOUBLE_QUOTE);
                backslashes = 0;
            }
            _ => {
                output.extend(std::iter::repeat_n(BACKSLASH, backslashes));
                backslashes = 0;
                output.push(unit);
            }
        }
    }
    output.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    output.push(DOUBLE_QUOTE);
    Ok(())
}

fn wide_null(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units = value.encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process path contains NUL",
        ));
    }
    units.push(0);
    Ok(units)
}

fn environment_block(command: &ChildCommand) -> io::Result<Option<Vec<u16>>> {
    if command.env.is_empty() {
        return Ok(None);
    }
    let overrides = EnvironmentOverrides::from_command(command)?;
    let inherited = crate::selected::environment::InheritedEnvironment::capture()?;
    Ok(Some(merge_environment_block(
        inherited.units()?,
        &overrides,
    )))
}

struct EncodedEnvironmentEntry {
    key_len: usize,
    units: Vec<u16>,
}

impl EncodedEnvironmentEntry {
    fn key(&self) -> &[u16] {
        &self.units[..self.key_len]
    }
}

struct EnvironmentOverrides(Vec<EncodedEnvironmentEntry>);

impl EnvironmentOverrides {
    fn from_command(command: &ChildCommand) -> io::Result<Self> {
        let mut overrides = Self(Vec::new());
        for (key, value) in &command.env {
            let key = key.encode_wide().collect::<Vec<_>>();
            let value = value.encode_wide().collect::<Vec<_>>();
            if key.is_empty()
                || key.iter().any(|unit| *unit == 0 || *unit == b'=' as u16)
                || value.contains(&0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "environment key is empty, contains '=' or NUL, or value contains NUL",
                ));
            }
            let key_len = key.len();
            let mut units = key;
            units.push(b'=' as u16);
            units.extend(value);
            overrides.insert(EncodedEnvironmentEntry { key_len, units });
        }
        Ok(overrides)
    }

    fn insert(&mut self, entry: EncodedEnvironmentEntry) {
        let mut low = 0usize;
        let mut high = self.0.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if compare_environment_keys(self.0[middle].key(), entry.key()) == CmpOrdering::Less {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if self
            .0
            .get(low)
            .is_some_and(|current| compare_environment_keys(current.key(), entry.key()).is_eq())
        {
            self.0[low] = entry;
        } else {
            self.0.insert(low, entry);
        }
    }
}

fn merge_environment_block(inherited: &[u16], overrides: &EnvironmentOverrides) -> Vec<u16> {
    let mut block = Vec::new();
    let mut inherited_at = 0usize;
    let mut override_at = 0usize;
    while inherited_at < inherited.len() && inherited[inherited_at] != 0 {
        let entry_end = inherited[inherited_at..]
            .iter()
            .position(|unit| *unit == 0)
            .map_or(inherited.len(), |offset| inherited_at + offset);
        let entry = &inherited[inherited_at..entry_end];
        let Some(key) = environment_entry_key(entry) else {
            inherited_at = entry_end.saturating_add(1);
            continue;
        };
        let mut keep_inherited = true;
        while let Some(override_entry) = overrides.0.get(override_at) {
            match compare_environment_keys(override_entry.key(), key) {
                CmpOrdering::Less => {
                    block.extend_from_slice(&override_entry.units);
                    block.push(0);
                    override_at += 1;
                }
                CmpOrdering::Equal => {
                    block.extend_from_slice(&override_entry.units);
                    block.push(0);
                    override_at += 1;
                    keep_inherited = false;
                    break;
                }
                CmpOrdering::Greater => break,
            }
        }
        if keep_inherited {
            block.extend_from_slice(entry);
            block.push(0);
        }
        inherited_at = entry_end.saturating_add(1);
    }
    for entry in &overrides.0[override_at..] {
        block.extend_from_slice(&entry.units);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

fn environment_entry_key(entry: &[u16]) -> Option<&[u16]> {
    let search_start = usize::from(entry.first() == Some(&(b'=' as u16)));
    let separator = entry
        .get(search_start..)?
        .iter()
        .position(|unit| *unit == b'=' as u16)?
        + search_start;
    (separator != 0).then_some(&entry[..separator])
}

fn compare_environment_keys(left: &[u16], right: &[u16]) -> CmpOrdering {
    for (left, right) in left.iter().copied().zip(right.iter().copied()) {
        match ascii_upper_unit(left).cmp(&ascii_upper_unit(right)) {
            CmpOrdering::Equal => {}
            ordering => return ordering,
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod sorted_environment_tests {
    use super::*;

    #[test]
    fn native_environment_merge_preserves_drive_entries_and_replaces_case_insensitively() {
        let command = ChildCommand::new("cmd.exe")
            .env("Path", "new")
            .env("alpha", "first")
            .env("ALPHA", "two");
        let overrides = EnvironmentOverrides::from_command(&command).expect("valid overrides");
        let inherited = "=C:=C:\\old\0alpha=old\0PATH=old\0ZED=last\0\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let actual = String::from_utf16(&merge_environment_block(&inherited, &overrides))
            .expect("valid UTF-16");
        assert_eq!(actual, "=C:=C:\\old\0ALPHA=two\0Path=new\0ZED=last\0\0");
    }

    #[test]
    fn native_environment_merge_validates_overrides_and_empty_double_nul() {
        let invalid = ChildCommand::new("cmd.exe").env("BAD=KEY", "value");
        assert!(EnvironmentOverrides::from_command(&invalid).is_err());

        let command = ChildCommand::new("cmd.exe").env("TERM", "xterm");
        let overrides = EnvironmentOverrides::from_command(&command).expect("valid override");
        assert_eq!(
            merge_environment_block(&[0], &overrides),
            "TERM=xterm\0\0".encode_utf16().collect::<Vec<_>>()
        );
    }
}

fn ascii_upper_unit(unit: u16) -> u16 {
    if (b'a' as u16..=b'z' as u16).contains(&unit) {
        unit - 32
    } else {
        unit
    }
}

#[cfg(test)]
fn native_size_for_test(size: TerminalSize) -> COORD {
    coord_from_size(size).expect("test size is valid")
}

#[cfg(test)]
mod tests {
    use super::{
        DsrBootstrap, NativeInputOwnership, NativeTerminalKey, PASSTHROUGH_MIN_BUILD, ProcessId,
        TerminalSize, ascii_upper_unit, base_conpty_flags, classify_console_input_mode,
        command_line, native_console_key_event, native_size_for_test, select_conpty_flags,
    };
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn native_size_preserves_neutral_row_and_column_order() {
        let native = native_size_for_test(TerminalSize { rows: 24, cols: 80 });

        assert_eq!(native.X, 80);
        assert_eq!(native.Y, 24);
    }

    #[test]
    fn neutral_process_id_rejects_zero() {
        assert!(ProcessId::new(0).is_err());
        assert_eq!(ProcessId::new(42).expect("valid process id").as_u32(), 42);
    }

    #[test]
    fn native_cursor_keys_preserve_win32_identity_and_repeat() {
        let up = native_console_key_event(NativeTerminalKey::Up, 3);
        assert_eq!(up.virtual_key_code, 0x26);
        assert_eq!(up.virtual_scan_code, 0x48);
        assert_eq!(up.unicode_char, 0);
        assert_eq!(up.control_key_state, super::ENHANCED_KEY);
        assert_eq!(up.repeat_count, 3);

        let down = native_console_key_event(NativeTerminalKey::Down, 7);
        assert_eq!(down.virtual_key_code, 0x28);
        assert_eq!(down.virtual_scan_code, 0x50);
        assert_eq!(down.unicode_char, 0);
        assert_eq!(down.control_key_state, super::ENHANCED_KEY);
        assert_eq!(down.repeat_count, 7);
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

    #[test]
    fn command_line_quotes_spaces_quotes_and_trailing_backslashes() {
        let command = super::ChildCommand::new("prog.exe").arg(r#"C:\two words\ends\"#);
        let line = command_line(&command).expect("command line");
        let rendered = String::from_utf16_lossy(&line[..line.len() - 1]);

        assert_eq!(rendered, r#"prog.exe "C:\two words\ends\\""#);
    }

    #[test]
    fn command_line_preserves_non_unicode_wide_units() {
        let raw_arg = OsString::from_wide(&[0xD800]);
        let command = super::ChildCommand::new("prog.exe").arg(raw_arg);
        let line = command_line(&command).expect("command line");

        assert!(line.contains(&0xD800));
    }

    #[test]
    fn conpty_flags_keep_native_input_and_resize_contract() {
        assert_eq!(base_conpty_flags(), 0x6);
        assert_eq!(select_conpty_flags(Some(PASSTHROUGH_MIN_BUILD), false), 0xE);
        assert_eq!(
            select_conpty_flags(Some(PASSTHROUGH_MIN_BUILD - 1), false),
            0x6
        );
        assert_eq!(select_conpty_flags(None, false), 0x6);
        assert_eq!(select_conpty_flags(Some(PASSTHROUGH_MIN_BUILD), true), 0x6);
    }

    /// The load-time-to-call-time move is only real if the run-time lookup
    /// actually finds the exports where a static import would have. Agreeing
    /// with the OS build number in both directions proves the lookup works on
    /// a system that has ConPTY, and — on an older host — that the adapter
    /// reports absence instead of calling through a null pointer.
    #[test]
    fn conpty_resolution_agrees_with_the_reported_windows_build() {
        let build = super::current_windows_build().expect("Windows reports its build");
        assert_eq!(
            super::conpty::is_available(),
            build >= super::CONPTY_MIN_BUILD,
            "ConPTY availability must track build {build}, not a static import"
        );
    }

    /// A refusal that names a symbol is unactionable; one that names a version
    /// tells the reader what to do about it.
    #[test]
    fn an_absent_conpty_is_explained_as_a_windows_version_requirement() {
        let reason = super::conpty::unavailable_reason();
        assert!(reason.contains("17763"), "the required build is named");
        assert!(reason.contains("1809"), "the marketing name is named");
        assert!(
            !reason.contains("CreatePseudoConsole"),
            "a bare export name is what this change exists to stop showing users"
        );
    }

    #[test]
    fn environment_key_normalization_is_ascii_case_insensitive() {
        assert_eq!(ascii_upper_unit(b'a' as u16), b'A' as u16);
        assert_eq!(ascii_upper_unit(0x4E2D), 0x4E2D);
    }

    #[test]
    fn powershell_dsr_is_removed_across_read_fragments() {
        let mut helper = DsrBootstrap {
            deadline: Instant::now() + Duration::from_secs(1),
            completed: false,
            pending: Vec::new(),
        };
        let first = helper.filter(b"before\x1b[");
        assert_eq!(first.0, b"before");
        assert_eq!(first.1, None);

        let second = helper.filter(b"6nafter");
        assert_eq!(second.0, b"after");
        assert_eq!(second.1, Some(super::DSR_RESPONSE));
    }

    #[test]
    fn powershell_dsr_timeout_flushes_pending_bytes() {
        let mut helper = DsrBootstrap {
            deadline: Instant::now() - Duration::from_millis(1),
            completed: false,
            pending: b"\x1b[".to_vec(),
        };
        let filtered = helper.filter(b"X");

        assert_eq!(filtered.0, b"\x1b[X");
        assert_eq!(filtered.1, None);
    }

    #[test]
    fn powershell_detection_is_basename_only() {
        assert!(super::should_enable_dsr_bootstrap(Path::new(
            r"C:\Program Files\PowerShell\7\pwsh.exe"
        )));
        assert!(!super::should_enable_dsr_bootstrap(Path::new("vim.exe")));
    }

    #[test]
    fn direct_conpty_delivers_real_child_output_before_eof() {
        let shell = std::env::var_os("COMSPEC").expect("Windows COMSPEC");
        let spawned = super::ChildCommand::new(shell)
            .arg("/D")
            .arg("/C")
            .arg("echo agenterm-direct-conpty")
            .spawn()
            .expect("spawn direct ConPTY child");
        let (master, mut child) = spawned.into_parts();
        let status = child.wait().expect("wait for direct ConPTY child");
        assert!(status.success(), "child status: {status:?}");
        child.close_pseudoconsole();

        let mut output = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let amount = master.io().read(&mut chunk).expect("read ConPTY output");
            if amount == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..amount]);
        }
        let rendered = String::from_utf8_lossy(&output);
        assert!(
            rendered.contains("agenterm-direct-conpty"),
            "missing child output: {rendered:?}"
        );
    }

    #[test]
    fn direct_conpty_inherits_system_environment_and_applies_override() {
        let shell = std::env::var_os("COMSPEC").expect("Windows COMSPEC");
        let expected_shell = shell.to_string_lossy().into_owned();
        let spawned = super::ChildCommand::new(shell)
            .arg("/D")
            .arg("/C")
            .arg("echo inherited=%COMSPEC% override=%AGENTERM_PTY_ENV_TEST%")
            .env("AGENTERM_PTY_ENV_TEST", "native-block")
            .spawn()
            .expect("spawn direct ConPTY child with environment override");
        let (master, mut child) = spawned.into_parts();
        let status = child.wait().expect("wait for direct ConPTY child");
        assert!(status.success(), "child status: {status:?}");
        child.close_pseudoconsole();

        let mut output = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let amount = master.io().read(&mut chunk).expect("read ConPTY output");
            if amount == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..amount]);
        }
        let rendered = String::from_utf8_lossy(&output);
        let expected = format!("inherited={expected_shell} override=native-block");
        assert!(
            rendered.contains(&expected),
            "missing exact inherited environment and override {expected:?}: {rendered:?}"
        );
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version_information: *mut OSVERSIONINFOEXW) -> i32;
}
