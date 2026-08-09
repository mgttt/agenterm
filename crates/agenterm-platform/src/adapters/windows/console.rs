//! Shared Windows console attachment guard.
//!
//! Every adapter that needs `AttachConsole` / `FreeConsole` must go through
//! this module — the process-wide console attachment is serialized under one
//! lock. Callers outside this module must not call `AttachConsole` / `FreeConsole`
//! directly.

use std::io;
use std::os::windows::io::{FromRawHandle as _, OwnedHandle};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler, SetStdHandle,
};

static LOCK: Mutex<()> = Mutex::new(());

/// Acquire the console serialization lock without attaching.
///
/// Use this when the caller (or a library it delegates to) manages its own
/// `AttachConsole` / `FreeConsole` calls but must still be serialized with
/// other console users in this process.
pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().expect("console attach lock poisoned")
}

/// RAII guard that attaches to a process's console and releases on drop.
///
/// The guard also suppresses `Ctrl+C` / `Ctrl+Break` events while attached
/// so that a child-console control event never reaches the host process.
pub(crate) struct ConsoleGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _control_ignore: Option<ConsoleControlIgnoreGuard>,
    redirected: Option<RedirectedHandles>,
}

impl ConsoleGuard {
    /// Attach to the console of the calling process's parent.
    ///
    /// On success, also redirects stdout, stderr, and stdin to the attached
    /// console so that `println!` / `eprintln!` / stdin reads work from a
    /// GUI-subsystem (`windows_subsystem = "windows"`) binary.
    ///
    /// Fails when there is no parent console — the typical case for a
    /// GUI-subsystem binary launched by double-click.
    pub(crate) fn attach_parent() -> io::Result<Self> {
        Self::attach_parent_impl(true)
    }

    /// Like `attach_parent`, but leaves the default console control
    /// behavior in place so `Ctrl+C` terminates this process — the contract
    /// a CLI worker must honor for parity with a console-subsystem binary.
    pub(crate) fn attach_parent_with_default_interrupts() -> io::Result<Self> {
        Self::attach_parent_impl(false)
    }

    fn attach_parent_impl(ignore_control_events: bool) -> io::Result<Self> {
        let lock = LOCK
            .lock()
            .map_err(|_| io::Error::other("console attach lock poisoned"))?;
        // Capture the std slots before `AttachConsole`: a valid pre-attach
        // handle is a caller redirection (pipe or file) that must survive
        // the attach, which may repoint the slots at console handles.
        let pre_attach = StdHandleSnapshot::capture();
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
        if attached == 0 {
            return Err(io::Error::last_os_error());
        }
        let control_ignore = if ignore_control_events {
            match ConsoleControlIgnoreGuard::install() {
                Ok(guard) => Some(guard),
                Err(error) => {
                    let _ = unsafe { FreeConsole() };
                    return Err(error);
                }
            }
        } else {
            None
        };
        let redirected = match RedirectedHandles::install(&pre_attach) {
            Ok(handles) => handles,
            Err(error) => {
                let _ = unsafe { FreeConsole() };
                return Err(error);
            }
        };
        Ok(Self {
            _lock: lock,
            _control_ignore: control_ignore,
            redirected: Some(redirected),
        })
    }

    /// Attach to the console of the target process.
    ///
    /// Unlike `attach_parent`, this does *not* redirect the host process's
    /// stdio handles — the host is intended to inspect the child console,
    /// not take it over for its own I/O.
    pub(crate) fn attach_process(process_id: u32) -> io::Result<Self> {
        let lock = LOCK
            .lock()
            .map_err(|_| io::Error::other("console attach lock poisoned"))?;
        // SAFETY: `AttachConsole` validates the non-zero process id.
        let attached = unsafe { AttachConsole(process_id) };
        if attached == 0 {
            return Err(io::Error::last_os_error());
        }
        match ConsoleControlIgnoreGuard::install() {
            Ok(control_ignore) => Ok(Self {
                _lock: lock,
                _control_ignore: Some(control_ignore),
                redirected: None,
            }),
            Err(error) => {
                let _ = unsafe { FreeConsole() };
                Err(error)
            }
        }
    }
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        if let Some(handles) = &self.redirected {
            handles.restore();
        }
        // SAFETY: Releases the console attachment.
        let _ = unsafe { FreeConsole() };
    }
}

/// Std handle values as they were at capture time.
struct StdHandleSnapshot {
    input: HANDLE,
    output: HANDLE,
    error: HANDLE,
}

impl StdHandleSnapshot {
    fn capture() -> Self {
        unsafe {
            Self {
                input: GetStdHandle(STD_INPUT_HANDLE),
                output: GetStdHandle(STD_OUTPUT_HANDLE),
                error: GetStdHandle(STD_ERROR_HANDLE),
            }
        }
    }
}

/// Settle the std slots after `AttachConsole` so every stream is real.
///
/// Per stream, in preference order: the caller's pre-attach redirection
/// (pipe or file handle from process startup — put back if the attach
/// repointed the slot), whatever valid handle the attach installed, else a
/// `CONOUT$` / `CONIN$` handle opened with `GENERIC_READ | GENERIC_WRITE`
/// (required by `WriteConsoleW` on modern Windows). Rust's `println!` uses
/// `GetStdHandle` at first-write time, so this correctly wires output —
/// provided `std::io::stdout()` has not already been cached (the startup
/// code avoids this by using `write_parent_console`).
struct RedirectedHandles {
    _output: Option<OwnedHandle>,
    _input: Option<OwnedHandle>,
    restores: [Option<(STD_HANDLE, HANDLE)>; 3],
}

impl RedirectedHandles {
    fn install(pre_attach: &StdHandleSnapshot) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle as _;

        let slots = [
            (STD_INPUT_HANDLE, pre_attach.input, true),
            (STD_OUTPUT_HANDLE, pre_attach.output, false),
            (STD_ERROR_HANDLE, pre_attach.error, false),
        ];
        let mut console_output: Option<OwnedHandle> = None;
        let mut console_input: Option<OwnedHandle> = None;
        let mut restores: [Option<(STD_HANDLE, HANDLE)>; 3] = [None; 3];
        let mut applied: Vec<(STD_HANDLE, HANDLE)> = Vec::new();
        for (index, (which, pre, is_input)) in slots.into_iter().enumerate() {
            let current = unsafe { GetStdHandle(which) };
            let desired = if valid_handle(pre) {
                pre
            } else if valid_handle(current) {
                current
            } else {
                let cache = if is_input {
                    &mut console_input
                } else {
                    &mut console_output
                };
                if cache.is_none() {
                    let name = if is_input {
                        windows_sys::core::w!("CONIN$")
                    } else {
                        windows_sys::core::w!("CONOUT$")
                    };
                    match open_console_file(name, GENERIC_READ | GENERIC_WRITE) {
                        Ok(handle) => *cache = Some(handle),
                        Err(error) => {
                            rollback_std_handles(&applied);
                            return Err(error);
                        }
                    }
                }
                cache.as_ref().expect("console file handle").as_raw_handle() as HANDLE
            };
            if desired == current {
                continue;
            }
            if unsafe { SetStdHandle(which, desired) } == 0 {
                let error = io::Error::last_os_error();
                rollback_std_handles(&applied);
                return Err(error);
            }
            applied.push((which, current));
            // Slots healed back to a caller redirection keep it past drop;
            // console-file slots must return to their pre-attach value
            // because the owned console handle dies with this guard.
            if !valid_handle(pre) {
                restores[index] = Some((which, pre));
            }
        }
        Ok(Self {
            _output: console_output,
            _input: console_input,
            restores,
        })
    }

    fn restore(&self) {
        for (which, handle) in self.restores.iter().flatten() {
            unsafe { SetStdHandle(*which, *handle) };
        }
    }
}

fn rollback_std_handles(applied: &[(STD_HANDLE, HANDLE)]) {
    for (which, handle) in applied.iter().rev() {
        unsafe { SetStdHandle(*which, *handle) };
    }
}

/// Duplicate the process's current std handles for explicit child wiring.
///
/// Returns `[stdin, stdout, stderr]`; a slot is `None` when the process
/// holds no valid handle for that stream. Callers convert the duplicates
/// into `std::process::Stdio` so a GUI-subsystem child receives real
/// handles at startup regardless of what this process's cached Rust stdio
/// saw, and independent of when the console guard is dropped.
pub(crate) fn duplicate_std_handles() -> [Option<OwnedHandle>; 3] {
    [
        duplicate_std_handle(STD_INPUT_HANDLE),
        duplicate_std_handle(STD_OUTPUT_HANDLE),
        duplicate_std_handle(STD_ERROR_HANDLE),
    ]
}

fn duplicate_std_handle(which: STD_HANDLE) -> Option<OwnedHandle> {
    use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let source = unsafe { GetStdHandle(which) };
    if !valid_handle(source) {
        return None;
    }
    let process = unsafe { GetCurrentProcess() };
    let mut duplicated: HANDLE = std::ptr::null_mut();
    let succeeded = unsafe {
        DuplicateHandle(
            process,
            source,
            process,
            &mut duplicated,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if succeeded == 0 || !valid_handle(duplicated) {
        return None;
    }
    // SAFETY: `DuplicateHandle` produced a handle this process owns.
    Some(unsafe { OwnedHandle::from_raw_handle(duplicated as _) })
}

fn valid_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
}

fn open_console_file(name: *const u16, access: u32) -> io::Result<OwnedHandle> {
    let handle = unsafe {
        CreateFileW(
            name,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

/// Installs a `Ctrl+C` handler that swallows the event, preventing the
/// host process from being terminated by a control signal sent to the
/// attached console.
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
            // SAFETY: Removes the exact callback installed by `install`.
            SetConsoleCtrlHandler(Some(ignore_console_control_event), 0)
        };
    }
}

unsafe extern "system" fn ignore_console_control_event(_control_type: u32) -> i32 {
    1
}
